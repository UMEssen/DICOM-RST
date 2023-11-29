use crate::config::{Aet, DicomConfig, PacsConfig};
use crate::dimse::cecho::{CEchoRq, CEchoRsp};
use crate::dimse::{
    prepare_pdu_data, read_pdu_data, DicomError, FromDicomObject, IntoDicomObject, StatusType,
};
use async_trait::async_trait;
use deadpool::managed::{
    BuildError, Metrics, Object, Pool, QueueMode, RecycleError, RecycleResult,
};
use dicom::dictionary_std::uids::STUDY_ROOT_QUERY_RETRIEVE_INFORMATION_MODEL_FIND;
use dicom::ul::ClientAssociationOptions;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, info, warn};

mod connection;
pub use self::connection::DicomConnection;

#[derive(Debug, Clone)]
pub struct DicomPools(HashMap<Aet, Pool<DicomConnectionPool>>);

impl DicomPools {
    /// Creates a new [`DicomPool`] that contains a managed connection pool for each configured PACS.
    /// # Errors
    /// Returns a [`BuildError`] if it was not possible to create the pool.
    pub fn new(dicom_config: &DicomConfig) -> Result<Self, BuildError> {
        let mut pools = HashMap::with_capacity(dicom_config.pacs.len());

        for (aet, pacs_config) in &dicom_config.pacs {
            let mgr = DicomConnectionPool {
                aet: aet.clone(),
                pacs: pacs_config.clone(),
                calling_ae_title: dicom_config.aet.clone(),
            };

            let timeouts = deadpool::managed::Timeouts {
                wait: None,
                create: Some(Duration::from_secs(pacs_config.connect_timeout_seconds)),
                recycle: Some(Duration::from_secs(pacs_config.connect_timeout_seconds)),
            };

            let pool = Pool::builder(mgr)
                .max_size(pacs_config.max_pool_size)
                .queue_mode(QueueMode::Lifo)
                .runtime(deadpool::Runtime::Tokio1)
                .timeouts(timeouts)
                .build()?;

            pools.insert(aet.clone(), pool);
        }

        Ok(Self(pools))
    }

    #[inline]
    #[must_use]
    pub fn get(
        &self,
        aet: &str,
    ) -> Option<&Pool<DicomConnectionPool, Object<DicomConnectionPool>>> {
        self.0.get(aet)
    }

    #[inline]
    #[must_use]
    pub fn aets(&self) -> impl Iterator<Item = &Aet> {
        self.0.keys()
    }
}

#[derive(Debug, Clone)]
pub struct DicomConnectionPool {
    aet: Aet,
    pacs: PacsConfig,
    calling_ae_title: String,
}

#[async_trait]
impl deadpool::managed::Manager for DicomConnectionPool {
    type Type = DicomConnection;
    type Error = connection::Error;

    async fn create(&self) -> Result<Self::Type, Self::Error> {
        info!(
            "Establishing new client association for {} ({})",
            self.aet, self.pacs.address
        );

        // let options = ClientAssociationOptions::new()
        //     .with_abstract_syntax(STUDY_ROOT_QUERY_RETRIEVE_INFORMATION_MODEL_FIND)
        //     .calling_ae_title(self.calling_ae_title.clone())
        //     .called_ae_title(self.aet.clone());

        // let address = self.pacs.address.clone();

        // tokio::task::spawn_blocking(move || Ok(options.establish_with(&address)?))
        //     .await
        //     .expect("tokio::task::spawn_blocking")

        DicomConnection::new(
            &self.pacs.address,
            &self.aet,
            &self.calling_ae_title,
            &STUDY_ROOT_QUERY_RETRIEVE_INFORMATION_MODEL_FIND,
        )
        .await
    }

    async fn recycle(
        &self,
        client: &mut Self::Type,
        metrics: &Metrics,
    ) -> RecycleResult<Self::Error> {
        info!(
            "Recycling client association for {} ({}, age={:?} last_used={:?})",
            self.aet,
            self.pacs.address,
            metrics.age(),
            metrics.last_used()
        );

        // if  rand::random::<bool>() {
        //     warn!("Intentially returning Err from recycle()");
        //     return Err(RecycleError::Message(format!("Random")));
        // }

        let c_echo_rq = CEchoRq::default()
            .into_dicom_object()
            .map_err(|err| RecycleError::Message(format!("Failed to create C-ECHO-RQ: {err}")))?;

        let pctx = client
            .presentation_contexts()
            .first()
            .ok_or(RecycleError::StaticMessage(
                "Failed to get presentation context",
            ))?;

        client
            .send(prepare_pdu_data(&c_echo_rq, pctx.id))
            .await
            .map_err(|err| RecycleError::Message(format!("Failed to send C-ECHO-RQ: {err}")))?;

        let response = client
            .receive()
            .await
            .map_err(|err| RecycleError::Message(format!("Failed to receive C-ECHO-RSP: {err}")))?;

        // let response = tokio::task::block_in_place(|| {
        //     let pctx =
        //         client
        //             .presentation_contexts()
        //             .first()
        //             .ok_or(RecycleError::StaticMessage(
        //                 "Failed to get presentation context",
        //             ))?;

        //     client
        //         .send(&prepare_pdu_data(&c_echo_rq, pctx.id))
        //         .map_err(|err| RecycleError::Message(format!("Failed to send C-ECHO-RQ: {err}")))?;
        //     let response = client.receive().map_err(|err| {
        //         RecycleError::Message(format!("Failed to receive C-ECHO-RSP: {err}"))
        //     })?;

        //     Result::<_, RecycleError<Self::Error>>::Ok(response)
        // })?;

        let response_object = read_pdu_data(&response).map_err(connection::Error::Dicom)?;

        let c_echo_rsp =
            CEchoRsp::from_dicom_object(&response_object).map_err(connection::Error::Dicom)?;

        debug!("C-ECHO-RQ returned {:?}", c_echo_rsp.status_type);

        match c_echo_rsp.status_type {
            StatusType::Success => Ok(()),
            status => {
                warn!(
                    "Failed to recycle client association for {} ({}). C-ECHO-RQ returned {:?}",
                    self.aet, self.pacs.address, status
                );
                Err(RecycleError::Message(
                    format!("C-ECHO returned {status:?}",),
                ))
            }
        }
    }
}
