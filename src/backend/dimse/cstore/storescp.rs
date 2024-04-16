use crate::backend::dimse::association;
use crate::backend::dimse::cmove::{MoveMediator, MoveSubOperation, SubscriptionTopic};
use crate::backend::dimse::cstore::{
	CompositeStoreResponse, COMMAND_FIELD_COMPOSITE_STORE_REQUEST,
};
use crate::backend::dimse::{DicomMessageReader, DicomMessageWriter};
use crate::config::DimseServerConfig;
use crate::types::{AE, UI, US};
use anyhow::Context;
use association::server::{ServerAssociation, ServerAssociationOptions};
use association::Association;
use dicom::dictionary_std::tags;
use dicom::object::mem::InMemElement;
use dicom::object::FileMetaTableBuilder;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info, info_span, instrument, Instrument};

pub struct StoreServiceClassProvider {
	inner: Arc<InnerStoreServiceClassProvider>,
}

struct InnerStoreServiceClassProvider {
	mediator: MoveMediator,
	config: DimseServerConfig,
}

impl StoreServiceClassProvider {
	pub fn new(mediator: MoveMediator, config: DimseServerConfig) -> Self {
		Self {
			inner: Arc::new(InnerStoreServiceClassProvider { mediator, config }),
		}
	}

	#[instrument(skip_all, name = " STORE-SCP")]
	pub async fn spawn(&self) -> anyhow::Result<()> {
		let address = SocketAddr::from((self.inner.config.host, self.inner.config.port));
		let listener = TcpListener::bind(&address).await?;
		info!("Started Store Service Class Provider on {}", address);
		loop {
			match listener.accept().await {
				Ok((stream, peer)) => {
					info!("Accepted incoming connection from {peer}");
					let inner = Arc::clone(&self.inner);
					tokio::spawn(Self::process(stream, inner));
				}
				Err(err) => error!("Failed to accept incoming connection: {err}"),
			};
		}
	}

	async fn process(
		stream: TcpStream,
		inner: Arc<InnerStoreServiceClassProvider>,
	) -> anyhow::Result<()> {
		let tcp_stream = stream.into_std()?;
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
		'read: while let Ok(message) = association.read_message(Duration::MAX).await {
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
				.and_then(Result::ok);

			let file = message.data.unwrap().with_exact_meta(
				FileMetaTableBuilder::new()
					.media_storage_sop_class_uid(sop_class_uid.as_ref())
					.media_storage_sop_instance_uid(sop_instance_uid.as_ref())
					.transfer_syntax(&pctx.transfer_syntax)
					.build()
					.expect("FileMetaTableBuilder should contain required data"),
			);

			let file = Arc::new(file);

			for aet in &inner.config.notify_aets {
				let topic = SubscriptionTopic::new(AE::from(aet), move_originator_id);
				if let Err(err) = inner
					.mediator
					.publish(&topic, Ok(MoveSubOperation::Pending(Arc::clone(&file))))
					.await
				{
					error!("Failed to publish sub-operation over subscription: {err}");
					break 'read; // stop receiving further messages from this peer
				}
			}
		}
		Ok(())
	}
}
