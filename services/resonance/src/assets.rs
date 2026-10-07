//! Original stable avatar adapter. Only immutable, normalized Sanctuary uploads are read.
use crate::common::{ApiError, AppState};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use image::{ImageDecoder, ImageEncoder, ImageFormat, ImageReader};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    io::{self, Cursor, Write},
    path::{Path as FsPath, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tokio::{io::AsyncReadExt, sync::Semaphore};

const MAX_SOURCE: usize = 2 * 1024 * 1024;
const MAX_SIDE: u32 = 512;
const MAX_PNG: usize = 2 * 1024 * 1024;
const CACHE_ENTRIES: usize = 32;
static CACHE: OnceLock<Mutex<VecDeque<(PathBuf, Avatar)>>> = OnceLock::new();
static WORKERS: OnceLock<Arc<Semaphore>> = OnceLock::new();

#[derive(Clone)]
struct Avatar {
    png: Bytes,
    etag: HeaderValue,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/{id}", get(avatar))
}

fn host_matches(headers: &HeaderMap, configured: &str) -> bool {
    let domain = configured.to_ascii_lowercase();
    if domain.len() > 251
        || !domain.is_ascii()
        || domain == "ppy.sh"
        || domain.ends_with(".ppy.sh")
        || domain.parse::<std::net::IpAddr>().is_ok()
        || domain.split('.').count() < 2
        || domain.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
        || headers.get_all(header::HOST).iter().count() != 1
    {
        return false;
    }
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let hostname = match host.split_once(':') {
        Some((hostname, port)) => {
            if port.is_empty()
                || !port.bytes().all(|c| c.is_ascii_digit())
                || !matches!(port.parse::<u16>(), Ok(1..=u16::MAX))
            {
                return false;
            }
            hostname
        }
        None => host,
    };
    hostname.eq_ignore_ascii_case(&format!("a.{domain}"))
}

fn relative_avatar(url: &str) -> Result<PathBuf, ApiError> {
    let filename = url
        .strip_prefix("/media/avatar/")
        .and_then(|value| value.strip_suffix(".webp"))
        .ok_or(ApiError(StatusCode::NOT_FOUND, "avatar_not_found"))?;
    let id = uuid::Uuid::parse_str(filename)
        .map_err(|_| ApiError(StatusCode::NOT_FOUND, "avatar_not_found"))?;
    if filename != id.hyphenated().to_string() {
        return Err(ApiError(StatusCode::NOT_FOUND, "avatar_not_found"));
    }
    Ok(PathBuf::from("avatar").join(format!("{filename}.webp")))
}

async fn read_avatar(storage: &FsPath, relative: &FsPath) -> Result<Vec<u8>, ApiError> {
    let missing = || ApiError(StatusCode::NOT_FOUND, "avatar_not_found");
    let root = tokio::fs::canonicalize(storage)
        .await
        .map_err(|_| missing())?;
    let media = tokio::fs::canonicalize(root.join("media"))
        .await
        .map_err(|_| missing())?;
    if !media.starts_with(&root) {
        return Err(missing());
    }
    let resolved = tokio::fs::canonicalize(media.join(relative))
        .await
        .map_err(|_| missing())?;
    if !resolved.starts_with(&media) {
        return Err(missing());
    }
    let metadata = tokio::fs::metadata(&resolved)
        .await
        .map_err(|_| missing())?;
    if !metadata.is_file() {
        return Err(missing());
    }
    if metadata.len() > MAX_SOURCE as u64 {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE, "avatar_too_large"));
    }
    let file = tokio::fs::File::open(resolved)
        .await
        .map_err(|_| missing())?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_SOURCE as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| missing())?;
    if bytes.len() > MAX_SOURCE {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE, "avatar_too_large"));
    }
    Ok(bytes)
}

struct BoundedPng(Vec<u8>);
impl Write for BoundedPng {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_PNG.saturating_sub(self.0.len()) {
            return Err(io::Error::other("Avatar PNG exceeds output limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn normalize(bytes: &[u8]) -> Result<Avatar, ApiError> {
    let invalid = || ApiError(StatusCode::UNPROCESSABLE_ENTITY, "invalid_avatar");
    if bytes.len() > MAX_SOURCE {
        return Err(invalid());
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::WebP);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(8 * 1024 * 1024);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(|_| invalid())?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(invalid());
    }
    let rgba = image::DynamicImage::from_decoder(decoder)
        .map_err(|_| invalid())?
        .into_rgba8();
    let mut output = BoundedPng(Vec::new());
    // Fixed dimensions, fast compression, and no adaptive filter bound encoder work.
    image::codecs::png::PngEncoder::new_with_quality(
        &mut output,
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::NoFilter,
    )
    .write_image(
        rgba.as_raw(),
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|_| invalid())?;
    let etag = HeaderValue::from_str(&format!("\"{:x}\"", Sha256::digest(&output.0)))
        .map_err(|_| invalid())?;
    Ok(Avatar {
        png: Bytes::from(output.0),
        etag,
    })
}

fn cached(key: &FsPath) -> Option<Avatar> {
    let mut entries = CACHE.get_or_init(Mutex::default).lock().ok()?;
    let index = entries.iter().position(|entry| entry.0 == key)?;
    let entry = entries.remove(index)?;
    let result = entry.1.clone();
    entries.push_back(entry);
    Some(result)
}

fn remember(key: PathBuf, avatar: Avatar) {
    if let Ok(mut entries) = CACHE.get_or_init(Mutex::default).lock() {
        entries.retain(|entry| entry.0 != key);
        while entries.len() >= CACHE_ENTRIES {
            entries.pop_front();
        }
        entries.push_back((key, avatar));
    }
}

fn response(avatar: Avatar, headers: &HeaderMap) -> Response {
    let not_modified = headers.get_all(header::IF_NONE_MATCH).iter().any(|value| {
        value.to_str().is_ok_and(|value| {
            value.len() <= 4096
                && value.split(',').any(|tag| {
                    let tag = tag.trim();
                    tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == avatar.etag
                })
        })
    });
    let mut response = if not_modified {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        Body::from(avatar.png).into_response()
    };
    response.headers_mut().insert(header::ETAG, avatar.etag);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=60, must-revalidate"),
    );
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

async fn avatar(
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let configured = std::env::var("ILLYA_DOMAIN").unwrap_or_default();
    if !host_matches(&headers, &configured)
        || raw_id.len() > 19
        || !raw_id.bytes().all(|c| c.is_ascii_digit())
    {
        return Err(ApiError(StatusCode::NOT_FOUND, "avatar_not_found"));
    }
    let id = raw_id
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or(ApiError(StatusCode::NOT_FOUND, "avatar_not_found"))?;
    // Check account visibility before cache access; restricting an account takes effect immediately.
    let url = sqlx::query_scalar::<_, Option<String>>(
        "SELECT avatar_url FROM users WHERE id=$1 AND NOT is_restricted",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .flatten()
    .ok_or(ApiError(StatusCode::NOT_FOUND, "avatar_not_found"))?;
    let relative = relative_avatar(&url)?;
    let key = state.storage.join("media").join(&relative);
    if let Some(avatar) = cached(&key) {
        return Ok(response(avatar, &headers));
    }
    let permit = WORKERS
        .get_or_init(|| Arc::new(Semaphore::new(2)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "avatar_processor_busy"))?;
    let bytes = read_avatar(&state.storage, &relative).await?;
    let job = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        normalize(&bytes)
    });
    // This bounds HTTP wait. A timed-out worker keeps its permit until it actually exits.
    let avatar = tokio::time::timeout(Duration::from_secs(2), job)
        .await
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "avatar_processor_timeout"))?
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "avatar_processor_failed"))??;
    // Sanctuary assigns a fresh UUID per upload, so derivative cache keys are immutable.
    remember(key, avatar.clone());
    Ok(response(avatar, &headers))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(host: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_str(host).unwrap());
        headers
    }

    fn webp(width: u32, height: u32) -> Vec<u8> {
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            width,
            height,
            image::Rgba([17, 22, 35, 180]),
        ));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::WebP).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn host_requires_exact_configured_avatar_domain() {
        for host in ["a.illya.test", "A.ILLYA.TEST:443", "a.illya.test:8081"] {
            assert!(host_matches(&headers(host), "illya.test"));
        }
        for host in [
            "localhost",
            "illya.test",
            "a.ppy.sh",
            "a.illya.test.attacker.test",
            "evil@a.illya.test",
            "a.illya.test:0",
            "a.illya.test:65536",
            "a.illya.test:",
        ] {
            assert!(!host_matches(&headers(host), "illya.test"));
        }
        for configured in [
            "",
            "https://illya.test",
            "ppy.sh",
            "osu.ppy.sh",
            "127.0.0.1",
        ] {
            assert!(!host_matches(&headers("a.illya.test"), configured));
        }
        let mut duplicated = headers("a.illya.test");
        duplicated.append(header::HOST, HeaderValue::from_static("a.illya.test"));
        assert!(!host_matches(&duplicated, "illya.test"));
    }

    #[test]
    fn only_generated_avatar_paths_are_accepted() {
        let id = uuid::Uuid::new_v4();
        assert_eq!(
            relative_avatar(&format!("/media/avatar/{id}.webp")).unwrap(),
            PathBuf::from("avatar").join(format!("{id}.webp")),
        );
        for url in [
            "/media/avatar/../../secret.webp",
            "/media/avatar/%2e%2e.webp",
            "/media/banner/file.webp",
            "https://evil.test/avatar.webp",
            "/etc/passwd",
            "/media/avatar/not-a-uuid.webp",
            "/media/avatar/00000000-0000-0000-0000-000000000000.webp?x=1",
        ] {
            assert!(relative_avatar(url).is_err());
        }
    }

    #[tokio::test]
    async fn normalized_webp_becomes_png_with_same_pixels_and_conditional_etag() {
        let avatar = normalize(&webp(24, 12)).unwrap();
        assert_eq!(&avatar.png[..8], b"\x89PNG\r\n\x1a\n");
        let decoded = image::load_from_memory_with_format(&avatar.png, ImageFormat::Png)
            .unwrap()
            .into_rgba8();
        assert_eq!(decoded.dimensions(), (24, 12));
        assert_eq!(*decoded.get_pixel(0, 0), image::Rgba([17, 22, 35, 180]));
        let mut conditional = HeaderMap::new();
        conditional.insert(header::IF_NONE_MATCH, avatar.etag.clone());
        let cached = response(avatar.clone(), &conditional);
        assert_eq!(cached.status(), StatusCode::NOT_MODIFIED);
        assert!(
            axum::body::to_bytes(cached.into_body(), MAX_PNG)
                .await
                .unwrap()
                .is_empty()
        );
        let fresh = response(avatar, &HeaderMap::new());
        assert_eq!(fresh.status(), StatusCode::OK);
        assert_eq!(fresh.headers()[header::CONTENT_TYPE], "image/png");
    }

    #[test]
    fn corrupt_oversized_and_non_webp_inputs_are_rejected() {
        assert!(normalize(b"not an image").is_err());
        assert!(normalize(&vec![0; MAX_SOURCE + 1]).is_err());
        assert!(normalize(&webp(MAX_SIDE + 1, 1)).is_err());
        assert!(normalize(&webp(1, MAX_SIDE + 1)).is_err());
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(1, 1)
            .write_to(&mut png, ImageFormat::Png)
            .unwrap();
        assert!(normalize(png.get_ref()).is_err());
        let mut bounded = BoundedPng(vec![0; MAX_PNG]);
        assert!(bounded.write_all(b"x").is_err());
    }

    #[tokio::test]
    async fn file_reads_are_bounded_and_contained() {
        let root = std::env::temp_dir().join(format!("illya-avatar-{}", uuid::Uuid::new_v4()));
        let avatar = root.join("media/avatar");
        tokio::fs::create_dir_all(&avatar).await.unwrap();
        let file = avatar.join("fixture.webp");
        tokio::fs::write(&file, b"fixture").await.unwrap();
        assert_eq!(
            read_avatar(&root, FsPath::new("avatar/fixture.webp"))
                .await
                .unwrap(),
            b"fixture"
        );
        tokio::fs::write(&file, vec![0; MAX_SOURCE + 1])
            .await
            .unwrap();
        assert_eq!(
            read_avatar(&root, FsPath::new("avatar/fixture.webp"))
                .await
                .unwrap_err()
                .0,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert!(read_avatar(&root, FsPath::new("avatar")).await.is_err());
        tokio::fs::write(root.join("private.webp"), b"secret")
            .await
            .unwrap();
        assert!(
            read_avatar(&root, FsPath::new("../private.webp"))
                .await
                .is_err()
        );
        #[cfg(unix)]
        {
            let link = avatar.join("linked.webp");
            std::os::unix::fs::symlink(root.join("private.webp"), &link).unwrap();
            assert!(
                read_avatar(&root, FsPath::new("avatar/linked.webp"))
                    .await
                    .is_err()
            );
            tokio::fs::remove_file(link).await.unwrap();
        }
        tokio::fs::remove_file(file).await.unwrap();
        tokio::fs::remove_file(root.join("private.webp"))
            .await
            .unwrap();
        tokio::fs::remove_dir(avatar).await.unwrap();
        tokio::fs::remove_dir(root.join("media")).await.unwrap();
        tokio::fs::remove_dir(root).await.unwrap();
    }
}
