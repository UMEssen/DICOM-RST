use std::{net::TcpStream, thread, time::Duration};

use dicom::ul::{pdu::PresentationContextResult, ClientAssociationOptions, Pdu};
use tracing::{debug, error, warn};

use crate::dimse::{self, DicomError};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Dicom(#[from] dimse::DicomError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("Backend closed")]
    ChannelClosed,
    #[error("Timeout")]
    Timeout,
}

#[derive(Debug)]
enum Command {
    Send(Pdu, tokio::sync::oneshot::Sender<Result<(), DicomError>>),
    Receive(tokio::sync::oneshot::Sender<Result<Pdu, DicomError>>),
}

#[derive(Debug)]
pub struct DicomConnection {
    backend_uuid: uuid::Uuid,
    channel: tokio::sync::mpsc::Sender<Command>,
    presentation_contexts: Vec<PresentationContextResult>,
    tcp_stream: TcpStream,
}

impl DicomConnection {
    pub async fn new(
        uuid: uuid::Uuid,
        address: &str,
        aet: &str,
        calling_ae_title: &str,
        abstract_syntax_uid: &str,
    ) -> Result<Self, Error> {
        let options = ClientAssociationOptions::new()
            .called_ae_title(aet.to_owned())
            .calling_ae_title(calling_ae_title.to_owned())
            .with_abstract_syntax(abstract_syntax_uid.to_owned());

        let address = address.to_owned();

        let (connect_tx, connect_result) = tokio::sync::oneshot::channel::<Result<_, DicomError>>();

        let (tx, mut rx) = tokio::sync::mpsc::channel::<Command>(1);
        let _handle = thread::Builder::new().name(aet.to_owned()).spawn(move || {
            let span = tracing::info_span!("backend", backend_uuid = uuid.to_string());
            let _enter = span.enter();

            let mut association = match options.establish_with(&address) {
                Ok(mut association) => {
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
                debug!("{command:?}");

                let result = match command {
                    Command::Send(pdu, response) => {
                        let send_result = association.send(&pdu).map_err(|e| e.into());
                        response
                            .send(send_result)
                            .map_err(|_value| Error::ChannelClosed)
                    }
                    Command::Receive(response) => {
                        // if rand::random::<bool>() {
                        //     warn!("Intentially blocking thread for debugging...");
                        //     std::thread::sleep(Duration::from_secs(6));
                        // }

                        response
                            .send(association.receive().map_err(|e| e.into()))
                            .map_err(|_value| Error::ChannelClosed)
                    }
                };

                if let Some(err) = result.err() {
                    error!("Error in DicomConnection backend: {err:?}");
                    return Err(());
                }
            }

            rx.close();
            association.abort().unwrap();

            Ok(())
        })?;

        let (tcp_stream, presentation_contexts) =
            connect_result.await.expect("connect_result.await")?;

        Ok(Self {
            backend_uuid: uuid,
            channel: tx,
            presentation_contexts,
            tcp_stream,
        })
    }
}

impl DicomConnection {
    pub fn uuid(&self) -> &uuid::Uuid {
        &self.backend_uuid
    }

    pub fn presentation_contexts(&self) -> &[PresentationContextResult] {
        &self.presentation_contexts
    }

    pub async fn send(&mut self, pdu: Pdu, timeout: Duration) -> Result<(), Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();

        let result = tokio::time::timeout(timeout, async {
            self.channel
                .send(Command::Send(pdu, tx))
                .await
                .map_err(|_| Error::ChannelClosed)?;

            rx.await
                .map_err(|_e| Error::ChannelClosed)?
                .map_err(Error::Dicom)
        })
        .await
        .map_err(|_| Error::Timeout)
        .and_then(|r| r);

        if let Err(ref err) = result {
            error!("Error in DicomConnection::send: {err:?}",);
        }

        result
    }

    pub async fn receive(&mut self, timeout: Duration) -> Result<Pdu, Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();

        let result = tokio::time::timeout(timeout, async {
            self.channel
                .send(Command::Receive(tx))
                .await
                .map_err(|_| Error::ChannelClosed)?;

            rx.await
                .map_err(|_e| Error::ChannelClosed)?
                .map_err(Error::Dicom)
        })
        .await
        .map_err(|_| Error::Timeout)
        .and_then(|r| r);

        if let Err(ref err) = result {
            error!("Error in DicomConnection::receive: {err}",);
        }

        result
    }

    pub fn close(&mut self) {
        debug!(
            backend_uuid = self.backend_uuid.to_string(),
            "Closing TcpStream from outside"
        );
        self.tcp_stream.shutdown(std::net::Shutdown::Both).unwrap();
    }
}

impl Drop for DicomConnection {
    fn drop(&mut self) {
        self.close()
    }
}
