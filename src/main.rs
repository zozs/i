use clap::Parser;
use i::{Opt, WebError, get_base_dir, router};
use tracing_subscriber::EnvFilter;

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

    log::info!("listening on {bind_string}");
    log::info!("serving and storing files in: {base_dir:?}");

    let app = router(base_dir, opt);

    let listener = tokio::net::TcpListener::bind(bind_string).await.unwrap();
    Ok(axum::serve(listener, app).await?)
}
