use dicom::ul::pdu::PresentationContextResult;
use dicom::ul::Pdu;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tracing::error;

pub mod client;
pub mod pool;
pub mod server;

#[derive(Debug, Error)]
pub enum AssociationError {
	#[error(transparent)]
	Channel(#[from] ChannelError),
	#[error("Failed to spawn thread")]
	OsThread(std::io::Error),
	#[error("Failed to write P-DATA chunk: {0}")]
	ChunkWriter(std::io::Error),
	#[error(transparent)]
	Client(#[from] dicom::ul::association::client::Error),
	#[error(transparent)]
	Server(#[from] dicom::ul::association::server::Error),
}

pub trait Association {
	async fn receive(&self, timeout: Duration) -> Result<Pdu, AssociationError>;

	async fn send(&self, pdu: Pdu, timeout: Duration) -> Result<(), AssociationError>;

	fn close(&mut self);

	fn presentation_contexts(&self) -> &[PresentationContextResult];
}

#[derive(Debug)]
pub enum Command {
	Send(Pdu, oneshot::Sender<Result<(), AssociationError>>),
	Receive(oneshot::Sender<Result<Pdu, AssociationError>>),
}

#[derive(Debug, Error)]
pub enum ChannelError {
	#[error("Timed out")]
	Timeout,
	#[error("Channel is closed")]
	Closed,
}

pub trait AskPattern<T> {
	async fn ask<R>(
		&self,
		command: impl FnOnce(oneshot::Sender<R>) -> T,
		timeout: Duration,
	) -> Result<R, ChannelError>;
}

impl<T> AskPattern<T> for Sender<T> {
	async fn ask<R>(
		&self,
		command: impl FnOnce(oneshot::Sender<R>) -> T,
		timeout: Duration,
	) -> Result<R, ChannelError> {
		let (tx, rx) = oneshot::channel();
		tokio::time::timeout(timeout, async {
			self.send(command(tx))
				.await
				.map_err(|_| ChannelError::Closed)?;

			rx.await.map_err(|_| ChannelError::Closed)
		})
		.await
		.map_err(|_| ChannelError::Timeout)?
	}
}
