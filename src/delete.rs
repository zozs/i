use axum::{
    Form,
    extract::State,
    http::{StatusCode, header::LOCATION},
    response::IntoResponse,
};
use serde::Deserialize;

use crate::WebError;

use super::{Opt, helpers::filename_path, helpers::thumbnail_filename_path};

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
