use actix_multipart::Multipart;
use actix_web::{web, Error, HttpResponse};
use futures::{StreamExt, TryStreamExt};
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::io::Write;
use std::path::Path;

use super::{get_base_dir, Opt};

struct FileUpload {
    original_filename: String,
    random_filename: String,
    random_filename_path: String,
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

fn filename_path(filename: &str, opt: &Opt) -> Result<String, Error> {
    Ok(format!(
        "{}/{}",
        get_base_dir(opt)?,
        sanitize_filename::sanitize(&filename)
    ))
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
    mut payload: Multipart,
    opt: web::Data<Opt>,
) -> Result<HttpResponse, Error> {
    let mut file_field: Option<FileUpload> = None;
    // Use default options field if we don't wish to include it.
    let mut options_field: Option<Options> = Some(Options {
        use_original_filename: false,
        redirect: true,
    });

    // iterate over multipart stream
    while let Ok(Some(mut field)) = payload.try_next().await {
        if let Some(content_disposition) = field.content_disposition() {
            // Check if it is file or data part of form.
            match content_disposition.get_name() {
                Some("file") => {
                    // Save to temporary filename, we might later rename it to original.
                    let original_filename = content_disposition.get_filename().unwrap();
                    let extension = get_extension_from_filename(original_filename);
                    let random_filename = generate_random_filename(extension);

                    let filepath = filename_path(&random_filename, &opt)?;
                    let random_filename_path = filepath.clone();
                    // File::create is blocking operation, use threadpool
                    let mut f = web::block(|| std::fs::File::create(filepath))
                        .await
                        .map_err(|_| {
                            actix_web::error::ErrorInternalServerError("Could not upload file")
                        })?;
                    // Field in turn is stream of *Bytes* object
                    while let Some(chunk) = field.next().await {
                        let data = chunk.unwrap();
                        // filesystem operations are blocking, we have to use threadpool
                        f = web::block(move || f.write_all(&data).map(|_| f)).await?;
                    }
                    file_field = Some(FileUpload {
                        original_filename: original_filename.to_string(),
                        random_filename,
                        random_filename_path,
                    });
                }
                Some("options") => options_field = parse_field_options(field).await.ok(),
                _ => { /* TODO: show error or something */ }
            }
        } else {
            log::debug!("no content disposition in field :(");
        }
    }

    // Check if we received both file itself and data.
    if let (Some(file), Some(options)) = (file_field, options_field) {
        let final_filename: &str = if options.use_original_filename {
            // Rename from temporary random filename to original. Will overwrite if filename already exists.
            let original_filename_path = filename_path(&file.original_filename, &opt)?;
            std::fs::rename(&file.random_filename_path, &original_filename_path)?;
            &file.original_filename
        } else {
            &file.random_filename
        };

        // Derive url of newly created file.
        let url =
            public_path(final_filename, &opt).map_err(|_| HttpResponse::InternalServerError())?;

        let response = if options.redirect {
            HttpResponse::SeeOther()
                .header("Location", url.as_str())
                .json(UploadResponse { url })
        } else {
            HttpResponse::Ok().json(UploadResponse { url })
        };

        Ok(response)
    } else {
        Ok(HttpResponse::BadRequest().into())
    }
}

async fn parse_field_options(mut field: actix_multipart::Field) -> Result<Options, Error> {
    // Parse data in options json.

    // First read multipart data to Vec<u8>.
    let mut v: Vec<u8> = Vec::new();
    while let Some(chunk) = field.next().await {
        let data = chunk.unwrap();
        // filesystem operations are blocking, we have to use threadpool
        v = web::block(move || v.write_all(&data).map(|_| v)).await?;
    }

    Ok(serde_json::from_slice(&v)?)
}

fn public_path(filename: &str, opt: &Opt) -> Result<String, url::ParseError> {
    let public_base = url::Url::parse(&opt.server_url)?;
    Ok(public_base.join(filename)?.into_string())
}
