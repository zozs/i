use askama_axum::Template;
use axum::{
    extract::{Request, State},
    handler::HandlerWithoutStateExt,
    http::{
        header::{CONTENT_TYPE, WWW_AUTHENTICATE},
        StatusCode,
    },
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use axum_extra::{
    headers::{authorization::Basic, Authorization},
    TypedHeader,
};
use clap::Parser;
use image::ImageError;
use std::path::{Path, PathBuf};
use tokio::task::JoinError;
use tower_http::services::ServeDir;

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

#[derive(Debug, thiserror::Error)]
pub enum WebError {
    #[error("authentication failed")]
    AuthenticationFailed,
    #[error("tried to upload empty file")]
    EmptyUpload,
    #[error("i/o error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("thread pool error: {0}")]
    ThreadPoolError(#[from] JoinError),
    #[error("invalid url")]
    InvalidUrl(#[from] url::ParseError),
    #[error("bad request")]
    BadRequest,
    #[error("image error")]
    InvalidImage(#[from] ImageError),
}

impl axum::response::IntoResponse for WebError {
    fn into_response(self) -> Response {
        match self {
            WebError::AuthenticationFailed => (
                StatusCode::UNAUTHORIZED,
                [(WWW_AUTHENTICATE, "Basic realm=\"i: file upload\"")],
                "unauthorized",
            )
                .into_response(),
            WebError::EmptyUpload => (StatusCode::BAD_REQUEST, self.to_string()).into_response(),
            WebError::IoError(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "i/o error").into_response()
            }
            WebError::ThreadPoolError(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
            }
            WebError::InvalidUrl(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "invalid url").into_response()
            }
            WebError::BadRequest => (StatusCode::BAD_REQUEST, "bad request").into_response(),
            WebError::InvalidImage(_) => (StatusCode::BAD_REQUEST, "invalid image").into_response(),
        }
    }
}

#[derive(Template)]
#[template(path = "notfound.html")]
struct NotFoundTemplate {}

async fn bulma() -> impl IntoResponse {
    let placeholder = include_bytes!("../dist/bulma.min.css");
    ([(CONTENT_TYPE, "text/css")], placeholder)
}

async fn index() -> impl IntoResponse {
    "i API ready!"
}

async fn handle_404() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, NotFoundTemplate {})
}

async fn placeholder_thumbnail() -> impl IntoResponse {
    let placeholder = include_bytes!("../dist/placeholder.png");
    ([(CONTENT_TYPE, "image/png")], placeholder)
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

async fn auth_validator(
    State(opt): State<Opt>,
    creds: Option<TypedHeader<Authorization<Basic>>>,
    request: Request,
    next: middleware::Next,
) -> Result<Response, WebError> {
    if let (Some(euser), Some(epass)) = (opt.auth_user.as_ref(), opt.auth_pass.as_ref()) {
        // Since both user and pass are given, we now require authentication. Check that they match.
        if let Some(TypedHeader(Authorization(creds))) = creds {
            match (creds.username(), creds.password()) {
                (auser, apass) if auser == euser && apass == epass => Ok(next.run(request).await),
                _ => Err(WebError::AuthenticationFailed),
            }
        } else {
            Err(WebError::AuthenticationFailed)
        }
    } else {
        Ok(next.run(request).await)
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let opt = Opt::parse();

    env_logger::init();

    let host = "0.0.0.0";
    let bind_string = format!("{}:{}", host, opt.port);

    let base_dir = get_base_dir(&opt)?;

    log::info!("listening on {}", bind_string);
    log::info!("serving and storing files in: {:?}", base_dir);

    let serve_dir = ServeDir::new(&base_dir).not_found_service(handle_404.into_service());

    // TODO: fix logger
    let app = Router::new()
        .route("/", get(index))
        .route("/", post(upload::handle_upload))
        .route("/delete", post(delete::handle_delete))
        .route("/recent", get(recent::recent))
        .route_layer(middleware::from_fn_with_state(opt.clone(), auth_validator)) // every route above covered by auth
        .route("/recent/bulma.min.css", get(bulma))
        .route("/recent/placeholder.png", get(placeholder_thumbnail))
        .fallback_service(serve_dir)
        .with_state(opt);

    let listener = tokio::net::TcpListener::bind(bind_string).await.unwrap();
    axum::serve(listener, app).await
}
