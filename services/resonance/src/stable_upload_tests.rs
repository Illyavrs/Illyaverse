//! Multipart-reader tests use synthetic bodies and never reach score ingestion.
use super::{
    ScoreUpload, parse_score_submission, read_score_upload, resolve_upload_disposition,
    upload_disposition_metadata,
};
use crate::common::ApiError;
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{DefaultBodyLimit, Multipart},
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use std::{
    convert::Infallible,
    io::Write,
    process::Command,
    sync::{Arc, Mutex},
    task::Poll,
};
use tokio::sync::oneshot;
use tower::ServiceExt;

const BOUNDARY: &str = "illyaverse-stable-upload-fixture";
const SCALAR_BYTES: usize = 32 * 1024;
const REPLAY_BYTES: usize = 16 * 1024 * 1024;
const TOTAL_BYTES: usize = 20 * 1024 * 1024;
const FRAMEWORK_BODY_LIMIT: usize = 512;

// Serialize the large payload fixtures so their buffers have a bounded peak.
static UPLOAD_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const DIAGNOSTIC_CHILD_MARKER: &str = "ILLYA_STABLE_UPLOAD_DIAGNOSTIC_FIXTURE";

fn diagnostic_runs_in_child(test_name: &'static str) -> bool {
    if std::env::var(DIAGNOSTIC_CHILD_MARKER).as_deref() == Ok(test_name) {
        return false;
    }
    // Each logging assertion gets its own process-wide callsite registry and
    // subscriber. The marker is set on the child only, never in this process.
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--nocapture"])
        .env(DIAGNOSTIC_CHILD_MARKER, test_name)
        .output()
        .expect("start isolated multipart diagnostic fixture");
    assert!(
        output.status.success(),
        "isolated diagnostic fixture failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    true
}

struct Part {
    name: String,
    filename: Option<String>,
    content_type: String,
    bytes: Vec<u8>,
}
impl Part {
    fn text(name: &str, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            name: name.into(),
            filename: None,
            content_type: "text/plain".into(),
            bytes: bytes.into(),
        }
    }
    fn file(name: &str, filename: &str, bytes: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            filename: Some(filename.into()),
            content_type: "application/octet-stream".into(),
            bytes,
        }
    }
}

fn multipart_bytes(parts: Vec<Part>) -> Vec<u8> {
    let mut bytes = Vec::new();
    for part in parts {
        bytes.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{}\"",
                part.name
            )
            .as_bytes(),
        );
        if let Some(filename) = part.filename {
            bytes.extend_from_slice(format!("; filename=\"{filename}\"").as_bytes());
        }
        if !part.content_type.is_empty() {
            bytes.extend_from_slice(format!("\r\nContent-Type: {}", part.content_type).as_bytes());
        }
        bytes.extend_from_slice(b"\r\n\r\n");
        bytes.extend_from_slice(&part.bytes);
        bytes.extend_from_slice(b"\r\n");
    }
    bytes.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    bytes
}

fn custom_disposition_body(
    disposition: &str,
    content_type: Option<&str>,
    payload: &[u8],
) -> Vec<u8> {
    let mut bytes = format!("--{BOUNDARY}\r\nContent-Disposition: {disposition}\r\n").into_bytes();
    if let Some(content_type) = content_type {
        bytes.extend_from_slice(format!("Content-Type: {content_type}\r\n").as_bytes());
    }
    bytes.extend_from_slice(b"\r\n");
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    bytes
}

fn chunked_body(bytes: Vec<u8>) -> Body {
    // Headers, CRLFs, UTF-8 sequences, payloads and delimiters can all span chunks.
    let lengths = [1, 3, 7, 31, 1021, 16_381, 65_521];
    let mut chunks = Vec::new();
    let mut offset = 0;
    let mut index = 0;
    while offset < bytes.len() {
        let end = (offset + lengths[index % lengths.len()]).min(bytes.len());
        chunks.push(Ok::<_, Infallible>(bytes[offset..end].to_vec()));
        offset = end;
        index += 1;
    }
    Body::from_stream(futures_util::stream::iter(chunks))
}

fn headers_then_body(mut bytes: Vec<u8>) -> Body {
    let header_end = bytes
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .unwrap()
        + 4;
    let remaining = bytes.split_off(header_end);
    let mut chunks = [bytes, remaining].into_iter();
    let mut pause = false;
    Body::from_stream(futures_util::stream::poll_fn(move |context| {
        // Multer drains ready frames before parsing them. Pause after headers so
        // next_field succeeds and the following frame is read by Field.chunk.
        if pause {
            pause = false;
            context.waker().wake_by_ref();
            return Poll::Pending;
        }
        let chunk = chunks.next();
        pause = chunk.is_some();
        Poll::Ready(chunk.map(Ok::<_, Infallible>))
    }))
}

fn untrusted_multipart_body(payload: Vec<u8>) -> Vec<u8> {
    let mut part = Part::file(
        "UNTRUSTED_FIELD_MARKER",
        "UNTRUSTED_FILENAME_MARKER.bin",
        payload,
    );
    part.content_type = "application/octet-stream; note=UNTRUSTED_MIME_MARKER".into();
    multipart_bytes(vec![part])
}

async fn read_body(bytes: Vec<u8>, chunked: bool) -> (StatusCode, Result<ScoreUpload, ApiError>) {
    let body = if chunked {
        chunked_body(bytes)
    } else {
        Body::from(bytes)
    };
    // Exercise this reader's limits independently of Axum's default 2 MiB limit.
    let (response, parsed) = read_request_body(body, DefaultBodyLimit::disable()).await;
    (response.status(), parsed)
}

async fn read_request_body(
    body: Body,
    body_limit: DefaultBodyLimit,
) -> (Response, Result<ScoreUpload, ApiError>) {
    let (sender, mut receiver) = oneshot::channel();
    let capture = Arc::new(Mutex::new(Some(sender)));
    let app = Router::new()
        .route(
            "/upload",
            post(move |multipart: Multipart| {
                let capture = capture.clone();
                async move {
                    let parsed = read_score_upload(multipart).await;
                    let response = match &parsed {
                        Ok(_) => StatusCode::OK.into_response(),
                        Err(error) => ApiError(error.0, error.1).into_response(),
                    };
                    let sender = capture.lock().unwrap().take().unwrap();
                    assert!(sender.send(parsed).is_ok());
                    response
                }
            }),
        )
        .layer(body_limit);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/upload")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={BOUNDARY}"),
                )
                .body(body)
                .unwrap(),
        )
        .await
        .unwrap();
    let parsed = receiver
        .try_recv()
        .expect("the Multipart extractor must pass the body to the reader");
    (response, parsed)
}

async fn upload(parts: Vec<Part>, chunked: bool) -> (StatusCode, Result<ScoreUpload, ApiError>) {
    read_body(multipart_bytes(parts), chunked).await
}

fn accepted(result: (StatusCode, Result<ScoreUpload, ApiError>)) -> ScoreUpload {
    assert_eq!(result.0, StatusCode::OK);
    result
        .1
        .unwrap_or_else(|error| panic!("unexpected multipart error: {} {}", error.0, error.1))
}

fn rejected(result: (StatusCode, Result<ScoreUpload, ApiError>), status: StatusCode, code: &str) {
    assert_eq!(result.0, status);
    match result.1 {
        Err(error) => {
            assert_eq!(error.0, status);
            assert_eq!(error.1, code);
        }
        Ok(_) => panic!("multipart reader unexpectedly accepted the body"),
    }
}

async fn rejected_request(
    result: (Response, Result<ScoreUpload, ApiError>),
    status: StatusCode,
    code: &str,
) -> String {
    let (response, parsed) = result;
    let response_status = response.status();
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
    let body = String::from_utf8(bytes.to_vec()).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({"error": code, "message": code}),
    );
    rejected((response_status, parsed), status, code);
    body
}

fn assert_untrusted_multipart_redacted(body: &str, event: &str) {
    for marker in [
        "UNTRUSTED_FIELD_MARKER",
        "UNTRUSTED_FILENAME_MARKER",
        "UNTRUSTED_MIME_MARKER",
        "UNTRUSTED_PAYLOAD_MARKER",
        "UNTRUSTED_HEADER_MARKER",
        "UNTRUSTED_STREAM_MARKER",
    ] {
        assert!(
            !body.contains(marker) && !event.contains(marker),
            "untrusted multipart values must not enter responses or diagnostics",
        );
    }
}

#[tokio::test]
async fn score_upload_valid_request_under_framework_limit() {
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    let envelope = "fixture-envelope";
    let replay = vec![0x00, 0xff, 0x81];
    let bytes = multipart_bytes(vec![
        Part::text("score", envelope.as_bytes().to_vec()),
        Part::file("score", "fixture.osr", replay.clone()),
    ]);
    assert!(bytes.len() <= FRAMEWORK_BODY_LIMIT);
    for body in [Body::from(bytes.clone()), headers_then_body(bytes)] {
        let (response, parsed) =
            read_request_body(body, DefaultBodyLimit::max(FRAMEWORK_BODY_LIMIT)).await;
        let parsed = accepted((response.status(), parsed));
        assert_eq!(parsed.envelope.as_deref(), Some(envelope));
        assert_eq!(parsed.replay.as_deref(), Some(replay.as_slice()));
        assert!(parsed.fields.is_empty());
    }
}

#[tokio::test]
async fn score_upload_keeps_envelope_and_binary_replay() {
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    let replay: Vec<u8> = (0..SCALAR_BYTES + 913)
        .map(|index| (index % 256) as u8)
        .collect();
    let envelope = "synthetic-envelope-日";
    for reverse in [false, true] {
        for chunked in [false, true] {
            let mut parts = vec![
                Part::text("score", envelope.as_bytes().to_vec()),
                Part::file("score", "fixture.osr", replay.clone()),
                Part::text("osuver", b"20261007".to_vec()),
            ];
            if reverse {
                parts.reverse();
            }
            let parsed = accepted(upload(parts, chunked).await);
            assert_eq!(parsed.envelope.as_deref(), Some(envelope));
            assert_eq!(parsed.replay.as_deref(), Some(replay.as_slice()));
            assert_eq!(
                parsed.fields.get("osuver").map(String::as_str),
                Some("20261007")
            );
            assert_eq!(parsed.fields.len(), 1);
        }
    }
}

#[tokio::test]
async fn score_upload_discards_large_ancillary_part_without_losing_score_parts() {
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    let parameter_names = [
        "osuver", "iv", "pass", "s", "bmk", "sbk", "x", "ft", "fs", "c1", "st", "i",
    ];
    let envelope = "synthetic-encrypted-envelope";
    let replay = vec![0x81; SCALAR_BYTES + 913];
    // Only the structural shape and length reflect the observed native retry:
    // 13 valid parts, then a five-byte unknown name without filename or MIME.
    // No native field name or payload is retained in this synthetic fixture.
    let ancillary: Vec<u8> = (0..80_486).map(|index| (index % 256) as u8).collect();
    assert!(String::from_utf8(ancillary.clone()).is_err());
    for reverse in [false, true] {
        for chunked in [false, true] {
            let mut parts = vec![Part::text("score", envelope.as_bytes().to_vec())];
            parts.extend(
                parameter_names
                    .map(|name| Part::text(name, format!("synthetic-{name}").into_bytes())),
            );
            assert_eq!(parts.len(), 13);
            let mut ignored = Part::text("probe", ancillary.clone());
            ignored.content_type.clear();
            parts.push(ignored);
            parts.push(Part::file("score", "synthetic.osr", replay.clone()));
            if reverse {
                parts.reverse();
            }
            let parsed = accepted(upload(parts, chunked).await);
            assert_eq!(parsed.envelope.as_deref(), Some(envelope));
            assert_eq!(parsed.replay.as_deref(), Some(replay.as_slice()));
            assert_eq!(parsed.fields.len(), parameter_names.len());
            assert!(!parsed.fields.contains_key("probe"));
            for name in parameter_names {
                assert_eq!(parsed.fields[name], format!("synthetic-{name}"));
            }
        }
    }
}

#[tokio::test]
async fn score_upload_discards_unknown_form_data_independently_of_mime_and_duplicates() {
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    for content_type in [None, Some("text/plain"), Some("application/octet-stream")] {
        let parsed = accepted(
            read_body(
                custom_disposition_body(
                    r#"FORM-DATA; NaMe="probe"; ignored="x; filename=not-a-parameter""#,
                    content_type,
                    &vec![0xff; SCALAR_BYTES + 1],
                ),
                true,
            )
            .await,
        );
        assert!(parsed.fields.is_empty());
        assert!(parsed.envelope.is_none());
        assert!(parsed.replay.is_none());
    }
    // Ignored ancillary duplicates neither occupy the retained parameter map
    // nor acquire the identity of a score envelope or replay.
    let parsed = accepted(
        upload(
            vec![
                Part::text("probe", vec![0xff; SCALAR_BYTES + 1]),
                Part::text("probe", vec![0xfe; SCALAR_BYTES + 1]),
                Part::text("pass", b"synthetic-password-hash".to_vec()),
            ],
            true,
        )
        .await,
    );
    assert_eq!(parsed.fields.len(), 1);
    assert_eq!(parsed.fields["pass"], "synthetic-password-hash");
    assert!(parsed.envelope.is_none());
    assert!(parsed.replay.is_none());
}

#[tokio::test]
async fn score_upload_ancillary_discard_keeps_ambiguous_and_critical_parts_strict() {
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    let payload = vec![0xff; SCALAR_BYTES + 1];
    for disposition in [
        r#"form-data; name="""#,
        "form-data",
        r#"form-data; name="probe"; filename="""#,
        r#"form-data; name="Score""#,
        r#"form-data; name="PASS""#,
        r#"form-data; name="OsUvEr""#,
        r#"attachment; name="probe""#,
        r#"file; name="probe""#,
        r#"unrecognized; name="probe""#,
    ] {
        rejected(
            read_body(custom_disposition_body(disposition, None, &payload), true).await,
            StatusCode::PAYLOAD_TOO_LARGE,
            "score_upload_too_large",
        );
    }
    for name in [
        "score", "osuver", "iv", "pass", "s", "bmk", "sbk", "x", "ft", "fs", "c1", "st", "i",
    ] {
        for name in [name.to_owned(), name.to_ascii_uppercase()] {
            rejected(
                upload(vec![Part::text(&name, payload.clone())], true).await,
                StatusCode::PAYLOAD_TOO_LARGE,
                "score_upload_too_large",
            );
        }
    }
    // A small unknown file is still ignored by the original file handling;
    // it must never populate the actual replay slot.
    let parsed = accepted(upload(vec![Part::file("probe", "other.bin", vec![0xff])], true).await);
    assert!(parsed.fields.is_empty());
    assert!(parsed.envelope.is_none());
    assert!(parsed.replay.is_none());
}

#[tokio::test]
async fn score_upload_discarded_ancillary_part_keeps_exact_total_boundary() {
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    for chunked in [false, true] {
        let parsed =
            accepted(upload(vec![Part::text("probe", vec![0xff; TOTAL_BYTES])], chunked).await);
        assert!(parsed.fields.is_empty());
        assert!(parsed.envelope.is_none());
        assert!(parsed.replay.is_none());
        rejected(
            upload(
                vec![Part::text("probe", vec![0xff; TOTAL_BYTES + 1])],
                chunked,
            )
            .await,
            StatusCode::PAYLOAD_TOO_LARGE,
            "score_upload_too_large",
        );
    }
}

#[tokio::test]
async fn score_upload_replay_classification_preserves_filename_semantics() {
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    let replay = vec![0xff; SCALAR_BYTES + 1];
    let parsed = accepted(upload(vec![Part::file("score", "", replay.clone())], true).await);
    assert!(parsed.envelope.is_none());
    assert_eq!(parsed.replay.as_deref(), Some(replay.as_slice()));

    let mut no_filename = Part::text("score", replay.clone());
    no_filename.content_type = "application/octet-stream".into();
    rejected(
        upload(vec![no_filename], true).await,
        StatusCode::PAYLOAD_TOO_LARGE,
        "score_upload_too_large",
    );
    rejected(
        upload(vec![Part::file("i", "fixture.png", replay)], true).await,
        StatusCode::PAYLOAD_TOO_LARGE,
        "score_upload_too_large",
    );
}

#[tokio::test]
async fn score_upload_attachment_with_lowercase_parameters_is_a_replay() {
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    let replay = vec![0xfe; 47 * 1024];
    for content_type in [None, Some("application/octet-stream")] {
        let parsed = accepted(
            read_body(
                custom_disposition_body(
                    r#"attachment; name="score"; filename="synthetic-fixture.osr""#,
                    content_type,
                    &replay,
                ),
                true,
            )
            .await,
        );
        assert_eq!(parsed.replay.as_deref(), Some(replay.as_slice()));
        assert!(parsed.envelope.is_none());
        assert!(parsed.fields.is_empty());
    }
}

#[tokio::test]
async fn score_upload_uppercase_disposition_parameters_accept_binary_replay() {
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    let replay: Vec<u8> = (0..47 * 1024).map(|index| (index % 256) as u8).collect();
    for content_type in [None, Some("text/plain"), Some("application/octet-stream")] {
        let parsed = accepted(
            read_body(
                custom_disposition_body(
                    r#"attachment; Name="score"; FileName="synthetic-fixture.osr""#,
                    content_type,
                    &replay,
                ),
                true,
            )
            .await,
        );
        assert_eq!(parsed.replay.as_deref(), Some(replay.as_slice()));
        assert!(parsed.envelope.is_none());
        assert!(parsed.fields.is_empty());
    }
    // A filename alone does not identify a score replay, even with a binary MIME.
    rejected(
        read_body(
            custom_disposition_body(
                r#"attachment; FileName="synthetic-fixture.osr""#,
                Some("application/octet-stream"),
                &replay,
            ),
            true,
        )
        .await,
        StatusCode::PAYLOAD_TOO_LARGE,
        "score_upload_too_large",
    );
}

#[tokio::test]
async fn score_upload_quoted_names_and_unknown_parameters() {
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    let parsed = accepted(
        read_body(
            custom_disposition_body(
                r#"attachment; NaMe="quoted;\"name\\value"; x="ignored; filename=not-a-parameter""#,
                None,
                b"synthetic-text",
            ),
            true,
        )
        .await,
    );
    assert_eq!(
        parsed
            .fields
            .get("quoted;\"name\\value")
            .map(String::as_str),
        Some("synthetic-text")
    );
    assert!(parsed.replay.is_none());
    assert!(parsed.envelope.is_none());

    let replay = vec![0xff; SCALAR_BYTES + 1];
    let parsed = accepted(read_body(custom_disposition_body(
        r#"attachment; ignored="value; Name=not-a-parameter"; NaMe="score"; FiLeNaMe="../synthetic; name=not-a-parameter.osr""#,
        None,
        &replay,
    ), true).await);
    assert_eq!(parsed.replay.as_deref(), Some(replay.as_slice()));
}

#[tokio::test]
async fn score_upload_invalid_disposition_parameters() {
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    for disposition in [
        r#"attachment; name="score"; Name="score""#,
        r#"attachment; NaMe="score"; NAME="other""#,
        r#"attachment; Name="score"; filename="one.osr"; FileName="one.osr""#,
        r#"attachment; Name="score"; FiLeNaMe="one.osr"; FILENAME="two.osr""#,
        "attachment; Name=\"unterminated",
        "attachment; Name=\"dangling\\",
    ] {
        rejected(
            read_body(
                custom_disposition_body(disposition, None, b"synthetic-text"),
                true,
            )
            .await,
            StatusCode::BAD_REQUEST,
            "invalid_score_disposition",
        );
    }
}

fn disposition_headers(value: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(value).unwrap(),
    );
    headers
}

fn invalid_disposition(headers: &HeaderMap) {
    let error = resolve_upload_disposition(headers)
        .err()
        .expect("invalid disposition must be rejected");
    assert_eq!(error.0, StatusCode::BAD_REQUEST);
    assert_eq!(error.1, "invalid_score_disposition");
}

#[test]
fn score_upload_disposition_resolver_boundaries_and_duplicate_headers() {
    let name = "n".repeat(128);
    let resolved = resolve_upload_disposition(&disposition_headers(&format!(
        "attachment; Name=\"{name}\""
    )))
    .unwrap();
    assert_eq!(resolved.name.as_deref(), Some(name.as_str()));
    assert!(!resolved.filename_present);
    invalid_disposition(&disposition_headers(&format!(
        "attachment; Name=\"{}\"",
        "n".repeat(129)
    )));

    let prefix = "attachment; Name=\"score\"; ignored=\"";
    for length in [8192, 8193] {
        let value = format!("{prefix}{}\"", "x".repeat(length - prefix.len() - 1));
        assert_eq!(value.len(), length);
        let headers = disposition_headers(&value);
        if length == 8192 {
            let resolved = resolve_upload_disposition(&headers).unwrap();
            assert_eq!(resolved.name.as_deref(), Some("score"));
        } else {
            invalid_disposition(&headers);
        }
    }
    let resolved =
        resolve_upload_disposition(&disposition_headers(r#"attachment; Name=""; FileName="""#))
            .unwrap();
    assert_eq!(resolved.name.as_deref(), Some(""));
    assert!(resolved.filename_present);
    let resolved = resolve_upload_disposition(&HeaderMap::new()).unwrap();
    assert!(resolved.name.is_none());
    assert!(!resolved.filename_present);

    // Multer currently collapses raw repeated headers; the resolver rejects
    // duplicates whenever its HeaderMap preserves them.
    let mut headers = disposition_headers(r#"attachment; Name="score""#);
    headers.append(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static(r#"attachment; Name="score""#),
    );
    invalid_disposition(&headers);
}

#[tokio::test]
async fn score_upload_scalar_and_replay_size_boundaries() {
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    for chunked in [false, true] {
        for name in ["osuver", "pass", "score"] {
            let parsed =
                accepted(upload(vec![Part::text(name, vec![b'v'; SCALAR_BYTES])], chunked).await);
            if name == "score" {
                assert_eq!(parsed.envelope.as_ref().unwrap().len(), SCALAR_BYTES);
                assert!(parsed.fields.is_empty());
            } else {
                assert_eq!(parsed.fields[name].len(), SCALAR_BYTES);
                assert!(parsed.envelope.is_none());
            }
            assert!(parsed.replay.is_none());
            rejected(
                upload(
                    vec![Part::text(name, vec![b'v'; SCALAR_BYTES + 1])],
                    chunked,
                )
                .await,
                StatusCode::PAYLOAD_TOO_LARGE,
                "score_upload_too_large",
            );
        }
        let parsed = accepted(
            upload(
                vec![Part::file("score", "limit.osr", vec![0x81; REPLAY_BYTES])],
                chunked,
            )
            .await,
        );
        let replay = parsed.replay.unwrap();
        assert_eq!(replay.len(), REPLAY_BYTES);
        assert_eq!(replay.first(), Some(&0x81));
        assert_eq!(replay.last(), Some(&0x81));
        drop(replay);
        rejected(
            upload(
                vec![Part::file(
                    "score",
                    "over.osr",
                    vec![0x81; REPLAY_BYTES + 1],
                )],
                chunked,
            )
            .await,
            StatusCode::PAYLOAD_TOO_LARGE,
            "score_upload_too_large",
        );
    }
}

fn total_limit_parts(over: bool) -> Vec<Part> {
    let mut parts = vec![Part::file("score", "total.osr", vec![0x82; REPLAY_BYTES])];
    let parameter_count = (TOTAL_BYTES - REPLAY_BYTES) / SCALAR_BYTES;
    assert_eq!(parameter_count, 128);
    for index in 0..parameter_count {
        parts.push(Part::text(
            &format!("parameter_{index}"),
            vec![b'p'; SCALAR_BYTES],
        ));
    }
    if over {
        parts.push(Part::text("over_total", vec![b'p']));
    }
    parts
}

#[tokio::test]
async fn score_upload_total_size_boundary() {
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    let parsed = accepted(upload(total_limit_parts(false), true).await);
    assert_eq!(parsed.replay.as_ref().unwrap().len(), REPLAY_BYTES);
    assert!(
        parsed.fields.is_empty(),
        "drained ancillary fields must not be retained"
    );
    drop(parsed);
    // The 128 drained ancillary fields still count toward the aggregate cap;
    // a final discarded byte must fail even though none is retained.
    rejected(
        upload(total_limit_parts(true), true).await,
        StatusCode::PAYLOAD_TOO_LARGE,
        "score_upload_too_large",
    );
}

#[tokio::test]
async fn score_upload_duplicate_parts_and_invalid_text() {
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    for (parts, code) in [
        (
            vec![
                Part::text("score", b"first".to_vec()),
                Part::text("score", b"second".to_vec()),
            ],
            "duplicate_score_envelope",
        ),
        (
            vec![
                Part::file("score", "first.osr", vec![0xff]),
                Part::file("score", "second.osr", vec![0xfe]),
            ],
            "duplicate_replay_field",
        ),
        (
            vec![
                Part::text("osuver", b"first".to_vec()),
                Part::text("osuver", b"second".to_vec()),
            ],
            "duplicate_score_parameter",
        ),
        (vec![Part::text("osuver", vec![0xff])], "invalid_score_text"),
        (vec![Part::text("score", vec![0xfe])], "invalid_score_text"),
    ] {
        rejected(upload(parts, true).await, StatusCode::BAD_REQUEST, code);
    }
}

#[tokio::test]
async fn score_upload_malformed_multipart() {
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    for bytes in [
        b"not a multipart body".to_vec(),
        format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"score\"\r\n").into_bytes(),
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"score\"\r\n\r\ntruncated field"
        )
        .into_bytes(),
    ] {
        rejected(
            read_body(bytes, true).await,
            StatusCode::BAD_REQUEST,
            "invalid_score_multipart",
        );
    }
}

#[derive(Clone, Default)]
struct CapturedLogs(Arc<Mutex<Vec<u8>>>);
struct LogWriter(Arc<Mutex<Vec<u8>>>);
impl Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedLogs {
    type Writer = LogWriter;
    fn make_writer(&'a self) -> Self::Writer {
        LogWriter(self.0.clone())
    }
}

fn diagnostic_capture() -> CapturedLogs {
    let captured = CapturedLogs::default();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .with_max_level(tracing::Level::WARN)
        .with_writer(captured.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("one subscriber in the isolated diagnostic process");
    captured
}

async fn diagnostic_body(
    bytes: Vec<u8>,
    captured: &CapturedLogs,
) -> ((StatusCode, Result<ScoreUpload, ApiError>), String) {
    captured.0.lock().unwrap().clear();
    let result = read_body(bytes, true).await;
    let event = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    (result, event)
}

async fn diagnostic_limited_body(
    body: Body,
    captured: &CapturedLogs,
) -> ((Response, Result<ScoreUpload, ApiError>), String) {
    captured.0.lock().unwrap().clear();
    let result = read_request_body(body, DefaultBodyLimit::max(FRAMEWORK_BODY_LIMIT)).await;
    let event = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    (result, event)
}

fn synthetic_score_envelope_fields() -> Vec<String> {
    [
        "0123456789abcdef0123456789abcdef",
        "SECRET_USERNAME_MARKER",
        "abcdef0123456789abcdef0123456789",
        "2",
        "0",
        "0",
        "0",
        "0",
        "0",
        "600",
        "2",
        "True",
        "X",
        "0",
        "True",
        "0",
        "261008120000",
        "  b20261008  ",
    ]
    .map(str::to_owned)
    .to_vec()
}

#[tokio::test]
async fn score_envelope_parse_diagnostics_preserve_status_and_redact() {
    if diagnostic_runs_in_child(
        "stable_scores::upload_tests::score_envelope_parse_diagnostics_preserve_status_and_redact",
    ) {
        return;
    }
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    let captured = diagnostic_capture();
    let valid_fields = synthetic_score_envelope_fields();
    assert_eq!(valid_fields.len(), 18);
    let accepted = parse_score_submission(&valid_fields.join(":"))
        .expect("synthetic valid envelope must retain existing parser behavior");
    assert_eq!(accepted.username, "SECRET_USERNAME_MARKER");
    assert_eq!(accepted.client_version, "b20261008");
    assert!(captured.0.lock().unwrap().is_empty());
    let extension_sets = [
        vec!["SECRET_EXTENSION_MARKER"],
        vec![
            "SECRET_EXTENSION_MARKER",
            "pass=SECRET_EXTENSION_MARKER",
            "",
            " bSECRET_TRAILING_VERSION_MARKER     ",
        ],
    ];
    for extensions in &extension_sets {
        let mut fields = valid_fields.clone();
        fields.extend(extensions.iter().map(|value| (*value).to_owned()));
        let extended = parse_score_submission(&fields.join(":"))
            .expect("opaque trailing extensions must preserve the canonical score");
        assert_eq!(extended.username, accepted.username);
        assert_eq!(extended.client_version, accepted.client_version);
        assert_eq!(extended.beatmap_md5, accepted.beatmap_md5);
        assert_eq!(extended.checksum, accepted.checksum);
        assert_eq!(extended.hit_counts, accepted.hit_counts);
        assert_eq!(extended.total_score, accepted.total_score);
        assert_eq!(extended.mods, accepted.mods);
        assert!(captured.0.lock().unwrap().is_empty());
    }

    let mut cases = Vec::new();
    for (reason, field_index, replacement) in [
        ("digest", 0, "SECRET_FIELD_MARKER"),
        ("username", 1, "SECRET_USERNAME_MARKER\n"),
        ("hit_counts", 3, "SECRET_FIELD_MARKER"),
        ("total", 9, "SECRET_FIELD_MARKER"),
        ("combo", 10, "SECRET_FIELD_MARKER"),
        ("boolean", 11, "SECRET_FIELD_MARKER"),
        ("grade", 12, "SECRET_FIELD_MARKER"),
        ("mods", 13, "SECRET_FIELD_MARKER"),
        ("mode", 15, "SECRET_FIELD_MARKER"),
        ("timestamp", 16, "SECRET_FIELD_MARKER"),
        ("client_version", 17, "      "),
    ] {
        let mut fields = valid_fields.clone();
        fields[field_index] = replacement.into();
        cases.push((reason, fields.join(":")));
    }
    // An overlong client field can contain arbitrary secret text. The reason
    // and byte lengths may be logged, but its text must remain private.
    let mut overlong_version = valid_fields.clone();
    overlong_version[17] = format!("{}  ", "SECRET_VERSION_MARKER".repeat(7));
    assert!(overlong_version[17].trim_end_matches(' ').len() > 128);
    cases.push(("client_version", overlong_version.join(":")));
    let mut wrong_count = valid_fields.clone();
    wrong_count.truncate(17);
    cases.push(("field_count", wrong_count.join(":")));
    // Trailing extensions never repair a malformed canonical prefix, and
    // diagnostic version metrics must come from field 18 rather than the tail.
    for extensions in &extension_sets {
        let mut fields = valid_fields.clone();
        fields[0] = "SECRET_FIELD_MARKER".into();
        fields.extend(extensions.iter().map(|value| (*value).to_owned()));
        cases.push(("digest", fields.join(":")));
    }
    cases.push((
        "envelope_limit",
        "SECRET_FIELD_MARKER".repeat(
            resonance_protocol::submission::MAX_ENVELOPE_BYTES / "SECRET_FIELD_MARKER".len() + 1,
        ),
    ));

    for (reason, decoded) in cases {
        captured.0.lock().unwrap().clear();
        let error = parse_score_submission(&decoded)
            .expect_err("synthetic malformed envelope must preserve rejection");
        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert_eq!(error.1, "invalid_score_envelope");
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        let body = String::from_utf8(to_bytes(response.into_body(), 1024).await.unwrap().to_vec())
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap(),
            serde_json::json!({
                "error": "invalid_score_envelope",
                "message": "invalid_score_envelope",
            }),
        );
        let event = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        assert!(event.contains("stable score envelope rejected"));
        let field_count = decoded.split(':').count();
        let mut metrics = vec![
            format!("parse_reason=\"{reason}\""),
            format!("envelope_field_count={field_count}"),
            format!("envelope_bytes={}", decoded.len()),
        ];
        if field_count >= 18 {
            let client_version = decoded.split(':').nth(17).unwrap();
            metrics.extend([
                format!("client_version_raw_bytes={}", client_version.len()),
                format!(
                    "client_version_normalized_bytes={}",
                    client_version.trim().len()
                ),
                format!(
                    "client_flag_spaces={}",
                    client_version.len() - client_version.trim_end_matches(' ').len(),
                ),
            ]);
        } else {
            // A truncated canonical prefix has no client-version field and
            // must not acquire version diagnostics from another substring.
            for key in [
                "client_version_raw_bytes=",
                "client_version_normalized_bytes=",
                "client_flag_spaces=",
            ] {
                assert!(!event.contains(key), "unexpected unclassified metric {key}");
            }
        }
        for metric in metrics {
            assert!(
                event.split_whitespace().any(|token| token == metric),
                "missing fixed diagnostic {metric}: {event}",
            );
        }
        for marker in [
            "SECRET_USERNAME_MARKER",
            "SECRET_FIELD_MARKER",
            "SECRET_VERSION_MARKER",
            "SECRET_EXTENSION_MARKER",
            "SECRET_TRAILING_VERSION_MARKER",
        ] {
            assert!(
                !event.contains(marker) && !body.contains(marker),
                "decrypted envelope text must not enter diagnostics or responses",
            );
        }
    }
}

#[tokio::test]
async fn score_upload_framework_limit_next_field_preserves_status_and_redacts() {
    if diagnostic_runs_in_child(
        "stable_scores::upload_tests::score_upload_framework_limit_next_field_preserves_status_and_redacts",
    ) {
        return;
    }
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    let captured = diagnostic_capture();
    let payload = b"UNTRUSTED_PAYLOAD_MARKER".repeat(32);
    assert!(payload.len() < SCALAR_BYTES);
    let bytes = untrusted_multipart_body(payload);
    assert!(bytes.len() > FRAMEWORK_BODY_LIMIT);
    // The single over-limit frame fails before the parser can expose a field.
    // This framework limit is independent of the reader's explicit part caps.
    let (result, event) = diagnostic_limited_body(Body::from(bytes), &captured).await;
    let body = rejected_request(
        result,
        StatusCode::PAYLOAD_TOO_LARGE,
        "score_upload_too_large",
    )
    .await;
    assert_untrusted_multipart_redacted(&body, &event);
    for expected in [
        "stable score multipart rejected",
        "field_category=\"unavailable\"",
        "parser_field_category=\"unavailable\"",
        "part_index=1",
        "part_bytes=0",
        "total_bytes=0",
        "rejection_stage=\"multipart_next_field\"",
    ] {
        assert!(
            event.contains(expected),
            "missing diagnostic {expected}: {event}"
        );
    }
}

#[tokio::test]
async fn score_upload_framework_limit_chunk_preserves_status_and_redacts() {
    if diagnostic_runs_in_child(
        "stable_scores::upload_tests::score_upload_framework_limit_chunk_preserves_status_and_redacts",
    ) {
        return;
    }
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    let captured = diagnostic_capture();
    let payload = b"UNTRUSTED_PAYLOAD_MARKER".repeat(32);
    assert!(payload.len() < SCALAR_BYTES);
    let bytes = untrusted_multipart_body(payload);
    let header_end = bytes
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .unwrap()
        + 4;
    assert!(header_end < FRAMEWORK_BODY_LIMIT);
    assert!(bytes.len() > FRAMEWORK_BODY_LIMIT);
    let (result, event) = diagnostic_limited_body(headers_then_body(bytes), &captured).await;
    let body = rejected_request(
        result,
        StatusCode::PAYLOAD_TOO_LARGE,
        "score_upload_too_large",
    )
    .await;
    assert_untrusted_multipart_redacted(&body, &event);
    for expected in [
        "stable score multipart rejected",
        "field_category=\"unknown\"",
        "parser_field_category=\"unknown\"",
        "resolved_name_present=true",
        "resolved_filename_present=true",
        "parser_name_matches_resolved=true",
        "content_type_category=\"octet-stream\"",
        "part_index=1",
        "part_bytes=0",
        "total_bytes=0",
        "part_limit=32768",
        "rejection_stage=\"multipart_chunk\"",
    ] {
        assert!(
            event.contains(expected),
            "missing diagnostic {expected}: {event}"
        );
    }
}

#[tokio::test]
async fn score_upload_malformed_request_preserves_status_and_redacts() {
    if diagnostic_runs_in_child(
        "stable_scores::upload_tests::score_upload_malformed_request_preserves_status_and_redacts",
    ) {
        return;
    }
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    let captured = diagnostic_capture();
    let malformed = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"UNTRUSTED_FIELD_MARKER\"; filename=\"UNTRUSTED_FILENAME_MARKER.bin\"\r\nContent-Type: application/octet-stream; note=UNTRUSTED_MIME_MARKER\r\nInvalid Header: UNTRUSTED_HEADER_MARKER\r\n\r\nUNTRUSTED_PAYLOAD_MARKER\r\n--{BOUNDARY}--\r\n"
    )
    .into_bytes();
    let mut truncated = untrusted_multipart_body(b"UNTRUSTED_PAYLOAD_MARKER".to_vec());
    let closing_boundary_bytes = format!("\r\n--{BOUNDARY}--\r\n").len();
    truncated.truncate(truncated.len() - closing_boundary_bytes);
    for (bytes, stage) in [
        (malformed, "multipart_next_field"),
        (truncated, "multipart_chunk"),
    ] {
        assert!(bytes.len() <= FRAMEWORK_BODY_LIMIT);
        let (result, event) = diagnostic_limited_body(Body::from(bytes), &captured).await;
        let body =
            rejected_request(result, StatusCode::BAD_REQUEST, "invalid_score_multipart").await;
        assert_untrusted_multipart_redacted(&body, &event);
        assert!(event.contains("stable score multipart rejected"));
        assert!(event.contains(&format!("rejection_stage=\"{stage}\"")));
    }
}

#[tokio::test]
async fn score_upload_stream_failure_preserves_status_and_redacts() {
    if diagnostic_runs_in_child(
        "stable_scores::upload_tests::score_upload_stream_failure_preserves_status_and_redacts",
    ) {
        return;
    }
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    let captured = diagnostic_capture();
    let body = Body::from_stream(futures_util::stream::iter([Err::<Vec<u8>, _>(
        std::io::Error::other("UNTRUSTED_STREAM_MARKER"),
    )]));
    let (result, event) = diagnostic_limited_body(body, &captured).await;
    let body = rejected_request(
        result,
        StatusCode::INTERNAL_SERVER_ERROR,
        "invalid_score_multipart",
    )
    .await;
    assert_untrusted_multipart_redacted(&body, &event);
    assert!(event.contains("rejection_stage=\"multipart_next_field\""));
}

#[tokio::test]
async fn score_upload_parser_and_resolver_diagnostics_remain_redacted() {
    if diagnostic_runs_in_child(
        "stable_scores::upload_tests::score_upload_parser_and_resolver_diagnostics_remain_redacted",
    ) {
        return;
    }
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    let captured = diagnostic_capture();
    for (disposition, parser, length, variant, names_match) in [
        (r#"form-data; name="Score""#, "unknown", 5, true, true),
        (
            r#"attachment; name="PRIVATE_FIELD_MARKER""#,
            "unknown",
            20,
            false,
            true,
        ),
        // Multer can find a name-shaped substring inside an unrelated quoted
        // parameter. The bounded resolver identifies the actual parameter.
        (
            r#"attachment; ignored="x; name=score ; extra"; name="PRIVATE_FIELD_MARKER""#,
            "score",
            20,
            false,
            false,
        ),
        // Categories and lengths alone do not identify parser disagreement.
        (
            r#"attachment; ignored="x; name=OTHER_FIELD_MARKERXX ; extra"; name="PRIVATE_FIELD_MARKER""#,
            "unknown",
            20,
            false,
            false,
        ),
    ] {
        let (result, event) = diagnostic_body(
            custom_disposition_body(disposition, None, &vec![b'x'; SCALAR_BYTES + 1]),
            &captured,
        )
        .await;
        rejected(
            result,
            StatusCode::PAYLOAD_TOO_LARGE,
            "score_upload_too_large",
        );
        assert!(event.contains("field_category=\"unknown\""));
        assert!(event.contains(&format!("parser_field_category=\"{parser}\"")));
        assert!(event.contains(&format!("resolved_name_length={length}")));
        assert!(event.contains(&format!("score_name_case_variant={variant}")));
        assert!(event.contains("resolved_name_present=true"));
        assert!(event.contains("resolved_filename_present=false"));
        assert!(event.contains(&format!("parser_name_matches_resolved={names_match}")));
        assert!(!event.contains("PRIVATE_FIELD_MARKER"));
        assert!(!event.contains("OTHER_FIELD_MARKERXX"));
        assert!(!event.contains("ignored="));
    }
}

#[tokio::test]
async fn score_upload_presence_diagnostics_distinguish_parser_and_resolver() {
    if diagnostic_runs_in_child(
        "stable_scores::upload_tests::score_upload_presence_diagnostics_distinguish_parser_and_resolver",
    ) {
        return;
    }
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    let captured = diagnostic_capture();
    for (
        disposition,
        parser_name,
        parser_filename,
        resolved_name,
        resolved_filename,
        names_match,
    ) in [
        // The resolver accepts parameter-key casing without treating the
        // case-sensitive name value as a replay field.
        (
            r#"attachment; name="Score"; FileName="""#,
            true,
            false,
            true,
            true,
            true,
        ),
        (
            r#"attachment; Name="PRIVATE_FIELD_MARKER"; filename="""#,
            false,
            true,
            true,
            true,
            false,
        ),
        // An explicitly empty name is different from an absent name.
        (
            r#"attachment; name=""; filename="""#,
            true,
            true,
            true,
            true,
            true,
        ),
        (r#"attachment; filename="""#, false, true, false, true, true),
        ("attachment", false, false, false, false, true),
        // A filename-shaped substring inside an unrelated quoted value does
        // not confer replay identity on the part.
        (
            r#"attachment; ignored="x; filename=PRIVATE_FILENAME_MARKER ; extra"; name="PRIVATE_FIELD_MARKER""#,
            true,
            true,
            true,
            false,
            true,
        ),
    ] {
        let (result, event) = diagnostic_body(
            custom_disposition_body(disposition, None, &vec![b'x'; SCALAR_BYTES + 1]),
            &captured,
        )
        .await;
        rejected(
            result,
            StatusCode::PAYLOAD_TOO_LARGE,
            "score_upload_too_large",
        );
        for expected in [
            format!("parser_name_present={parser_name}"),
            format!("parser_filename_present={parser_filename}"),
            format!("resolved_name_present={resolved_name}"),
            format!("resolved_filename_present={resolved_filename}"),
            format!("parser_name_matches_resolved={names_match}"),
            "part_limit=32768".to_owned(),
            "rejection_stage=\"part_limit\"".to_owned(),
        ] {
            assert!(
                event.contains(&expected),
                "missing diagnostic {expected}: {event}"
            );
        }
        for marker in [
            "PRIVATE_FIELD_MARKER",
            "PRIVATE_FILENAME_MARKER",
            "ignored=",
        ] {
            assert!(
                !event.contains(marker),
                "untrusted header values must remain redacted"
            );
        }
    }
}

#[tokio::test]
async fn score_upload_uppercase_disposition_rejections_remain_redacted() {
    if diagnostic_runs_in_child(
        "stable_scores::upload_tests::score_upload_uppercase_disposition_rejections_remain_redacted",
    ) {
        return;
    }
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    let captured = diagnostic_capture();
    let mut payload = b"UNTRUSTED_PAYLOAD_MARKER".repeat(4096);
    payload.truncate(47 * 1024);
    for (content_type, category) in [
        (None, "missing"),
        (Some("text/plain; note=UNTRUSTED_MIME_MARKER"), "other"),
    ] {
        let (result, event) = diagnostic_body(
            custom_disposition_body(
                r#"attachment; Name="score"; NAME="score"; FileName="private-fixture.osr""#,
                content_type,
                &payload,
            ),
            &captured,
        )
        .await;
        rejected(result, StatusCode::BAD_REQUEST, "invalid_score_disposition");
        for marker in [
            "private-fixture.osr",
            "UNTRUSTED_MIME_MARKER",
            "UNTRUSTED_PAYLOAD_MARKER",
        ] {
            assert!(
                !event.contains(marker),
                "header values and payload must remain redacted"
            );
        }
        for expected in [
            "field_category=\"unknown\"",
            "name_present=false",
            "name_length=0",
            "filename_present=false",
            "parser_name_present=false",
            "parser_filename_present=false",
            "binary_mime=false",
            "disposition_type=\"attachment\"",
            "disposition_name_parameter_ci=true",
            "disposition_filename_parameter_ci=true",
            "disposition_name_parameter_lowercase=false",
            "disposition_filename_parameter_lowercase=false",
            "part_bytes=0",
            "total_bytes=0",
            "part_limit=32768",
            "rejection_stage=\"content_disposition\"",
        ] {
            assert!(
                event.contains(expected),
                "missing diagnostic {expected}: {event}"
            );
        }
        assert!(event.contains(&format!("content_type_category=\"{category}\"")));
        for unresolved in [
            "resolved_name_present=",
            "resolved_filename_present=",
            "parser_name_matches_resolved=",
        ] {
            assert!(
                !event.contains(unresolved),
                "invalid disposition must not publish resolved booleans: {event}"
            );
        }
    }
}

#[test]
fn score_upload_disposition_metadata_uses_literal_categories_and_quoted_parameters() {
    for (value, kind) in [
        (
            Some("FORM-DATA; Name=score; FileName=fixture.osr"),
            "form-data",
        ),
        (
            Some("attachment; name=score; filename=fixture.osr"),
            "attachment",
        ),
        (Some("file; name=score"), "file"),
        (Some("UNTRUSTED_DISPOSITION_MARKER; name=score"), "other"),
        (None, "missing"),
    ] {
        let mut headers = HeaderMap::new();
        if let Some(value) = value {
            headers.insert(
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(value).unwrap(),
            );
        }
        assert_eq!(upload_disposition_metadata(&headers).kind, kind);
    }
    for value in [
        r#"attachment; filename="marker; name=FAKE_PARAM_MARKER""#,
        r#"attachment; filename="marker\"; name=FAKE_PARAM_MARKER""#,
    ] {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(value).unwrap(),
        );
        let metadata = upload_disposition_metadata(&headers);
        assert_eq!(metadata.kind, "attachment");
        assert!(metadata.filename_ci && metadata.filename_lowercase);
        assert!(
            !metadata.name_ci && !metadata.name_lowercase,
            "a fake name parameter inside a quoted filename must not be reported"
        );
    }
}

#[tokio::test]
async fn score_upload_diagnostics_exclude_untrusted_markers() {
    if diagnostic_runs_in_child(
        "stable_scores::upload_tests::score_upload_diagnostics_exclude_untrusted_markers",
    ) {
        return;
    }
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    let mut payload = b"UNTRUSTED_PAYLOAD_MARKER".repeat(2048);
    payload.truncate(SCALAR_BYTES + 1);
    let mut part = Part::file(
        "UNTRUSTED_FIELD_MARKER",
        "UNTRUSTED_FILENAME_MARKER.bin",
        payload,
    );
    part.content_type = "application/octet-stream; note=UNTRUSTED_MIME_MARKER".into();
    let captured = diagnostic_capture();
    let (result, event) = diagnostic_body(multipart_bytes(vec![part]), &captured).await;
    rejected(
        result,
        StatusCode::PAYLOAD_TOO_LARGE,
        "score_upload_too_large",
    );
    for marker in [
        "UNTRUSTED_FIELD_MARKER",
        "UNTRUSTED_FILENAME_MARKER",
        "UNTRUSTED_MIME_MARKER",
        "UNTRUSTED_PAYLOAD_MARKER",
    ] {
        assert!(
            !event.contains(marker),
            "untrusted multipart values must not enter diagnostics"
        );
    }
    for expected in [
        "stable score multipart rejected",
        "field_category=\"unknown\"",
        "name_present=true",
        "name_length=22",
        "filename_present=true",
        "parser_name_present=true",
        "parser_filename_present=true",
        "resolved_name_present=true",
        "resolved_filename_present=true",
        "parser_name_matches_resolved=true",
        "binary_mime=true",
        "content_type_category=\"octet-stream\"",
        "disposition_type=\"form-data\"",
        "disposition_name_parameter_ci=true",
        "disposition_filename_parameter_ci=true",
        "disposition_name_parameter_lowercase=true",
        "disposition_filename_parameter_lowercase=true",
        "part_index=1",
        "part_bytes=32769",
        "total_bytes=32769",
        "part_limit=32768",
        "total_limit=20971520",
        "rejection_stage=\"part_limit\"",
    ] {
        assert!(
            event.contains(expected),
            "missing safe diagnostic {expected}: {event}"
        );
    }
}
