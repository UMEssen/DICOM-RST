use super::{oneshot, AskPattern, Association, AssociationError, ChannelError, Command, Sender};
use dicom::transfer_syntax::TransferSyntaxRegistry;
use dicom::ul::{pdu::PresentationContextResult, Pdu};
use std::convert::identity;
use std::io::ErrorKind;
use std::{net::TcpStream, thread, time::Duration};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

#[derive(Debug)]
pub struct ServerAssociation {
	uuid: Uuid,
	channel: Sender<Command>,
	presentation_contexts: Vec<PresentationContextResult>,
	tcp_stream: TcpStream,
}

pub struct ServerAssociationOptions {
	pub aet: String,
	pub tcp_stream: TcpStream,
}

impl ServerAssociation {
	pub async fn new(options: ServerAssociationOptions) -> Result<Self, AssociationError> {
		let uuid = Uuid::new_v4();
		let mut server_options =
			dicom::ul::ServerAssociationOptions::new().ae_title(options.aet.clone());

		for syntax in TransferSyntaxRegistry.iter() {
			if !syntax.is_unsupported() {
				server_options = server_options.with_transfer_syntax(syntax.uid());
			}
		}

		for syntax in ABSTRACT_SYNTAXES {
			server_options = server_options.with_abstract_syntax(syntax);
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

						let pcs = association
							.presentation_contexts()
							.into_iter()
							.cloned()
							.collect();

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
							let send_result = association.send(&pdu).map_err(|e| e.into());
							response
								.send(send_result)
								.map_err(|_value| ChannelError::Closed)
						}
						Command::Receive(response) => {
							let receive_result =
								association.receive().map_err(AssociationError::Server);
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
						dicom::ul::association::server::Error::WireSend { source, .. }
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
			uuid,
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

	fn presentation_contexts(&self) -> &[PresentationContextResult] {
		&self.presentation_contexts
	}
}

impl Drop for ServerAssociation {
	fn drop(&mut self) {
		self.close();
	}
}

// TODO: Use named variables from dicom::dictionary_std::uids
/// All Standard SOP Classes
/// <https://dicom.nema.org/dicom/2013/output/chtml/part04/sect_B.5.html#table_B.5-1>
pub static ABSTRACT_SYNTAXES: [&str; 111] = [
	"1.2.840.10008.5.1.4.1.1.1",
	"1.2.840.10008.5.1.4.1.1.1.1",
	"1.2.840.10008.5.1.4.1.1.1.1.1",
	"1.2.840.10008.5.1.4.1.1.1.2",
	"1.2.840.10008.5.1.4.1.1.1.2.1",
	"1.2.840.10008.5.1.4.1.1.1.3",
	"1.2.840.10008.5.1.4.1.1.1.3.1",
	"1.2.840.10008.5.1.4.1.1.2",
	"1.2.840.10008.5.1.4.1.1.2.1",
	"1.2.840.10008.5.1.4.1.1.2.2",
	"1.2.840.10008.5.1.4.1.1.3.1",
	"1.2.840.10008.5.1.4.1.1.4",
	"1.2.840.10008.5.1.4.1.1.4.1",
	"1.2.840.10008.5.1.4.1.1.4.2",
	"1.2.840.10008.5.1.4.1.1.4.3",
	"1.2.840.10008.5.1.4.1.1.4.4",
	"1.2.840.10008.5.1.4.1.1.6.1",
	"1.2.840.10008.5.1.4.1.1.6.2",
	"1.2.840.10008.5.1.4.1.1.7",
	"1.2.840.10008.5.1.4.1.1.7.1",
	"1.2.840.10008.5.1.4.1.1.7.2",
	"1.2.840.10008.5.1.4.1.1.7.3",
	"1.2.840.10008.5.1.4.1.1.7.4",
	"1.2.840.10008.5.1.4.1.1.9.1.1",
	"1.2.840.10008.5.1.4.1.1.9.1.2",
	"1.2.840.10008.5.1.4.1.1.9.1.3",
	"1.2.840.10008.5.1.4.1.1.9.2.1",
	"1.2.840.10008.5.1.4.1.1.9.3.1",
	"1.2.840.10008.5.1.4.1.1.9.4.1",
	"1.2.840.10008.5.1.4.1.1.9.4.2",
	"1.2.840.10008.5.1.4.1.1.9.5.1",
	"1.2.840.10008.5.1.4.1.1.9.6.1",
	"1.2.840.10008.5.1.4.1.1.11.1",
	"1.2.840.10008.5.1.4.1.1.11.2",
	"1.2.840.10008.5.1.4.1.1.11.3",
	"1.2.840.10008.5.1.4.1.1.11.4",
	"1.2.840.10008.5.1.4.1.1.11.5",
	"1.2.840.10008.5.1.4.1.1.12.1",
	"1.2.840.10008.5.1.4.1.1.12.1.1",
	"1.2.840.10008.5.1.4.1.1.12.2",
	"1.2.840.10008.5.1.4.1.1.12.2.1",
	"1.2.840.10008.5.1.4.1.1.13.1.1",
	"1.2.840.10008.5.1.4.1.1.13.1.2",
	"1.2.840.10008.5.1.4.1.1.13.1.3",
	"1.2.840.10008.5.1.4.1.1.14.1",
	"1.2.840.10008.5.1.4.1.1.14.2",
	"1.2.840.10008.5.1.4.1.1.20",
	"1.2.840.10008.5.1.4.1.1.66",
	"1.2.840.10008.5.1.4.1.1.66.1",
	"1.2.840.10008.5.1.4.1.1.66.2",
	"1.2.840.10008.5.1.4.1.1.66.3",
	"1.2.840.10008.5.1.4.1.1.66.4",
	"1.2.840.10008.5.1.4.1.1.66.5",
	"1.2.840.10008.5.1.4.1.1.67",
	"1.2.840.10008.5.1.4.1.1.68.1",
	"1.2.840.10008.5.1.4.1.1.68.2",
	"1.2.840.10008.5.1.4.1.1.77.1.1",
	"1.2.840.10008.5.1.4.1.1.77.1.1.1",
	"1.2.840.10008.5.1.4.1.1.77.1.2",
	"1.2.840.10008.5.1.4.1.1.77.1.2.1",
	"1.2.840.10008.5.1.4.1.1.77.1.3",
	"1.2.840.10008.5.1.4.1.1.77.1.4",
	"1.2.840.10008.5.1.4.1.1.77.1.4.1",
	"1.2.840.10008.5.1.4.1.1.77.1.5.1",
	"1.2.840.10008.5.1.4.1.1.77.1.5.2",
	"1.2.840.10008.5.1.4.1.1.77.1.5.3",
	"1.2.840.10008.5.1.4.1.1.77.1.5.4",
	"1.2.840.10008.5.1.4.1.1.77.1.6",
	"1.2.840.10008.5.1.4.1.1.78.1",
	"1.2.840.10008.5.1.4.1.1.78.2",
	"1.2.840.10008.5.1.4.1.1.78.3",
	"1.2.840.10008.5.1.4.1.1.78.4",
	"1.2.840.10008.5.1.4.1.1.78.5",
	"1.2.840.10008.5.1.4.1.1.78.6",
	"1.2.840.10008.5.1.4.1.1.78.7",
	"1.2.840.10008.5.1.4.1.1.78.8",
	"1.2.840.10008.5.1.4.1.1.79.1",
	"1.2.840.10008.5.1.4.1.1.80.1",
	"1.2.840.10008.5.1.4.1.1.81.1",
	"1.2.840.10008.5.1.4.1.1.82.1",
	"1.2.840.10008.5.1.4.1.1.88.11",
	"1.2.840.10008.5.1.4.1.1.88.22",
	"1.2.840.10008.5.1.4.1.1.88.33",
	"1.2.840.10008.5.1.4.1.1.88.34",
	"1.2.840.10008.5.1.4.1.1.88.40",
	"1.2.840.10008.5.1.4.1.1.88.50",
	"1.2.840.10008.5.1.4.1.1.88.59",
	"1.2.840.10008.5.1.4.1.1.88.65",
	"1.2.840.10008.5.1.4.1.1.88.67",
	"1.2.840.10008.5.1.4.1.1.88.69",
	"1.2.840.10008.5.1.4.1.1.88.70",
	"1.2.840.10008.5.1.4.1.1.104.1",
	"1.2.840.10008.5.1.4.1.1.104.2",
	"1.2.840.10008.5.1.4.1.1.128",
	"1.2.840.10008.5.1.4.1.1.130",
	"1.2.840.10008.5.1.4.1.1.128.1",
	"1.2.840.10008.5.1.4.1.1.131",
	"1.2.840.10008.5.1.4.1.1.481.1",
	"1.2.840.10008.5.1.4.1.1.481.2",
	"1.2.840.10008.5.1.4.1.1.481.3",
	"1.2.840.10008.5.1.4.1.1.481.4",
	"1.2.840.10008.5.1.4.1.1.481.5",
	"1.2.840.10008.5.1.4.1.1.481.6",
	"1.2.840.10008.5.1.4.1.1.481.7",
	"1.2.840.10008.5.1.4.1.1.481.8",
	"1.2.840.10008.5.1.4.1.1.481.9",
	"1.2.840.10008.5.1.4.34.7",
	"1.2.840.10008.5.1.4.43.1",
	"1.2.840.10008.5.1.4.44.1",
	"1.2.840.10008.5.1.4.45.1",
	// Verification SOP Class (C-ECHO)
	"1.2.840.10008.1.1",
];
