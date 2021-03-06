use actix_web::dev::ServiceRequest;
use actix_web::{middleware, web, App, Error, HttpResponse, HttpServer, Responder};
use actix_web_httpauth::extractors::basic::{BasicAuth, Config};
use actix_web_httpauth::extractors::AuthenticationError;
use actix_web_httpauth::middleware::HttpAuthentication;
use structopt::StructOpt;

mod recent;
mod upload;

#[derive(StructOpt, Clone, Debug)]
#[structopt(name = "i", about = "i is a simple file uploader web service.")]
pub struct Opt {
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

    /// Number of entries to show in the list of recent uploads
    #[structopt(short = "r", long, env, default_value = "15")]
    recents: usize,
}

async fn index() -> impl Responder {
    HttpResponse::Ok().body("i API ready!")
}

fn get_base_dir<'a>(opt: &'a Opt) -> std::io::Result<&'a str> {
    // Create directory where files should be uploaded.
    std::fs::create_dir_all(&opt.base_dir)?;

    Ok(&opt.base_dir)
}

fn auth_activated(opt: &Opt) -> bool {
    opt.auth_user.is_some() && opt.auth_pass.is_some()
}

async fn auth_validator(
    req: ServiceRequest,
    credentials: BasicAuth,
) -> Result<ServiceRequest, Error> {
    let opt: &Opt = req.app_data::<web::Data<Opt>>().unwrap();

    if let (Some(euser), Some(epass)) = (opt.auth_user.as_ref(), opt.auth_pass.as_ref()) {
        // Since both user and pass are given, we now require authentication. Check that they match.
        return match (credentials.user_id(), credentials.password()) {
            (auser, Some(apass)) if auser == euser && apass == epass => Ok(req), // success!
            _ => {
                let config: &Config = req.app_data::<web::Data<Config>>().unwrap();
                let config: Config = config.clone();
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
                    .route(web::post().to(upload::handle_upload)),
            )
            .service(
                web::resource("/recent")
                    .wrap(middleware::Condition::new(
                        auth_activated(&opt),
                        auth_recent,
                    ))
                    .route(web::get().to(recent::recent)),
            )
            .service(actix_files::Files::new("/", &base_dir))
    })
    .bind(bind_string)?
    .run()
    .await
}
