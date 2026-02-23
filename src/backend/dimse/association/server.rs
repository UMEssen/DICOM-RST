use super::{oneshot, AskPattern, Association, AssociationError, ChannelError, Command, Sender};
use dicom::transfer_syntax::TransferSyntaxRegistry;
use dicom::ul::pdu::Pdu;
use dicom::ul::pdu::PresentationContextNegotiated;
use std::convert::identity;
use std::io::ErrorKind;
use std::{net::TcpStream, thread, time::Duration};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

#[derive(Debug)]
pub struct ServerAssociation {
	channel: Sender<Command>,
	presentation_contexts: Vec<PresentationContextNegotiated>,
	tcp_stream: TcpStream,
}

pub struct ServerAssociationOptions {
	pub aet: String,
	pub tcp_stream: TcpStream,
	pub uncompressed: bool,
}

impl ServerAssociation {
	pub async fn new(options: ServerAssociationOptions) -> Result<Self, AssociationError> {
		let uuid = Uuid::new_v4();
		let mut server_options = dicom::ul::ServerAssociationOptions::new()
			.ae_title(options.aet.clone())
			.promiscuous(true);

		for syntax in TransferSyntaxRegistry.iter() {
			if (options.uncompressed && syntax.is_codec_free())
				|| (!options.uncompressed && !syntax.is_unsupported())
			{
				server_options = server_options.with_transfer_syntax(syntax.uid());
			}
		}

		let (connect_tx, connect_result) = oneshot::channel::<Result<_, AssociationError>>();

		let (tx, mut rx) = tokio::sync::mpsc::channel::<Command>(1);
		let _handle = thread::Builder::new()
			.name(format!("{}-server", options.aet))
			.spawn(move || {
				let span =
					tracing::info_span!("ServerAssociation", association_id = uuid.to_string());
				let _enter = span.enter();

				let mut association = match server_options.establish(options.tcp_stream) {
					Ok(mut association) => {
						info!(
							calling_aet = association.client_ae_title(),
							called_aet = options.aet,
							"Established new server association"
						);

						let pcs = association.presentation_contexts().to_vec();

						let stream = association
							.inner_stream()
							.try_clone()
							.expect("TcpStream::clone");

						connect_tx.send(Ok((stream, pcs))).map_err(|_value| ())?;
						association
					}
					Err(e) => {
						connect_tx.send(Err(e.into())).map_err(|_value| ())?;
						return Err(());
					}
				};

				while let Some(command) = rx.blocking_recv() {
					let result = match command {
						Command::Send(pdu, response) => {
							let send_result = association
								.send(&pdu)
								.map_err(AssociationError::Association);
							response
								.send(send_result)
								.map_err(|_value| ChannelError::Closed)
						}
						Command::Receive(response) => {
							let receive_result =
								association.receive().map_err(AssociationError::Association);
							response
								.send(receive_result)
								.map_err(|_value| ChannelError::Closed)
						}
					};

					if let Some(err) = result.err() {
						error!("Error in ServerAssociation: {err}");
						return Err(());
					}
				}

				rx.close();

				if let Err(e) = association.abort() {
					match e {
						dicom::ul::association::Error::WireSend { source, .. }
							if source.kind() == ErrorKind::BrokenPipe =>
						{
							// no-op, happens on MacOS if the TCP stream is already closed
						}
						_ => {
							warn!("ServerAssociation.abort() returned error: {e}");
						}
					}
				}

				Ok(())
			})
			.map_err(AssociationError::OsThread)?;

		let (tcp_stream, presentation_contexts) =
			connect_result.await.expect("connect_result.await")?;

		Ok(Self {
			channel: tx,
			presentation_contexts,
			tcp_stream,
		})
	}
}

impl Association for ServerAssociation {
	async fn receive(&self, timeout: Duration) -> Result<Pdu, AssociationError> {
		self.channel
			.ask(Command::Receive, timeout)
			.await
			.map_err(AssociationError::Channel)
			.and_then(identity)
	}

	async fn send(&self, pdu: Pdu, timeout: Duration) -> Result<(), AssociationError> {
		self.channel
			.ask(|reply_to| Command::Send(pdu, reply_to), timeout)
			.await
			.map_err(AssociationError::Channel)
			.and_then(identity)
	}

	fn close(&mut self) {
		debug!("Closing TcpStream from outside");

		if let Err(err) = self.tcp_stream.shutdown(std::net::Shutdown::Both) {
			warn!("TcpStream::shutdown failed: {err}");
		}
	}

	fn presentation_contexts(&self) -> &[PresentationContextNegotiated] {
		&self.presentation_contexts
	}
}

impl Drop for ServerAssociation {
	fn drop(&mut self) {
		self.close();
	}
}
