use std::{net::TcpStream, thread, time::Duration};

use dicom::{
    object::AtAccessError,
    ul::{
        address, association, pdu::PresentationContextResult, ClientAssociation,
        ClientAssociationOptions, Pdu,
    },
};
use tracing::debug;

use crate::dimse::{self, DicomError};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Dicom(#[from] dimse::DicomError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("Backend closed")]
    Closed,
}

enum Command {
    Send(Pdu, tokio::sync::oneshot::Sender<Result<(), DicomError>>),
    Receive(tokio::sync::oneshot::Sender<Result<Pdu, DicomError>>),
}

#[derive(Debug)]
pub struct DicomConnection {
    handle: thread::JoinHandle<Result<(), ()>>,
    channel: tokio::sync::mpsc::Sender<Command>,
    presentation_contexts: Vec<PresentationContextResult>,
    tcp_stream: TcpStream,
}

impl DicomConnection {
    pub async fn new(
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

        let handle = thread::Builder::new().name(aet.to_owned()).spawn(move || {
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
                match command {
                    Command::Send(pdu, response) => {
                        response
                            .send(association.send(&pdu).map_err(|e| e.into()))
                            .map_err(|_value| ())?;
                    }
                    Command::Receive(response) => {
                        response
                            .send(association.receive().map_err(|e| e.into()))
                            .map_err(|_value| ())?;
                    }
                }
            }

            rx.close();
            association.abort().unwrap();

            Ok(())
        })?;

        let (tcp_stream, presentation_contexts) =
            connect_result.await.expect("connect_result.await")?;

        Ok(Self {
            handle,
            channel: tx,
            presentation_contexts,
            tcp_stream,
        })
    }
}

impl DicomConnection {
    pub fn presentation_contexts(&self) -> &[PresentationContextResult] {
        &self.presentation_contexts
    }

    pub async fn send(&mut self, pdu: Pdu) -> Result<(), Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.channel
            .send(Command::Send(pdu, tx))
            .await
            .map_err(|_e| Error::Closed)?;

        rx.await.map_err(|_e| Error::Closed)?.map_err(Error::Dicom)
    }

    pub async fn receive(&mut self) -> Result<Pdu, Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.channel
            .send(Command::Receive(tx))
            .await
            .map_err(|_e| Error::Closed)?;

        rx.await.map_err(|_e| Error::Closed)?.map_err(Error::Dicom)
    }

    pub fn close(self) {
        self.tcp_stream.shutdown(std::net::Shutdown::Both).unwrap();
        let _thread_result = self.handle.join().unwrap();
    }
}

// trait State {}

// #[derive(Debug)]
// struct Disconnected;
// struct Connected(ClientAssociation);

// #[derive(Debug)]
// struct BlockingDicomConnection<State> {
//     state: State,
// }

// impl BlockingDicomConnection<Disconnected> {
//     pub fn connect(
//         addr: &str,
//         options: ClientAssociationOptions<'_>,
//     ) -> Result<BlockingDicomConnection<Connected>, DicomError> {
//         todo!()
//     }
// }

// impl BlockingDicomConnection<Connected> {
//     pub fn send(&mut self, pdu: Pdu) -> Result<(), DicomError> {
//         todo!()
//     }
// }
