use actix_multipart::Multipart;
use actix_web::{middleware, web, App, Error, HttpResponse, HttpServer, Responder};
use actix_web::dev::ServiceRequest;
use actix_web_httpauth::extractors::AuthenticationError;
use actix_web_httpauth::extractors::basic::{BasicAuth, Config};
use actix_web_httpauth::middleware::HttpAuthentication;
use futures::{StreamExt, TryStreamExt};
use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use std::ffi::OsStr;

pub struct FileUpload {
    original_filename: String,
    random_filename: String,
    random_filename_path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    #[serde(default)]
    use_original_filename: bool, // default for bool is false.
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadResponse {
    url: String
}

async fn index() -> impl Responder {
    HttpResponse::Ok().body("i API ready!")
}

fn generate_random_filename(extension: Option<&str>) -> String {
    let random_string = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(8)
        .collect();
    match extension {
        Some(ext) => format!("{}.{}", random_string, ext),
        None => random_string,
    }
}

fn filename_path(filename: &str) -> Result<String, Error> {
    Ok(format!("{}/{}", get_base_dir()?, sanitize_filename::sanitize(&filename)))
}

fn get_base_dir() -> std::io::Result<&'static str> {
    let base_dir = option_env!("I_BASE_DIR").unwrap_or("./tmp");

    // Create directory where files should be uploaded.
    std::fs::create_dir_all(base_dir)?;

    Ok(base_dir)
}

fn get_extension_from_filename(filename: &str) -> Option<&str> {
    Path::new(filename)
        .extension()
        .and_then(OsStr::to_str)
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

fn public_path(filename: &str) -> Result<String, url::ParseError> {
    let port = option_env!("I_PORT").unwrap_or("8088");
    let public_base_string = match option_env!("I_PUBLIC_BASE") {
        Some(s) => s.to_string(),
        None => format!("http://localhost:{}", port),
    };

    let public_base = url::Url::parse(&public_base_string)?;
    Ok(public_base.join(filename)?.into_string())
}

async fn handle_upload(mut payload: Multipart) -> Result<HttpResponse, Error> {
    let mut file_field: Option<FileUpload> = None;
    let mut options_field: Option<Options> = None;

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

                    let filepath = filename_path(&random_filename)?;
                    let random_filename_path = filepath.clone();
                    // File::create is blocking operation, use threadpool
                    let mut f = web::block(|| std::fs::File::create(filepath))
                        .await
                        .unwrap();
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
            let original_filename_path = filename_path(&file.original_filename)?;
            std::fs::rename(&file.random_filename_path, &original_filename_path)?;
            &file.original_filename
        } else {
            &file.random_filename
        };

        // Derive url of newly created file.
        let url = public_path(final_filename).map_err(|_| HttpResponse::InternalServerError())?;

        Ok(
            HttpResponse::SeeOther()
                .header("Location", url.as_str())
                .json(UploadResponse {
                    url
                })
        )
    } else {
        Ok(HttpResponse::BadRequest().into())
    }

}

fn auth_activated() -> bool {
    option_env!("I_AUTH_USER").is_some() && option_env!("I_AUTH_PASS").is_some()
}

async fn auth_validator(
    req: ServiceRequest,
    credentials: BasicAuth,
) -> Result<ServiceRequest, Error> {
    if let (Some(euser), Some(epass)) = (option_env!("I_AUTH_USER"), option_env!("I_AUTH_PASS")) {
        // Since both user and pass are given, we now require authentication. Check that they match.
        return match (credentials.user_id(), credentials.password()) {
            (auser, Some(apass)) if auser == euser && apass == epass => Ok(req), // success!
            _ => {
                let config = req.app_data::<Config>()
                    .map(|data| data.get_ref().clone())
                    .unwrap_or_else(Default::default);
                Err(AuthenticationError::from(config).into())
            }
        };
    }
    Ok(req)
}

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    let host = "0.0.0.0";
    let port = option_env!("I_PORT").unwrap_or("8088");
    let bind_string = format!("{}:{}", host, port);

    let base_dir = get_base_dir()?;

    log::info!("listening on {}", bind_string);
    log::info!("serving and storing files in: {}", base_dir);

    HttpServer::new(move || {
        let auth = HttpAuthentication::basic(auth_validator);

        App::new().wrap(middleware::Logger::default())
            .data(Config::default().realm("i: file upload"))
            .service(
                web::resource("/")
                    .wrap(middleware::Condition::new(auth_activated(), auth))
                    .route(web::get().to(index))
                    .route(web::post().to(handle_upload))
            )
            .service(actix_files::Files::new("/", base_dir))
    })
    .bind(bind_string)?
    .run()
    .await
}
