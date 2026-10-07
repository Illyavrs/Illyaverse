//! Multipart-reader tests use synthetic bodies and never reach score ingestion.
use super::{
    ScoreUpload, read_score_upload, resolve_upload_disposition, upload_disposition_metadata,
};
use crate::common::ApiError;
use axum::{
    Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart},
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    routing::post,
};
use std::{
    convert::Infallible,
    io::Write,
    process::Command,
    sync::{Arc, Mutex},
};
use tokio::sync::oneshot;
use tower::ServiceExt;

const BOUNDARY: &str = "illyaverse-stable-upload-fixture";
const SCALAR_BYTES: usize = 32 * 1024;
const REPLAY_BYTES: usize = 16 * 1024 * 1024;
const TOTAL_BYTES: usize = 20 * 1024 * 1024;

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
        bytes.extend_from_slice(
            format!("\r\nContent-Type: {}\r\n\r\n", part.content_type).as_bytes(),
        );
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

async fn read_body(bytes: Vec<u8>, chunked: bool) -> (StatusCode, Result<ScoreUpload, ApiError>) {
    let (sender, mut receiver) = oneshot::channel();
    let capture = Arc::new(Mutex::new(Some(sender)));
    let app = Router::new()
        .route(
            "/upload",
            post(move |multipart: Multipart| {
                let capture = capture.clone();
                async move {
                    let parsed = read_score_upload(multipart).await;
                    let status = match &parsed {
                        Ok(_) => StatusCode::OK,
                        Err(error) => error.0,
                    };
                    let sender = capture.lock().unwrap().take().unwrap();
                    assert!(sender.send(parsed).is_ok());
                    status
                }
            }),
        )
        // Exercise this reader's limits independently of Axum's default 2 MiB limit.
        .layer(DefaultBodyLimit::disable());
    let body = if chunked {
        chunked_body(bytes)
    } else {
        Body::from(bytes)
    };
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
    (response.status(), parsed)
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
        let parsed = accepted(
            upload(
                vec![Part::text("osuver", vec![b'v'; SCALAR_BYTES])],
                chunked,
            )
            .await,
        );
        assert_eq!(parsed.fields["osuver"].len(), SCALAR_BYTES);
        rejected(
            upload(
                vec![Part::text("osuver", vec![b'v'; SCALAR_BYTES + 1])],
                chunked,
            )
            .await,
            StatusCode::PAYLOAD_TOO_LARGE,
            "score_upload_too_large",
        );
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
    assert_eq!(parsed.fields.len(), 128);
    assert_eq!(
        parsed.fields.values().map(String::len).sum::<usize>(),
        TOTAL_BYTES - REPLAY_BYTES
    );
    drop(parsed);
    // Every part remains individually valid; the final one-byte scalar exceeds the sum limit.
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

#[tokio::test]
async fn score_upload_parser_and_resolver_diagnostics_remain_redacted() {
    if diagnostic_runs_in_child(
        "stable_scores::upload_tests::score_upload_parser_and_resolver_diagnostics_remain_redacted",
    ) {
        return;
    }
    let _guard = UPLOAD_TEST_LOCK.lock().await;
    let captured = diagnostic_capture();
    for (disposition, parser, length, variant) in [
        (r#"form-data; name="Score""#, "unknown", 5, true),
        (
            r#"form-data; name="PRIVATE_FIELD_MARKER""#,
            "unknown",
            20,
            false,
        ),
        // Multer can find a name-shaped substring inside an unrelated quoted
        // parameter. The bounded resolver identifies the actual parameter.
        (
            r#"form-data; ignored="x; name=score ; extra"; name="PRIVATE_FIELD_MARKER""#,
            "score",
            20,
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
        assert!(!event.contains("PRIVATE_FIELD_MARKER"));
        assert!(!event.contains("ignored="));
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
