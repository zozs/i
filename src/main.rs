use std::io::Write;

use actix_multipart::Multipart;
use actix_web::{middleware, web, App, Error, HttpResponse, HttpServer, Responder};
use futures::{StreamExt, TryStreamExt};
use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;
use serde::Deserialize;
use std::path::Path;
use std::ffi::OsStr;

pub struct FileUpload {
    original_filename_path: String,
    random_filename_path: String,
}

#[derive(Deserialize)]
pub struct Options {
    use_original_filename: bool,
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

fn filename_path(filename: &str) -> String {
    return format!("./tmp/{}", sanitize_filename::sanitize(&filename));
}

fn get_extension_from_filename(filename: &str) -> Option<&str> {
    Path::new(filename)
        .extension()
        .and_then(OsStr::to_str)
}

async fn parse_field_options(field: actix_multipart::Field) -> Options {
    Options { use_original_filename: false }
}

async fn handle_upload(mut payload: Multipart) -> Result<HttpResponse, Error> {
    let mut file_field: Option<FileUpload> = None;
    let mut options_field: Option<Options> = None;

    log::debug!("handle upload!");

    // iterate over multipart stream
    while let Ok(Some(mut field)) = payload.try_next().await {
        log::debug!("got before if!");
        if let Some(content_disposition) = field.content_disposition() {
            // Check if it is file or data part of form.
            log::debug!("got disposition name {}!", content_disposition.get_name().unwrap());

            match content_disposition.get_name() {
                Some("file") => {
                    // Save to temporary filename, we might later rename it to original.
                    let original_filename = content_disposition.get_filename().unwrap();
                    let extension = get_extension_from_filename(original_filename);
                    let filename = generate_random_filename(extension);

                    let filepath = filename_path(&filename);
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
                        original_filename_path: filename_path(original_filename),
                        random_filename_path,
                    });
                }
                Some("options") => options_field = Some(parse_field_options(field).await),
                _ => { /* show error or something */ }
            }

            log::debug!("gotted disposition name {}!", content_disposition.get_name().unwrap());
        } else {
            log::debug!("no content disposition for you :(");
        }
    }

    log::debug!("done with parts!");

    // Check if we received both file itself and data.
    if let (Some(file), Some(options)) = (file_field, options_field) {
        log::debug!("found all parts!");
        if options.use_original_filename {
            // Rename from temporary random filename to original.
            std::fs::rename(file.random_filename_path, file.original_filename_path);
        }

        Ok(HttpResponse::Ok().into())
    } else {
        Ok(HttpResponse::BadRequest().into())
    }

}

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    // Create directory where files should be uploaded.
    std::fs::create_dir_all("./tmp").unwrap();

    HttpServer::new(|| {
        App::new().wrap(middleware::Logger::default()).service(
            web::resource("/")
                .route(web::get().to(index))
                .route(web::post().to(handle_upload)),
        )
    })
    .bind("0.0.0.0:8088")?
    .run()
    .await
}
