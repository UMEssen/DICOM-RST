use dicom::ul::pdu::{PDataValueType, PresentationContextResult};
use dicom::ul::Pdu;
use std::convert::identity;
use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::thread;
use std::time::Duration;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tracing::{debug, error};
use uuid::Uuid;

use super::{AskPattern, Association, AssociationError, ChannelError, Command};

pub struct ClientAssociation {
	channel: Sender<Command>,
	uuid: Uuid,
	tcp_stream: TcpStream,
	presentation_context: Vec<PresentationContextResult>,
	acceptor_max_pdu_length: u32,
}

pub struct ClientAssociationOptions {
	pub calling_aet: String,
	pub called_aet: String,
	pub abstract_syntax: String,
	pub transfer_syntaxes: Vec<String>,
	pub address: SocketAddr,
}

impl ClientAssociation {
	fn chunked_send(
		association: &mut dicom::ul::ClientAssociation,
		pdu: &Pdu,
	) -> Result<(), AssociationError> {
		match &pdu {
			Pdu::PData { data } => {
				let is_command = data
					.first()
					.is_some_and(|pdv| pdv.value_type == PDataValueType::Command);
				if is_command {
					association.send(pdu).map_err(AssociationError::Client)
				} else {
					let data_length: usize = data.iter().map(|pdv| pdv.data.len()).sum();
					if data_length > association.acceptor_max_pdu_length() as usize {
						for pdv in data {
							assert_eq!(pdv.value_type, PDataValueType::Data);
							let mut writer = association.send_pdata(pdv.presentation_context_id);
							writer
								.write_all(&pdv.data)
								.map_err(AssociationError::ChunkWriter)?;
						}
						Ok(())
					} else {
						association.send(pdu).map_err(AssociationError::Client)
					}
				}
			}
			_ => association.send(pdu).map_err(AssociationError::Client),
		}
	}

	pub async fn new(options: ClientAssociationOptions) -> Result<Self, AssociationError> {
		let uuid = Uuid::new_v4();
		let (tx, mut rx) = tokio::sync::mpsc::channel::<Command>(1);
		let (connect_tx, connect_result) = oneshot::channel::<Result<_, AssociationError>>();

		let address = options.address;
		let options = dicom::ul::ClientAssociationOptions::new()
			.calling_ae_title(options.calling_aet)
			.called_ae_title(options.called_aet)
			.with_presentation_context(
				options.abstract_syntax,
				Vec::from(options.transfer_syntaxes),
			);

		let _handle = thread::Builder::new()
			.name(String::from("calling_aet"))
			.spawn(move || {
				let mut association = match options.establish(address) {
					Ok(mut association) => {
						let presentation_contexts = Vec::from(association.presentation_contexts());
						let acceptor_max_pdu_length = association.acceptor_max_pdu_length();

						let stream = association
							.inner_stream()
							.try_clone()
							.expect("TcpStream should be cloneable");

						connect_tx
							.send(Ok((stream, presentation_contexts, acceptor_max_pdu_length)))
							.map_err(|_| ())?;

						association
					}
					Err(e) => {
						error!(backend_uuid = uuid.to_string(), "Failed to connect: {e}");
						connect_tx.send(Err(e.into())).map_err(|_| ())?;
						return Err(());
					}
				};

				while let Some(command) = rx.blocking_recv() {
					let result = match command {
						Command::Send(pdu, reply_to) => {
							let send_result = Self::chunked_send(&mut association, &pdu);
							reply_to.send(send_result).map_err(|_| ChannelError::Closed)
						}
						Command::Receive(reply_to) => {
							let receive_result =
								association.receive().map_err(AssociationError::Client);
							reply_to
								.send(receive_result)
								.map_err(|_| ChannelError::Closed)
						}
					};
					if let Some(err) = result.err() {
						error!(
							backend_uuid = uuid.to_string(),
							"Error in ClientAssociation backend: {err}"
						);
						return Err(());
					}
				}

				rx.close();

				if let Err(err) = association.abort() {
					debug!(
						backend_uuid = uuid.to_string(),
						"Failed to abort ClientAssociation: {err}"
					);
				}

				Ok(())
			})
			.map_err(AssociationError::OsThread)?;

		let (tcp_stream, presentation_context, acceptor_max_pdu_length) =
			connect_result.await.expect("connect_result.await")?;

		Ok(Self {
			channel: tx,
			uuid,
			tcp_stream,
			presentation_context,
			acceptor_max_pdu_length,
		})
	}

	pub fn uuid(&self) -> &Uuid {
		&self.uuid
	}
}

impl Drop for ClientAssociation {
	fn drop(&mut self) {
		self.close();
	}
}

impl Association for ClientAssociation {
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
		if let Err(err) = self.tcp_stream.shutdown(std::net::Shutdown::Both) {
			debug!(
				backend_uuid = self.uuid.to_string(),
				"Failed to shutdown TcpStream: {err}"
			);
		}
	}

	fn presentation_contexts(&self) -> &[PresentationContextResult] {
		&self.presentation_context
	}
}
