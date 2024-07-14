use axum::{
    extract::State,
    http::{header::LOCATION, StatusCode},
    response::IntoResponse,
    Form,
};
use serde::Deserialize;

use crate::WebError;

use super::{helpers::filename_path, helpers::thumbnail_filename_path, Opt};

#[derive(Deserialize)]
pub struct DeleteRequest {
    pub filename: String,
}

pub async fn handle_delete(
    State(opt): State<Opt>,
    Form(form): Form<DeleteRequest>,
) -> Result<impl IntoResponse, WebError> {
    if !sanitize_filename::is_sanitized(&form.filename) {
        return Err(WebError::BadRequest);
    }

    // We should delete both file and thumbnail.
    std::fs::remove_file(filename_path(&form.filename, &opt)?)?;
    std::fs::remove_file(thumbnail_filename_path(&form.filename, &opt)?).ok();

    Ok((StatusCode::SEE_OTHER, [(LOCATION, "recent")], "deleted"))
}
