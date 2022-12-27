use actix_web::{web, Error, HttpResponse};
use serde::Deserialize;

use super::{helpers::filename_path, helpers::thumbnail_filename_path, Opt};

#[derive(Deserialize)]
pub struct DeleteRequest {
    pub filename: String,
}

pub async fn handle_delete(
    form: web::Form<DeleteRequest>,
    opt: web::Data<Opt>,
) -> Result<HttpResponse, Error> {
    if !sanitize_filename::is_sanitized(&form.filename) {
        return Ok(HttpResponse::BadRequest().into());
    }

    // We should delete both file and thumbnail.
    std::fs::remove_file(filename_path(&form.filename, &opt)?)?;
    std::fs::remove_file(thumbnail_filename_path(&form.filename, &opt)?)?;

    let response = HttpResponse::SeeOther()
        .append_header(("Location", "recent"))
        .body("deleted");

    Ok(response)
}
