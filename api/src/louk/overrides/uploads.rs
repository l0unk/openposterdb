//! Admin uploads for overrides: PNG, JPEG or WebP only (sniffed from the
//! bytes, never from a content type or file name), at most 15 MiB (enforced
//! while the body is read), at most 8000 px per side, and fully decoded under
//! decoder limits before anything is written. Files live under
//! `DB_DIR/uploads/<sha256>.<png|jpg|webp>`, inside the backed-up db dir, and
//! are only ever addressed by a 64-char lowercase hex name, so no request
//! value reaches the file system unchecked.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::{ImageFormat, ImageReader, Limits};
use sha2::{Digest, Sha256};

/// Largest accepted upload body.
pub const MAX_UPLOAD_BYTES: usize = 15 * 1024 * 1024;
/// Largest accepted width or height.
pub const MAX_SIDE: u32 = 8000;
/// Decoder allocation cap: an 8000x8000 RGBA image plus slack.
const MAX_DECODE_ALLOC: u64 = (MAX_SIDE as u64) * (MAX_SIDE as u64) * 4 + 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UploadFormat {
    Png,
    Jpeg,
    WebP,
}

impl UploadFormat {
    pub const ALL: [UploadFormat; 3] = [UploadFormat::Png, UploadFormat::Jpeg, UploadFormat::WebP];

    pub fn ext(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::WebP => "webp",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::WebP => "webp",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::WebP => "image/webp",
        }
    }

    fn image_format(self) -> ImageFormat {
        match self {
            Self::Png => ImageFormat::Png,
            Self::Jpeg => ImageFormat::Jpeg,
            Self::WebP => ImageFormat::WebP,
        }
    }
}

/// Why an upload was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum UploadError {
    /// Not PNG, JPEG or WebP by its magic bytes.
    NotAnImage,
    /// Over [`MAX_UPLOAD_BYTES`].
    TooLarge,
    /// A side over [`MAX_SIDE`].
    TooManyPixels { width: u32, height: u32 },
    /// The bytes do not decode.
    Undecodable(String),
    /// Writing the file failed.
    Io(String),
}

impl std::fmt::Display for UploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnImage => write!(f, "only PNG, JPEG and WebP images are accepted"),
            Self::TooLarge => write!(
                f,
                "uploads are limited to {} MiB",
                MAX_UPLOAD_BYTES / (1024 * 1024)
            ),
            Self::TooManyPixels { width, height } => {
                write!(
                    f,
                    "image is {width}x{height}; at most {MAX_SIDE} px per side are accepted"
                )
            }
            Self::Undecodable(e) => write!(f, "the image could not be decoded: {e}"),
            Self::Io(_) => write!(f, "the upload could not be stored"),
        }
    }
}

/// A stored upload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedUpload {
    pub sha256: String,
    pub format: UploadFormat,
    pub width: u32,
    pub height: u32,
    pub bytes: usize,
    /// False when the same bytes were already stored.
    pub created: bool,
}

/// Format by magic bytes.
pub fn sniff(bytes: &[u8]) -> Option<UploadFormat> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some(UploadFormat::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(UploadFormat::Jpeg)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(UploadFormat::WebP)
    } else {
        None
    }
}

fn limits() -> Limits {
    let mut l = Limits::default();
    l.max_image_width = Some(MAX_SIDE);
    l.max_image_height = Some(MAX_SIDE);
    l.max_alloc = Some(MAX_DECODE_ALLOC);
    l
}

/// Check size, magic bytes, header dimensions and a full decode (in that
/// order, cheapest first). CPU-bound: call from a blocking thread.
pub fn validate(bytes: &[u8]) -> Result<(UploadFormat, u32, u32), UploadError> {
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(UploadError::TooLarge);
    }
    let format = sniff(bytes).ok_or(UploadError::NotAnImage)?;
    // Header only: reject oversized images before allocating pixels.
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), format.image_format())
        .into_dimensions()
        .map_err(|e| UploadError::Undecodable(e.to_string()))?;
    if width == 0 || height == 0 {
        return Err(UploadError::Undecodable("empty image".into()));
    }
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(UploadError::TooManyPixels { width, height });
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format.image_format());
    reader.limits(limits());
    reader
        .decode()
        .map_err(|e| UploadError::Undecodable(e.to_string()))?;
    Ok((format, width, height))
}

pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn uploads_dir(db_dir: &str) -> PathBuf {
    Path::new(db_dir).join("uploads")
}

/// Path of an upload; `None` unless `sha256` is 64 lowercase hex chars.
pub fn upload_path(db_dir: &str, sha256: &str, format: UploadFormat) -> Option<PathBuf> {
    is_sha256_hex(sha256).then(|| uploads_dir(db_dir).join(format!("{sha256}.{}", format.ext())))
}

/// The stored file of an upload and its format, if it exists.
pub async fn find_upload(db_dir: &str, sha256: &str) -> Option<(PathBuf, UploadFormat)> {
    for format in UploadFormat::ALL {
        let path = upload_path(db_dir, sha256, format)?;
        if tokio::fs::metadata(&path)
            .await
            .map(|m| m.is_file())
            .unwrap_or(false)
        {
            return Some((path, format));
        }
    }
    None
}

/// Read an upload's bytes (render path: `None` when missing, logged).
pub async fn read_upload(db_dir: &str, sha256: &str) -> Option<Vec<u8>> {
    let (path, _) = find_upload(db_dir, sha256).await?;
    match tokio::fs::read(&path).await {
        Ok(b) => Some(b),
        Err(e) => {
            tracing::warn!(error = %e, sha256, "louk overrides: reading an upload failed");
            None
        }
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Validate and store an upload under `DB_DIR/uploads/` (temp file + rename,
/// so a crash never leaves a half-written file under its final name). The
/// same bytes stored twice give the same sha256 and one file.
pub async fn save_upload(db_dir: &str, bytes: Vec<u8>) -> Result<SavedUpload, UploadError> {
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(UploadError::TooLarge);
    }
    let (bytes, checked) = tokio::task::spawn_blocking(move || {
        let r = validate(&bytes);
        (bytes, r)
    })
    .await
    .map_err(|e| UploadError::Io(e.to_string()))?;
    let (format, width, height) = checked?;
    let sha256 = sha256_hex(&bytes);
    let path =
        upload_path(db_dir, &sha256, format).ok_or_else(|| UploadError::Io("bad name".into()))?;
    let len = bytes.len();
    if tokio::fs::metadata(&path)
        .await
        .map(|m| m.is_file())
        .unwrap_or(false)
    {
        return Ok(SavedUpload {
            sha256,
            format,
            width,
            height,
            bytes: len,
            created: false,
        });
    }
    let dir = uploads_dir(db_dir);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| UploadError::Io(e.to_string()))?;
    let tmp = dir.join(format!(".{sha256}.{}.tmp", rand::random::<u64>()));
    if let Err(e) = tokio::fs::write(&tmp, &bytes).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(UploadError::Io(e.to_string()));
    }
    if let Err(e) = tokio::fs::rename(&tmp, &path).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(UploadError::Io(e.to_string()));
    }
    Ok(SavedUpload {
        sha256,
        format,
        width,
        height,
        bytes: len,
        created: true,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]));
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn sniff_magic_bytes() {
        assert_eq!(sniff(&png(2, 2)), Some(UploadFormat::Png));
        assert_eq!(
            sniff(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]),
            Some(UploadFormat::Jpeg)
        );
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some(UploadFormat::WebP));
        assert_eq!(sniff(b"GIF89a......"), None);
        assert_eq!(sniff(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"), None);
        assert_eq!(sniff(b""), None);
    }

    #[test]
    fn validate_accepts_png() {
        assert_eq!(validate(&png(64, 32)), Ok((UploadFormat::Png, 64, 32)));
    }

    #[test]
    fn validate_rejects_bad_input() {
        assert_eq!(
            validate(b"hello, not an image"),
            Err(UploadError::NotAnImage)
        );
        assert_eq!(
            validate(&png(9000, 10)),
            Err(UploadError::TooManyPixels {
                width: 9000,
                height: 10
            })
        );
        assert_eq!(
            validate(&png(10, 9000)),
            Err(UploadError::TooManyPixels {
                width: 10,
                height: 9000
            })
        );
        let mut truncated = png(64, 64);
        truncated.truncate(40);
        assert!(matches!(
            validate(&truncated),
            Err(UploadError::Undecodable(_))
        ));
        assert!(matches!(
            validate(b"\x89PNG\r\n\x1a\ngarbage-garbage"),
            Err(UploadError::Undecodable(_))
        ));
        assert_eq!(
            validate(&vec![0u8; MAX_UPLOAD_BYTES + 1]),
            Err(UploadError::TooLarge)
        );
    }

    #[test]
    fn sha_names_only() {
        assert!(is_sha256_hex(&"0123456789abcdef".repeat(4)));
        for bad in [
            "",
            "abc",
            "../../etc/passwd",
            &"G".repeat(64),
            &"a".repeat(63),
            &"a".repeat(65),
        ] {
            assert!(!is_sha256_hex(bad), "{bad}");
            assert!(upload_path("/db", bad, UploadFormat::Png).is_none());
        }
        assert_eq!(
            upload_path("/db", &"a".repeat(64), UploadFormat::Jpeg).unwrap(),
            PathBuf::from(format!("/db/uploads/{}.jpg", "a".repeat(64)))
        );
    }

    #[tokio::test]
    async fn save_dedups_by_content() {
        let dir = tempfile::tempdir().unwrap();
        let db_dir = dir.path().to_str().unwrap();
        let bytes = png(33, 17);
        let first = save_upload(db_dir, bytes.clone()).await.unwrap();
        assert!(first.created);
        assert_eq!(
            (first.width, first.height, first.format),
            (33, 17, UploadFormat::Png)
        );
        assert_eq!(first.sha256, sha256_hex(&bytes));
        let second = save_upload(db_dir, bytes.clone()).await.unwrap();
        assert!(!second.created);
        assert_eq!(second.sha256, first.sha256);
        let files: Vec<_> = std::fs::read_dir(uploads_dir(db_dir)).unwrap().collect();
        assert_eq!(files.len(), 1, "one file, no temp leftovers");
        assert_eq!(read_upload(db_dir, &first.sha256).await.unwrap(), bytes);
        assert!(save_upload(db_dir, b"nope".to_vec()).await.is_err());
        assert_eq!(std::fs::read_dir(uploads_dir(db_dir)).unwrap().count(), 1);
    }
}
