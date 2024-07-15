use askama_axum::IntoResponse;
use axum::extract::multipart::Field;
use axum::extract::{Multipart, State};
use axum::http::header::LOCATION;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use futures::StreamExt;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::WebError;

use super::helpers::{filename_path, thumbnail_filename_path};
use super::{thumbnail::generate_thumbnail, Opt};

struct FileUpload {
    original_filename: String,
    random_filename: String,
    random_filename_path: PathBuf,
}

fn default_as_true() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Options {
    #[serde(default)]
    use_original_filename: bool, // default for bool is false.
    #[serde(default = "default_as_true")] // semi-ugly hack to get true as default.
    redirect: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadResponse {
    url: String,
}

fn generate_random_filename(extension: Option<&str>) -> String {
    let mut rng = thread_rng();
    let random_string: String = std::iter::repeat(())
        .map(|()| rng.sample(Alphanumeric))
        .map(char::from)
        .take(8)
        .collect();
    match extension {
        Some(ext) => format!("{}.{}", random_string, ext),
        None => random_string,
    }
}

fn get_extension_from_filename(filename: &str) -> Option<&str> {
    Path::new(filename).extension().and_then(OsStr::to_str)
}

pub async fn handle_upload(
    State(opt): State<Opt>,
    mut payload: Multipart,
) -> Result<impl IntoResponse, WebError> {
    let mut file_field: Option<FileUpload> = None;
    // Use default options field if we don't wish to include it.
    let mut options_field: Option<Options> = Some(Options {
        use_original_filename: false,
        redirect: true,
    });

    // iterate over multipart stream
    while let Ok(Some(mut field)) = payload.next_field().await {
        match field.name() {
            Some("file") => {
                // Save to temporary filename, we might later rename it to original.
                let original_filename = field.file_name().unwrap().to_string();
                let extension = get_extension_from_filename(&original_filename);
                let random_filename = generate_random_filename(extension);

                let filepath = filename_path(&random_filename, &opt)?;
                let random_filename_path = filepath.clone();
                // File::create is blocking operation, use threadpool
                let mut f =
                    tokio::task::spawn_blocking(|| std::fs::File::create(filepath)).await??;
                // Field in turn is stream of *Bytes* object
                let mut written_bytes = 0;
                while let Some(chunk) = field.next().await {
                    let data = chunk.unwrap();
                    written_bytes += data.len();
                    // filesystem operations are blocking, we have to use threadpool
                    f = tokio::task::spawn_blocking(move || f.write_all(&data).map(|_| f))
                        .await??;
                }

                // If uploaded file had a length of zero, delete the (zero length) file, return error
                // and delete temporary (empty) file.
                if written_bytes == 0 {
                    log::info!(
                        "tried to upload empty file {}, aborting.",
                        random_filename_path.display()
                    );
                    std::fs::remove_file(random_filename_path)?;
                    return Err(WebError::EmptyUpload);
                }

                file_field = Some(FileUpload {
                    original_filename,
                    random_filename,
                    random_filename_path,
                });
            }
            Some("options") => options_field = parse_field_options(field).await.ok(),
            _ => { /* TODO: show error or something */ }
        }
    }

    // Check if we received both file itself and data.
    if let (Some(file), Some(options)) = (file_field, options_field) {
        let final_filename: &str = if options.use_original_filename {
            // Rename from temporary random filename to original. Will overwrite if filename already exists.
            let original_filename_path = filename_path(&file.original_filename, &opt)?;
            std::fs::rename(&file.random_filename_path, original_filename_path)?;
            &file.original_filename
        } else {
            &file.random_filename
        };

        // Derive url of newly created file.
        let url = public_path(final_filename, &opt)?;

        // Generate thumbnail if the upload was an image.
        let final_path = filename_path(final_filename, &opt)?;
        let final_thumb_path = thumbnail_filename_path(final_filename, &opt)?;
        tokio::task::spawn(async move {
            // TODO: replace with some mpsc channel for thumbnails
            let _ = generate_thumbnail(&final_path, &final_thumb_path, &opt)
                .map_err(|e| println!("Error when generating thumbnail: {}", e));
        });

        let (status, headers) = if options.redirect {
            (
                StatusCode::SEE_OTHER,
                [(LOCATION, url.parse().unwrap())].into_iter().collect(),
            )
        } else {
            (StatusCode::OK, HeaderMap::new())
        };

        Ok((status, headers, Json(UploadResponse { url })))
    } else {
        Err(WebError::BadRequest)
    }
}

async fn parse_field_options(field: Field<'_>) -> Result<Options, WebError> {
    // Parse data in options json.

    // First read multipart data to Vec<u8>.
    let v = field.bytes().await.map_err(|_| WebError::BadRequest)?;

    serde_json::from_slice(&v).map_err(|_| WebError::BadRequest)
}

fn public_path(filename: &str, opt: &Opt) -> Result<String, url::ParseError> {
    let public_base = url::Url::parse(&opt.server_url)?;
    Ok(public_base.join(filename)?.into())
}
