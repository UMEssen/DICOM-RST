use crate::config::{application_config, Aet, PacsConfig};
use crate::dimse::cecho::{CEchoRq, CEchoRsp};
use crate::dimse::{
    prepare_pdu_data, read_pdu_data, DicomError, FromDicomObject, IntoDicomObject, StatusType,
};
use async_trait::async_trait;
use deadpool::managed::{
    BuildError, Metrics, Object, Pool, QueueMode, RecycleError, RecycleResult,
};
use dicom::dictionary_std::uids::STUDY_ROOT_QUERY_RETRIEVE_INFORMATION_MODEL_FIND;
use dicom::ul::{ClientAssociation, ClientAssociationOptions};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub struct DicomPool(HashMap<Aet, Pool<DicomManager>>);

impl DicomPool {
    /// Creates a new [`DicomPool`] that contains a managed connection pool for each configured PACS.
    /// # Errors
    /// Returns a [`BuildError`] if it was not possible to create the pool.
    pub fn new() -> Result<Self, BuildError> {
        let configured_pacs = &application_config().dicom.pacs;
        let mut pools = HashMap::with_capacity(configured_pacs.len());

        for (aet, config) in configured_pacs {
            let mgr = DicomManager {
                aet: aet.clone(),
                config: config.clone(),
            };
            let pool = Pool::builder(mgr)
                .max_size(config.max_pool_size)
                .queue_mode(QueueMode::Fifo)
                .build()?;
            pools.insert(aet.clone(), pool);
        }

        Ok(Self(pools))
    }

    #[inline]
    #[must_use]
    pub fn get(&self, aet: &str) -> Option<&Pool<DicomManager, Object<DicomManager>>> {
        self.0.get(aet)
    }

    #[inline]
    #[must_use]
    pub fn available(&self) -> Vec<&Aet> {
        self.0.keys().collect()
    }
}

impl Display for DicomPool {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut string_builder = String::new();
        for (aet, obj) in &self.0 {
            string_builder.push_str(&format!(
                "aet={}, max_pool_size={}",
                aet,
                obj.manager().config.max_pool_size
            ));
        }
        write!(f, "{string_builder}")
    }
}

#[derive(Debug, Clone)]
pub struct DicomManager {
    pub aet: Aet,
    pub config: PacsConfig,
}

#[async_trait]
impl deadpool::managed::Manager for DicomManager {
    type Type = ClientAssociation;
    type Error = DicomError;

    async fn create(&self) -> Result<Self::Type, Self::Error> {
        info!(
            "Establishing new client association for {} ({})",
            self.aet, self.config.address
        );
        let config = application_config();
        let options = ClientAssociationOptions::new()
            .with_abstract_syntax(STUDY_ROOT_QUERY_RETRIEVE_INFORMATION_MODEL_FIND)
            .calling_ae_title(&config.dicom.aet);
        let association = options.establish_with(self.config.address.as_str())?;
        Ok(association)
    }

    async fn recycle(
        &self,
        client: &mut Self::Type,
        _metrics: &Metrics,
    ) -> RecycleResult<Self::Error> {
        info!(
            "Recycling client association for {} ({})",
            self.aet, self.config.address
        );
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
            .send(&prepare_pdu_data(&c_echo_rq, pctx.id))
            .map_err(|err| RecycleError::Message(format!("Failed to send C-ECHO-RQ: {err}")))?;
        let response = client
            .receive()
            .map_err(|err| RecycleError::Message(format!("Failed to receive C-ECHO-RSP: {err}")))?;

        let response_object = read_pdu_data(&response)?;
        let c_echo_rsp = CEchoRsp::from_dicom_object(&response_object)?;

        debug!("C-ECHO-RQ returned {:?}", c_echo_rsp.status_type);
        match c_echo_rsp.status_type {
            StatusType::Success => Ok(()),
            status => {
                warn!(
                    "Failed to recycle client association for {} ({}). C-ECHO-RQ returned {:?}",
                    self.aet, self.config.address, status
                );
                Err(RecycleError::Message(
                    format!("C-ECHO returned {status:?}",),
                ))
            }
        }
    }
}