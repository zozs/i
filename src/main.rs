use actix_multipart::Multipart;
use actix_web::dev::ServiceRequest;
use actix_web::{middleware, web, App, Error, HttpResponse, HttpServer, Responder};
use actix_web_httpauth::extractors::basic::{BasicAuth, Config};
use actix_web_httpauth::extractors::AuthenticationError;
use actix_web_httpauth::middleware::HttpAuthentication;
use futures::{StreamExt, TryStreamExt};
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::io::Write;
use std::path::Path;
use structopt::StructOpt;

#[derive(StructOpt, Clone, Debug)]
#[structopt(name = "i", about = "i is a simple file uploader web service.")]
struct Opt {
    /// Enable verbose logging
    #[structopt(short, long)]
    verbose: bool,

    /// Port to listen on.
    #[structopt(short = "P", long, default_value = "8088", env)]
    port: u16,

    /// The file system directory where uploaded files will be stored to, and served from.
    #[structopt(short, long, env, default_value = "./tmp")]
    base_dir: String,

    /// The complete server URL base which should be used when generating links.
    #[structopt(short, long, env, default_value = "http://localhost:8088")]
    server_url: String,

    /// Username for basic auth, if you want to require authentication to upload files
    #[structopt(short = "u", long, env)]
    auth_user: Option<String>,

    /// Password for basic auth, if you want to require authentication to upload files
    #[structopt(short = "p", long, env)]
    auth_pass: Option<String>,
}

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
    url: String,
}

async fn index() -> impl Responder {
    HttpResponse::Ok().body("i API ready!")
}

async fn recent() -> impl Responder {
    HttpResponse::Ok().body("you have reached the /recent endpoint.")
}

fn generate_random_filename(extension: Option<&str>) -> String {
    let random_string = thread_rng().sample_iter(&Alphanumeric).take(8).collect();
    match extension {
        Some(ext) => format!("{}.{}", random_string, ext),
        None => random_string,
    }
}

fn filename_path(filename: &str, opt: &Opt) -> Result<String, Error> {
    Ok(format!(
        "{}/{}",
        get_base_dir(opt)?,
        sanitize_filename::sanitize(&filename)
    ))
}

fn get_base_dir<'a>(opt: &'a Opt) -> std::io::Result<&'a str> {
    // Create directory where files should be uploaded.
    std::fs::create_dir_all(&opt.base_dir)?;

    Ok(&opt.base_dir)
}

fn get_extension_from_filename(filename: &str) -> Option<&str> {
    Path::new(filename).extension().and_then(OsStr::to_str)
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

async fn handle_upload(mut payload: Multipart, opt: web::Data<Opt>) -> Result<HttpResponse, Error> {
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

        Ok(HttpResponse::SeeOther()
            .header("Location", url.as_str())
            .json(UploadResponse { url }))
    } else {
        Ok(HttpResponse::BadRequest().into())
    }
}

fn auth_activated(opt: &Opt) -> bool {
    opt.auth_user.is_some() && opt.auth_pass.is_some()
}

async fn auth_validator(
    req: ServiceRequest,
    credentials: BasicAuth,
) -> Result<ServiceRequest, Error> {
    let opt = req
        .app_data::<Opt>()
        .map(|data| data.get_ref().clone())
        .unwrap();

    if let (Some(euser), Some(epass)) = (opt.auth_user, opt.auth_pass) {
        // Since both user and pass are given, we now require authentication. Check that they match.
        return match (credentials.user_id(), credentials.password()) {
            (auser, Some(apass)) if auser == &euser && apass == &epass => Ok(req), // success!
            _ => {
                let config = req
                    .app_data::<Config>()
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
    let opt = Opt::from_args();

    env_logger::init();

    let host = "0.0.0.0";
    let bind_string = format!("{}:{}", host, opt.port);

    let base_dir = get_base_dir(&opt)?.to_string();

    log::info!("listening on {}", bind_string);
    log::info!("serving and storing files in: {}", base_dir);

    HttpServer::new(move || {
        let auth = HttpAuthentication::basic(auth_validator);
        let auth_recent = auth.clone();

        App::new()
            .wrap(middleware::Logger::default())
            .data(Config::default().realm("i: file upload"))
            .data(opt.clone())
            .service(
                web::resource("/")
                    .wrap(middleware::Condition::new(auth_activated(&opt), auth))
                    .route(web::get().to(index))
                    .route(web::post().to(handle_upload)),
            )
            .service(
                web::resource("/recent")
                    .wrap(middleware::Condition::new(
                        auth_activated(&opt),
                        auth_recent,
                    ))
                    .route(web::get().to(recent)),
            )
            .service(actix_files::Files::new("/", &base_dir))
    })
    .bind(bind_string)?
    .run()
    .await
}
