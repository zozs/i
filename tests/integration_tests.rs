use axum::{
    body::Body,
    http::{Request, StatusCode, header::LOCATION},
};
use http_body_util::BodyExt; // for `collect`
use i::{Opt, router};
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
async fn delete_small_file_without_referer() {
    let opt = make_test_opt();
    let app = router("/tmp".into(), opt);
    std::fs::File::create("/tmp/i_tests_to_be_deleted.txt").unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/delete")
                .method("POST")
                .header(
                    axum::http::header::CONTENT_TYPE,
                    "application/x-www-form-urlencoded",
                )
                .body(r#"filename=i_tests_to_be_deleted.txt"#.to_string())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert!(response.headers().get(LOCATION).is_some());
    assert_eq!(response.headers().get(LOCATION).unwrap(), "recent");
}

#[tokio::test]
async fn delete_small_file_with_referer() {
    let opt = make_test_opt();
    let app = router("/tmp".into(), opt);
    std::fs::File::create("/tmp/i_tests_to_be_deleted2.txt").unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/delete")
                .method("POST")
                .header(
                    axum::http::header::REFERER,
                    "http://localhost:8088/recent/2026?page=1",
                )
                .header(
                    axum::http::header::CONTENT_TYPE,
                    "application/x-www-form-urlencoded",
                )
                .body(r#"filename=i_tests_to_be_deleted2.txt"#.to_string())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert!(response.headers().get(LOCATION).is_some());
    assert_eq!(
        response.headers().get(LOCATION).unwrap(),
        "http://localhost:8088/recent/2026?page=1"
    );
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

#[tokio::test]
async fn auth_required_and_works() {
    let mut opt = make_test_opt();
    opt.auth_user = Some("admin".into());
    opt.auth_pass = Some("secret".into());
    let app = router("/tmp".into(), opt);

    // 1. Fail without credentials
    let response = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // 2. Fail with wrong credentials
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .header("Authorization", "Basic YWRtaW46d3Jvbmc=") // admin:wrong
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // 3. Succeed with correct credentials
    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header("Authorization", "Basic YWRtaW46c2VjcmV0") // admin:secret
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn csp_header() {
    let opt = make_test_opt();
    let app = router("/tmp".into(), opt);
    std::fs::write("/tmp/test_csp.html", "<html><body>hello</body></html>").unwrap();

    // 1. Verify CSP header IS present on served files (fallback service)
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/test_csp.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::CONTENT_SECURITY_POLICY)
            .unwrap(),
        "default-src 'none';"
    );

    // 2. Verify CSP header IS NOT present on main UI routes
    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .headers()
            .get(axum::http::header::CONTENT_SECURITY_POLICY)
            .is_none()
    );
}
