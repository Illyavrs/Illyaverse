//! Native clients register through the website, then use their account to sign in.
//! This route intentionally never extracts or consumes a submitted credentials body.

use crate::common::AppState;
use axum::{
    Extension, Json, Router,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::json;

#[derive(Clone)]
struct WebsiteRegistration {
    url: String,
    message: String,
}

pub fn router(public_origin: &str) -> Result<Router<AppState>, &'static str> {
    let url = registration_url(public_origin)?;
    let message = format!("Please create your account at {url}, then sign in here.");
    Ok(Router::new()
        .route("/users", post(website_only))
        .route("/users/", post(website_only))
        .layer(Extension(WebsiteRegistration { url, message })))
}

fn registration_url(public_origin: &str) -> Result<String, &'static str> {
    let invalid = "PUBLIC_ORIGIN requires an HTTP or HTTPS origin without credentials, path, query or fragment";
    if public_origin.len() > 2048 {
        return Err(invalid);
    }
    let mut url = reqwest::Url::parse(public_origin).map_err(|_| invalid)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid);
    }
    url.set_path("/register");
    Ok(url.into())
}

async fn website_only(Extension(site): Extension<WebsiteRegistration>) -> Response {
    // Error consumers differ between native versions: stable reads field errors,
    // while lazer can also display a message and a registration link. No body
    // extractor means even streaming or oversized submissions are never buffered.
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "error": site.message,
            "url": site.url,
            "form_error": {
                "message": site.message,
                "redirect": site.url,
                "user": {
                    "username": [site.message],
                    "user_email": [],
                    "password": []
                }
            }
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use futures_util::stream;
    use serde_json::Value;
    use sqlx::postgres::PgPoolOptions;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tower::ServiceExt;

    fn state() -> AppState {
        AppState {
            pool: PgPoolOptions::new()
                .connect_lazy("postgresql://unused@127.0.0.1:1/unused")
                .unwrap(),
            storage: Arc::new(std::env::temp_dir()),
            sanctuary_url: Arc::new("http://127.0.0.1:1".into()),
            service_key: Arc::new(String::new()),
            http: reqwest::Client::new(),
            limits: Default::default(),
            trusted_proxies: Default::default(),
            signalr: Arc::new(crate::signalr::Runtime::new()),
            lazer_chat: Arc::new(crate::lazer_chat::Runtime::new()),
        }
    }

    #[test]
    fn configured_origin_is_the_only_registration_destination() {
        assert_eq!(
            registration_url("https://illya.test").unwrap(),
            "https://illya.test/register"
        );
        assert_eq!(
            registration_url("http://localhost:3000/").unwrap(),
            "http://localhost:3000/register"
        );
        assert_eq!(
            registration_url("https://EXAMPLE.COM:443/").unwrap(),
            "https://example.com/register"
        );
        for invalid in [
            "/",
            "//example.com",
            "ftp://example.com",
            "https://user@example.com",
            "https://user:password@example.com",
            "https://example.com/subdir",
            "https://example.com/?query=1",
            "https://example.com/#fragment",
        ] {
            assert!(registration_url(invalid).is_err());
        }
        assert!(registration_url(&format!("https://{}.test", "a".repeat(2048))).is_err());
    }

    #[tokio::test]
    async fn native_paths_and_validation_attempts_always_direct_to_website() {
        let app = router("https://illya.test").unwrap().with_state(state());
        for path in ["/users", "/users/", "/users?check=1", "/users/?check=1"] {
            for body in [
                "user%5Busername%5D=Player&user%5Buser_email%5D=private%40example.com&user%5Bpassword%5D=secret&check=0",
                "user%5Busername%5D=Player&user%5Buser_email%5D=private%40example.com&user%5Bpassword%5D=secret&check=1",
                "malformed-body",
            ] {
                let response = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri(path)
                            .header("host", "untrusted.example")
                            .header("x-forwarded-host", "untrusted.example")
                            .header("accept", "application/json")
                            .header("content-type", "application/x-www-form-urlencoded")
                            .body(Body::from(body))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
                assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
                assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
                assert!(!response.headers().contains_key(header::SET_COOKIE));
                let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
                let value: Value = serde_json::from_slice(&bytes).unwrap();
                let message =
                    "Please create your account at https://illya.test/register, then sign in here.";
                assert_eq!(value["error"], message);
                assert_eq!(value["url"], "https://illya.test/register");
                assert_eq!(value["form_error"]["message"], message);
                assert_eq!(
                    value["form_error"]["redirect"],
                    "https://illya.test/register"
                );
                assert_eq!(value["form_error"]["user"]["username"], json!([message]));
                assert_eq!(value["form_error"]["user"]["user_email"], json!([]));
                assert_eq!(value["form_error"]["user"]["password"], json!([]));
                let text = std::str::from_utf8(&bytes).unwrap();
                assert!(!text.contains("private@example.com"));
                assert!(!text.contains("secret"));
                assert!(!text.contains("untrusted.example"));
            }
        }
    }

    #[tokio::test]
    async fn streaming_credentials_are_never_consumed_or_buffered() {
        let polled = Arc::new(AtomicBool::new(false));
        let body_polled = polled.clone();
        let body = Body::from_stream(stream::once(async move {
            body_polled.store(true, Ordering::SeqCst);
            Ok::<_, std::io::Error>(vec![b'x'; 1_000_000])
        }));
        let app = router("https://illya.test").unwrap().with_state(state());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/users")
                    .header("content-type", "multipart/form-data; boundary=unused")
                    .body(body)
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert!(!polled.load(Ordering::SeqCst));
    }
}
