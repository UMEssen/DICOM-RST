use crate::backend::dimse::association;
use crate::backend::dimse::cmove::MoveSubOperation;
use crate::backend::dimse::cmove::{MoveMediator, TaskIdentifier};
use crate::backend::dimse::cstore::{
	CompositeStoreResponse, COMMAND_FIELD_COMPOSITE_STORE_REQUEST,
};
use crate::backend::dimse::{DicomMessageReader, DicomMessageWriter};
use crate::config::DimseServerConfig;
use crate::types::{UI, US};
use anyhow::Context;
use association::server::{ServerAssociation, ServerAssociationOptions};
use association::Association;
use dicom::dictionary_std::tags;
use dicom::object::mem::InMemElement;
use dicom::object::FileMetaTableBuilder;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tracing::{debug, error, info, instrument};

pub struct StoreServiceClassProvider {
	mediator: Arc<RwLock<MoveMediator>>,
	config: DimseServerConfig,
}

impl StoreServiceClassProvider {
	pub fn new(mediator: Arc<RwLock<MoveMediator>>, config: DimseServerConfig) -> Self {
		Self { mediator, config }
	}

	#[instrument(skip_all, name = " STORE-SCP")]
	pub async fn spawn(&self) -> anyhow::Result<()> {
		let address = SocketAddr::from((self.config.host, self.config.port));
		let listener = TcpListener::bind(&address).await?;
		info!("Started Store Service Class Provider on {}", address);
		loop {
			if let Err(err) = self.accept(&listener).await {
				error!("Error occurred while processing request: {}", err);
			}
		}
	}

	async fn accept(&self, listener: &TcpListener) -> anyhow::Result<()> {
		let (tcp_stream, peer) = listener.accept().await?;
		info!("Accepted new connection from {peer}");
		let tcp_stream = tcp_stream.into_std()?;
		// This is required because the `dicom-rs` crate does not use non-blocking reads/writes.
		// The actual reading/writing happens in ServerAssociation, which moves IO operation
		// to another thread.
		tcp_stream.set_nonblocking(false)?;

		let options = ServerAssociationOptions {
			aet: String::from("DICOM-RST"),
			tcp_stream,
		};
		let association = ServerAssociation::new(options).await?;

		// Duration::MAX to indefinitely wait for incoming messages
		while let Ok(message) = association.read_message(Duration::MAX).await {
			let pctx = association
				.presentation_contexts()
				.first()
				.context("No presentation context available")?;

			let command_field = message
				.command
				.get(tags::COMMAND_FIELD)
				.map(InMemElement::to_int::<US>)
				.and_then(Result::ok)
				.context("Missing tag COMMAND_FIELD (0000,0100)")?;

			if command_field != COMMAND_FIELD_COMPOSITE_STORE_REQUEST {
				return Err(anyhow::Error::msg(
					"Unexpected Command Field. Only C-STORE-RQ is supported.",
				));
			}

			let message_id = message
				.command
				.get(tags::MESSAGE_ID)
				.map(InMemElement::to_int)
				.and_then(Result::ok)
				.unwrap_or(0);

			let sop_class_uid = message
				.command
				.get(tags::AFFECTED_SOP_CLASS_UID)
				.map(InMemElement::to_str)
				.and_then(Result::ok)
				.context("Missing tag AFFECTED_SOP_CLASS_UID (0000,0002)")?;

			let sop_instance_uid = message
				.command
				.get(tags::AFFECTED_SOP_INSTANCE_UID)
				.map(InMemElement::to_str)
				.and_then(Result::ok)
				.context("Missing tag AFFECTED_SOP_INSTANCE_UID (0000,1000)")?;

			debug!("Received instance {} ({})", sop_instance_uid, sop_class_uid);
			let response = CompositeStoreResponse {
				sop_instance_uid: UI::from(sop_instance_uid.clone()),
				sop_class_uid: UI::from(sop_class_uid.clone()),
				message_id,
			};

			association
				.write_message(response, Duration::from_secs(10))
				.await?;

			let move_originator_id = message
				.command
				.get(tags::MOVE_ORIGINATOR_MESSAGE_ID)
				.map(InMemElement::to_int::<US>)
				.and_then(Result::ok)
				.unwrap();

			let file = message.data.unwrap().with_exact_meta(
				FileMetaTableBuilder::new()
					.media_storage_sop_class_uid(sop_class_uid.as_ref())
					.media_storage_sop_instance_uid(sop_instance_uid.as_ref())
					.transfer_syntax(&pctx.transfer_syntax)
					.build()
					.expect("FileMetaTableBuilder should contain required data"),
			);

			let file = Arc::new(file);
			let mediator = self.mediator.read().await;

			for aet in &self.config.notify_aets {
				if let Some(callback) =
					mediator.get(&TaskIdentifier::new(String::from(aet), move_originator_id))
				{
					if let Err(err) = callback
						.send(Ok(MoveSubOperation::Pending(Arc::clone(&file))))
						.await
					{
						error!("Failed to send via callback: {err}");
					}
				}
			}
		}
		Ok(())
	}
}
