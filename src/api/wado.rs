use crate::AppState;
use axum::{extract::Path, response::IntoResponse, routing::*, Router};

pub fn routes() -> Router<AppState> {
    Router::new()
        // https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html#sect_10.4.1.1.1
        .route("/pacs/:aet/studies/:study", get(study))
        .route(
            "/pacs/:aet/studies/:study/series/:series",
            get(study_series),
        )
        .route(
            "/pacs/:aet/studies/:study/series/:series/instances/:instance",
            get(study_series_instance),
        )
        // https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html#sect_10.4.1.1.2
        .route("/pacs/:aet/studies/:study/metadata", get(study_metadata))
        .route(
            "/pacs/:aet/studies/:study/series/:series/metadata",
            get(study_series_metadata),
        )
        .route(
            "/pacs/:aet/studies/:study/series/:series/instances/:instance/metadata",
            get(study_series_instance_metadata),
        )
        // https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html#sect_10.4.1.1.3
        .route("/pacs/:aet/studies/:study/rendered", get(study_rendered))
        .route(
            "/pacs/:aet/studies/:study/series/:series/rendered",
            get(study_series_rendered),
        )
        .route(
            "/pacs/:aet/studies/:study/series/:series/instances/:instance/rendered",
            get(study_series_instance_rendered),
        )
        .route(
            "/pacs/:aet/studies/:study/series/:series/instances/:instance/frames/:frames/rendered",
            get(study_series_instance_frames_rendered),
        )
    // TODO: Thumbnail https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html#sect_10.4.1.1.4
    // TODO: Bulkdata https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html#sect_10.4.1.1.5
    // TODO: Pixeldata https://dicom.nema.org/medical/dicom/current/output/chtml/part18/sect_10.4.html#sect_10.4.1.1.6
}

pub async fn study(Path((aet, study)): Path<(String, String)>) -> impl IntoResponse {
    unimplemented!()
}

pub async fn study_series(
    Path((aet, study, series)): Path<(String, String, String)>,
) -> impl IntoResponse {
    unimplemented!()
}

pub async fn study_series_instance(
    Path((aet, study, series, instance)): Path<(String, String, String, String)>,
) -> impl IntoResponse {
    unimplemented!()
}

pub async fn study_metadata(Path((aet, study)): Path<(String, String)>) -> impl IntoResponse {
    unimplemented!()
}

pub async fn study_series_metadata(
    Path((aet, study, series)): Path<(String, String, String)>,
) -> impl IntoResponse {
    unimplemented!()
}

pub async fn study_series_instance_metadata(
    Path((aet, study, series, instance)): Path<(String, String, String, String)>,
) -> impl IntoResponse {
    unimplemented!()
}

pub async fn study_rendered(Path((aet, study)): Path<(String, String)>) -> impl IntoResponse {
    unimplemented!()
}

pub async fn study_series_rendered(
    Path((aet, study, series)): Path<(String, String, String)>,
) -> impl IntoResponse {
    unimplemented!()
}

pub async fn study_series_instance_rendered(
    Path((aet, study, series, instance)): Path<(String, String, String, String)>,
) -> impl IntoResponse {
    unimplemented!()
}

pub async fn study_series_instance_frames_rendered(
    Path((aet, study, series, instance, frames)): Path<(String, String, String, String, String)>,
) -> impl IntoResponse {
    unimplemented!()
}
