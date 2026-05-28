use askama::Template;
use askama_web::WebTemplate;
use axum::{
    Router,
    extract::{DefaultBodyLimit, Request, State},
    handler::HandlerWithoutStateExt,
    http::{
        StatusCode,
        header::{CONTENT_TYPE, WWW_AUTHENTICATE},
    },
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Basic},
};
use constant_time_eq::constant_time_eq;
use image::ImageError;
use std::path::{Path, PathBuf};
use tokio::task::JoinError;
use tower_http::{
    services::ServeDir,
    set_header::SetResponseHeaderLayer,
    trace::{DefaultMakeSpan, TraceLayer},
};

pub mod delete;
pub mod helpers;
pub mod recent;
pub mod thumbnail;
pub mod upload;

#[derive(clap::Parser, Clone, Debug)]
#[command(name = "i", about = "i is a simple file uploader web service.")]
pub struct Opt {
    /// Port to listen on.
    #[arg(short = 'P', long, default_value = "8088", env)]
    pub port: u16,

    /// The file system directory where uploaded files will be stored to, and served from.
    #[arg(short, long, env, default_value = "./tmp")]
    pub base_dir: String,

    /// The complete server URL base which should be used when generating links.
    #[arg(short, long, env, default_value = "http://localhost:8088")]
    pub server_url: String,

    /// Username for basic auth, if you want to require authentication to upload files
    #[arg(short = 'u', long, env)]
    pub auth_user: Option<String>,

    /// Password for basic auth, if you want to require authentication to upload files
    #[arg(short = 'p', long, env)]
    pub auth_pass: Option<String>,

    /// Number of entries to show in the list of recent uploads
    #[arg(short = 'r', long, env, default_value_t = 18)]
    pub recents: usize,

    /// Thumbnail size
    #[arg(short, long, env, default_value_t = 150)]
    pub thumbnail_size: u32,

    /// Maximum upload size in bytes (default 2 GiB)
    #[arg(short, long, env, default_value_t = 2_147_483_648)]
    pub max_upload_size: usize,
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

#[derive(Template, WebTemplate)]
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

pub fn get_base_dir(opt: &Opt) -> std::io::Result<PathBuf> {
    // Create directory where files should be uploaded.
    let path = Path::new(&opt.base_dir);
    std::fs::create_dir_all(path)?;

    Ok(path.to_path_buf())
}

pub fn get_thumbnail_dir(opt: &Opt) -> std::io::Result<PathBuf> {
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
            let auser = creds.username();
            let apass = creds.password();

            // We use constant-time comparison to prevent timing attacks.
            // We must check both username AND password to prevent leaking username validity.
            let user_match = constant_time_eq(auser.as_bytes(), euser.as_bytes());
            let pass_match = constant_time_eq(apass.as_bytes(), epass.as_bytes());

            if user_match && pass_match {
                Ok(next.run(request).await)
            } else {
                Err(WebError::AuthenticationFailed)
            }
        } else {
            Err(WebError::AuthenticationFailed)
        }
    } else {
        Ok(next.run(request).await)
    }
}

pub fn router(base_dir: PathBuf, opt: Opt) -> Router {
    let max_upload = opt.max_upload_size;
    let serve_dir = ServeDir::new(&base_dir).not_found_service(handle_404.into_service());
    let serve_dir_with_csp = tower::ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::CONTENT_SECURITY_POLICY,
            axum::http::HeaderValue::from_static("default-src 'none';"),
        ))
        .service(serve_dir);

    let tracing_layer =
        TraceLayer::new_for_http().make_span_with(DefaultMakeSpan::new().include_headers(true));

    Router::new()
        .route("/", get(index))
        .route("/", post(upload::handle_upload))
        .route("/delete", post(delete::handle_delete))
        .route("/recent", get(recent::recent_pagination))
        .route("/recent/size", get(recent::recent_pagination_size))
        .route(
            "/recent/{year}/{month}",
            get(recent::recent_pagination_year_month),
        )
        .route(
            "/recent/{year}/{month}/size",
            get(recent::recent_pagination_year_month_size),
        )
        .route("/recent/{year}", get(recent::recent_pagination_year))
        .route(
            "/recent/{year}/size",
            get(recent::recent_pagination_year_size),
        )
        .route_layer(middleware::from_fn_with_state(opt.clone(), auth_validator)) // every route above covered by auth
        .route("/recent/bulma.min.css", get(bulma))
        .route("/recent/placeholder.png", get(placeholder_thumbnail))
        .fallback_service(serve_dir_with_csp)
        .with_state(opt)
        .layer(tracing_layer)
        .layer(DefaultBodyLimit::max(max_upload))
}
