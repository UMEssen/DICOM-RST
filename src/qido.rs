use std::collections::HashMap;

use axum::{
    extract::{Path, Query},
    response::IntoResponse,
};
use serde::Deserialize;

pub fn routes() -> axum::Router {
    use axum::routing::*;

    Router::new()
        // https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.6.html#sect_10.6.1.1
        .route("/studies", get(studies))
        .route("/studies/:study/series", get(study_series))
        .route("/studies/:study/instances", get(study_instances))
        .route("/series", get(series))
        .route(
            "/studies/:study/series/:series/instances",
            get(study_series_instances),
        )
        .route("/instances", get(instances))
}

pub async fn studies(
    params: Option<Query<Params>>,
    study_params: Query<PatientQueryParams>,
) -> impl IntoResponse {
    let Query(params) = params.unwrap_or_default();

    unimplemented!();
}

pub async fn study_series(
    Path(study): Path<String>,
    params: Option<Query<Params>>,
    series_params: Query<SeriesQueryParams>,
) -> impl IntoResponse {
    let Query(params) = params.unwrap_or_default();

    unimplemented!();
}

pub async fn study_instances(
    Path(study): Path<String>,
    params: Option<Query<Params>>,
    series_params: Query<SeriesQueryParams>,
    instance_params: Query<InstanceQueryParams>,
) -> impl IntoResponse {
    let Query(params) = params.unwrap_or_default();

    unimplemented!();
}

pub async fn series(
    params: Option<Query<Params>>,
    study_params: Query<PatientQueryParams>,
    series_params: Query<SeriesQueryParams>,
) -> impl IntoResponse {
    let Query(params) = params.unwrap_or_default();

    unimplemented!();
}

pub async fn instances(
    params: Option<Query<Params>>,
    study_params: Query<PatientQueryParams>,
    series_params: Query<SeriesQueryParams>,
    instance_params: Query<InstanceQueryParams>,
) -> impl IntoResponse {
    let Query(params) = params.unwrap_or_default();

    unimplemented!();
}

pub async fn study_series_instances(
    Path((study, series)): Path<(String, String)>,
    params: Option<Query<Params>>,
    instance_params: Query<InstanceQueryParams>,
) -> impl IntoResponse {
    let Query(params) = params.unwrap_or_default();

    unimplemented!();
}

use dicom::dictionary_std::tags;
use dicom::{core::DataDictionary, object::StandardDataDictionary};
use tracing::error;

#[derive(Debug, Deserialize)]
pub struct Params {
    // pub r#match: String, // TODO https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_8.3.4.html#sect_8.3.4.1
    pub fuzzymatching: bool,
    pub includefield: IncludeFields,
    pub limit: Option<u64>,
    pub offset: u64,
    pub emptyvaluematching: bool,
    pub multiplevaluematching: bool,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            // r#match: Default::default(),
            fuzzymatching: false,
            includefield: IncludeFields::All,
            limit: None,
            offset: 0,
            emptyvaluematching: false,
            multiplevaluematching: false,
        }
    }
}

#[derive(Debug, Deserialize)]
pub enum IncludeFields {
    All,
    List(Vec<String>),
}

const STUDY_SEARCH_ATTRIBUTES: &'static [dicom::core::Tag] = &[
    tags::STUDY_DATE,
    tags::STUDY_TIME,
    tags::ACCESSION_NUMBER,
    tags::MODALITIES_IN_STUDY,
    tags::REFERRING_PHYSICIAN_NAME,
    tags::PATIENT_NAME,
    tags::PATIENT_ID,
    tags::STUDY_INSTANCE_UID,
    tags::STUDY_ID,
];

#[derive(Debug, Deserialize)]
#[serde(try_from = "HashMap<String, String>")]
pub struct PatientQueryParams(Vec<(dicom::core::Tag, String)>);

impl TryFrom<HashMap<String, String>> for PatientQueryParams {
    type Error = &'static str;

    fn try_from(value: HashMap<String, String>) -> Result<Self, Self::Error> {
        let pairs = value
            .into_iter()
            .filter_map(|(key, value)| {
                StandardDataDictionary
                    .by_name(&key)
                    .filter(|e| STUDY_SEARCH_ATTRIBUTES.contains(&e.tag.inner()))
                    .map(|entry| (entry.tag.inner(), value))
            })
            .collect();

        Ok(PatientQueryParams(pairs))
    }
}

const SERIES_SEARCH_ATTRIBUTES: &'static [dicom::core::Tag] = &[
    tags::MODALITY,
    tags::SERIES_INSTANCE_UID,
    tags::SERIES_NUMBER,
    tags::PERFORMED_PROCEDURE_STEP_START_DATE,
    tags::PERFORMED_PROCEDURE_STEP_START_TIME,
    tags::REQUEST_ATTRIBUTES_SEQUENCE,
];

#[derive(Debug, Deserialize)]
#[serde(try_from = "HashMap<String, String>")]
pub struct SeriesQueryParams(Vec<(dicom::core::Tag, String)>);

impl TryFrom<HashMap<String, String>> for SeriesQueryParams {
    type Error = &'static str;

    fn try_from(value: HashMap<String, String>) -> Result<Self, Self::Error> {
        let pairs = value
            .into_iter()
            .filter_map(|(key, value)| {
                StandardDataDictionary
                    .by_name(&key)
                    .filter(|e| SERIES_SEARCH_ATTRIBUTES.contains(&e.tag.inner()))
                    .map(|entry| (entry.tag.inner(), value))
            })
            .collect();

        Ok(SeriesQueryParams(pairs))
    }
}

const INSTANCE_SEARCH_ATTRIBUTES: &'static [dicom::core::Tag] = &[
    tags::SOP_CLASS_UID,
    tags::SOP_INSTANCE_UID,
    tags::INSTANCE_NUMBER,
];

#[derive(Debug, Deserialize)]
#[serde(try_from = "HashMap<String, String>")]
pub struct InstanceQueryParams(Vec<(dicom::core::Tag, String)>);

impl TryFrom<HashMap<String, String>> for InstanceQueryParams {
    type Error = &'static str;

    fn try_from(value: HashMap<String, String>) -> Result<Self, Self::Error> {
        let pairs = value
            .into_iter()
            .filter_map(|(key, value)| {
                StandardDataDictionary
                    .by_name(&key)
                    .filter(|e| INSTANCE_SEARCH_ATTRIBUTES.contains(&e.tag.inner()))
                    .map(|entry| (entry.tag.inner(), value))
            })
            .collect();

        Ok(InstanceQueryParams(pairs))
    }
}

// #[cfg(test)]
// mod tests {
//     use axum::http::uri::PathAndQuery;
//     use hyper::Request;

//     #[tokio::test]
//     async fn routes() {
//         let cases = &[
//             "/studies?PatientID=11235813",
//             "/studies?PatientID=11235813&StudyDate=20130509",
//             "/studies?00100010=SMITH*&00101002.00100020=11235813&limit=25",
//             "/studies?00100010=SMITH*&OtherPatientIDsSequence.00100020=11235813",
//             "/studies?PatientID=11235813&includefield=00081048,00081049,00081060",
//             "/studies?PatientID=11235813&includefield=00081048&includefield=00081049&includefield=00081060",
//             "/studies?PatientID=11235813&StudyDate=20130509-20130510",
//             "/studies?StudyInstanceUID=1.2.392.200036.9116.2.2.2.2162893313.1029997326.94587,1.2.392.200036.9116.2.2.2.2162893313.1029997326.94583",
//             "/studies?00230010=AcmeCompany&includefield=00231002&includefield=00231003",
//             "/studies?00230010=AcmeCompany&00231001=001239&includefield=00231002&includefield=00231003"
//         ];

//         let app = super::routes().into_service();
//         for case in cases {
//             let request = Request::builder()
//                 .uri(PathAndQuery::from_static(&case))
//                 .body(())
//                 .unwrap();

//             use axum::ServiceExt;

//             app.oneshot(request).await.unwrap();
//         }
//     }
// }
