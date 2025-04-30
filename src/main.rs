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
use clap::Parser;
use image::ImageError;
use std::path::{Path, PathBuf};
use tokio::task::JoinError;
use tower_http::{
    services::ServeDir,
    trace::{DefaultMakeSpan, TraceLayer},
};
use tracing_subscriber::EnvFilter;

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

    /// Maximum upload size in bytes (default 2 GiB)
    #[arg(short, long, env, default_value_t = 2_147_483_648)]
    max_upload_size: usize,
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

fn router(base_dir: PathBuf, opt: Opt) -> Router {
    let max_upload = opt.max_upload_size;
    let serve_dir = ServeDir::new(&base_dir).not_found_service(handle_404.into_service());
    let tracing_layer =
        TraceLayer::new_for_http().make_span_with(DefaultMakeSpan::new().include_headers(true));

    Router::new()
        .route("/", get(index))
        .route("/", post(upload::handle_upload))
        .route("/delete", post(delete::handle_delete))
        .route("/recent", get(recent::recent))
        .route_layer(middleware::from_fn_with_state(opt.clone(), auth_validator)) // every route above covered by auth
        .route("/recent/bulma.min.css", get(bulma))
        .route("/recent/placeholder.png", get(placeholder_thumbnail))
        .fallback_service(serve_dir)
        .with_state(opt)
        .layer(tracing_layer)
        .layer(DefaultBodyLimit::max(max_upload))
}

#[tokio::main]
async fn main() -> Result<(), WebError> {
    let opt = Opt::parse();

    // Configure tracing
    let default = "i=info".parse().unwrap();
    let filter = EnvFilter::builder()
        .with_default_directive(default)
        .from_env_lossy();
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let host = "0.0.0.0";
    let bind_string = format!("{}:{}", host, opt.port);

    let base_dir = get_base_dir(&opt)?;

    log::info!("listening on {}", bind_string);
    log::info!("serving and storing files in: {:?}", base_dir);

    let app = router(base_dir, opt);

    let listener = tokio::net::TcpListener::bind(bind_string).await.unwrap();
    Ok(axum::serve(listener, app).await?)
}

#[cfg(test)]
mod tests {

    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode, header::LOCATION},
    };
    use http_body_util::BodyExt; // for `collect`
    use serde_json::Value;

    use tower::ServiceExt; // for `call`, `oneshot`, and `ready`

    fn make_test_opt() -> Opt {
        Opt {
            port: 1337,
            base_dir: "/tmp".into(),
            server_url: "http://test.example.com".into(),
            auth_user: None,
            auth_pass: None,
            recents: 1,
            thumbnail_size: 150,
            max_upload_size: 30 * 1024 * 1024,
        }
    }

    #[tokio::test]
    async fn hello_world() {
        let opt = make_test_opt();
        let app = router("/tmp".into(), opt);

        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"i API ready!");
    }

    #[tokio::test]
    async fn post_small_file() {
        let opt = make_test_opt();
        let app = router("/tmp".into(), opt);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .method("POST")
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        "multipart/form-data; boundary=boundary",
                    )
                    .body(
                        r#"--boundary
Content-Disposition: form-data; name="file"; filename="original.txt"
Content-Type: text/plain

hellu this is a cute little file UwU

--boundary--
"#
                        .replace('\n', "\r\n"),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert!(response.headers().get(LOCATION).is_some());

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert!(body.get("url").is_some())
    }

    #[tokio::test]
    async fn post_small_file_original() {
        let opt = make_test_opt();
        let app = router("/tmp".into(), opt);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .method("POST")
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        "multipart/form-data; boundary=boundary",
                    )
                    .body(
                        r#"--boundary
Content-Disposition: form-data; name="file"; filename="original.txt"
Content-Type: text/plain

hellu this is a cute little file UwU

--boundary
Content-Disposition: form-data; name="options"

{"useOriginalFilename":true}
--boundary--
"#
                        .replace('\n', "\r\n"),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            Some("http://test.example.com/original.txt"),
            body.get("url").map(|v| v.as_str().unwrap())
        );
    }

    #[tokio::test]
    async fn post_small_file_no_redirect() {
        let opt = make_test_opt();
        let app = router("/tmp".into(), opt);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .method("POST")
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        "multipart/form-data; boundary=boundary",
                    )
                    .body(
                        r#"--boundary
Content-Disposition: form-data; name="file"; filename="original.txt"
Content-Type: text/plain

hellu this is a cute little file UwU

--boundary
Content-Disposition: form-data; name="options"

{"redirect":false}
--boundary--
"#
                        .replace('\n', "\r\n"),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get(LOCATION).is_none());

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert!(body.get("url").is_some())
    }

    #[tokio::test]
    async fn post_big_file() {
        let opt = make_test_opt();
        let app = router("/tmp".into(), opt);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .method("POST")
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        "multipart/form-data; boundary=boundary",
                    )
                    .body(
                        format!(
                            r#"--boundary
Content-Disposition: form-data; name="file"; filename="original.txt"
Content-Type: text/plain

{}

--boundary--
"#,
                            "1234567890abcdef\n".repeat(64 * 1024 * 20)
                        )
                        .replace('\n', "\r\n"),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert!(body.get("url").is_some())
    }
}
