use actix_web::dev::{fn_service, ServiceRequest, ServiceResponse};
use actix_web::web::Bytes;
use actix_web::{middleware, web, App, Error, HttpResponse, HttpServer, Responder};
use actix_web_httpauth::extractors::basic::{BasicAuth, Config};
use actix_web_httpauth::extractors::AuthenticationError;
use actix_web_httpauth::middleware::HttpAuthentication;
use askama_actix::Template;
use clap::Parser;
use std::path::{Path, PathBuf};

mod delete;
mod helpers;
mod recent;
mod thumbnail;
mod upload;

#[derive(clap::Parser, Clone, Debug)]
#[command(name = "i", about = "i is a simple file uploader web service.")]
pub struct Opt {
    /// Port to listen on.
    #[arg(short = 'P', long, default_value = "8088", env)]
    port: u16,

    /// The file system directory where uploaded files will be stored to, and served from.
    #[arg(short, long, env, default_value = "./tmp")]
    base_dir: String,

    /// The complete server URL base which should be used when generating links.
    #[arg(short, long, env, default_value = "http://localhost:8088")]
    server_url: String,

    /// Username for basic auth, if you want to require authentication to upload files
    #[arg(short = 'u', long, env)]
    auth_user: Option<String>,

    /// Password for basic auth, if you want to require authentication to upload files
    #[arg(short = 'p', long, env)]
    auth_pass: Option<String>,

    /// Number of entries to show in the list of recent uploads
    #[arg(short = 'r', long, env, default_value_t = 15)]
    recents: usize,

    /// Thumbnail size
    #[arg(short, long, env, default_value_t = 150)]
    thumbnail_size: u32,

    /// Request logger format
    #[arg(
        short,
        long,
        env,
        default_value = r#"%a "%r" %s %b "%{Referer}i" "%{User-Agent}i" %T""#
    )]
    logger_format: String,
}

pub const THUMBNAIL_SUBDIR: &str = "thumbnails";

#[derive(Template)]
#[template(path = "notfound.html")]
struct NotFoundTemplate {}

async fn bulma() -> impl Responder {
    let bulma = include_str!("../dist/bulma.min.css");
    HttpResponse::Ok().content_type("text/css").body(bulma)
}

async fn index() -> impl Responder {
    HttpResponse::Ok().body("i API ready!")
}

async fn not_found(req: ServiceRequest) -> actix_web::Result<ServiceResponse> {
    let template = NotFoundTemplate {};
    let res = match template.render() {
        Ok(rendered) => HttpResponse::NotFound()
            .content_type(NotFoundTemplate::MIME_TYPE)
            .body(rendered),
        Err(_) => HttpResponse::InternalServerError().into(),
    };

    let (req, _) = req.into_parts();
    Ok(ServiceResponse::new(req, res))
}

async fn placeholder_thumbnail() -> impl Responder {
    let placeholder = Bytes::from_static(include_bytes!("../dist/placeholder.png"));
    HttpResponse::Ok()
        .content_type("image/png")
        .body(placeholder)
}

fn get_base_dir(opt: &Opt) -> std::io::Result<PathBuf> {
    // Create directory where files should be uploaded.
    let path = Path::new(&opt.base_dir);
    std::fs::create_dir_all(path)?;

    Ok(path.to_path_buf())
}

fn get_thumbnail_dir(opt: &Opt) -> std::io::Result<PathBuf> {
    // Create directory where thumbnails should be uploaded.
    let path = std::path::Path::new(&opt.base_dir);
    let path = path.join(THUMBNAIL_SUBDIR);
    std::fs::create_dir_all(&path)?;

    Ok(path)
}

fn auth_active(opt: &Opt) -> bool {
    opt.auth_user.is_some() && opt.auth_pass.is_some()
}

async fn auth_validator(
    req: ServiceRequest,
    credentials: BasicAuth,
) -> Result<ServiceRequest, (Error, ServiceRequest)> {
    let opt: &Opt = req.app_data::<web::Data<Opt>>().unwrap();

    if let (Some(euser), Some(epass)) = (opt.auth_user.as_ref(), opt.auth_pass.as_ref()) {
        // Since both user and pass are given, we now require authentication. Check that they match.
        return match (credentials.user_id(), credentials.password()) {
            (auser, Some(apass)) if auser == euser && apass == epass => Ok(req), // success!
            _ => {
                let config: &Config = req.app_data::<web::Data<Config>>().unwrap();
                let config: Config = config.clone();
                Err((AuthenticationError::from(config).into(), req))
            }
        };
    }
    Ok(req)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let opt = Opt::parse();

    env_logger::init();

    let host = "0.0.0.0";
    let bind_string = format!("{}:{}", host, opt.port);

    let base_dir = get_base_dir(&opt)?;

    log::info!("listening on {}", bind_string);
    log::info!("serving and storing files in: {:?}", base_dir);

    HttpServer::new(move || {
        let auth = HttpAuthentication::basic(auth_validator);

        App::new()
            .wrap(middleware::Logger::new(&opt.logger_format))
            .app_data(web::Data::new(Config::default().realm("i: file upload")))
            .app_data(web::Data::new(opt.clone()))
            .service(
                web::resource("/")
                    .wrap(middleware::Condition::new(auth_active(&opt), auth.clone()))
                    .route(web::get().to(index))
                    .route(web::post().to(upload::handle_upload)),
            )
            .service(
                web::resource("/delete")
                    .wrap(middleware::Condition::new(auth_active(&opt), auth.clone()))
                    .route(web::post().to(delete::handle_delete)),
            )
            .service(
                web::resource("/recent")
                    .wrap(middleware::Condition::new(auth_active(&opt), auth))
                    .route(web::get().to(recent::recent)),
            )
            .service(web::resource("/recent/bulma.min.css").route(web::get().to(bulma)))
            .service(
                web::resource("/recent/placeholder.png")
                    .route(web::get().to(placeholder_thumbnail)),
            )
            .service(actix_files::Files::new("/", &base_dir).default_handler(fn_service(not_found)))
    })
    .bind(bind_string)?
    .run()
    .await
}
