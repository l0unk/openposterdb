//! Fork: `ETag` and `304 Not Modified` for image responses (`LOUK_HTTP_ETAG`,
//! on by default; `docs/louk/speed.md`).
//!
//! The 200 response stays byte-identical (body and every existing header); the
//! layer only adds a strong `ETag` and answers a matching `If-None-Match` with
//! a bodyless 304.
//!
//! The tag is the first 128 bits of the SHA-256 of the image bytes, so it
//! changes whenever the bytes do (new ratings, an override, a purge and
//! re-render, another settings suffix or backdrop cap) and is stable across
//! restarts and memory/disk hits. It is computed **once per insertion into
//! `image_mem_cache`** (the weigher hook calls [`note_insert`]), not per hit,
//! and remembered in a small side map of tags (no image bytes):
//!
//! - every image response of a cache hit, disk load or render passes through
//!   `image_mem_cache` (a hit notes its key via [`note_key`] in `check_caches`,
//!   an insert notes it in the weigher), and the request's last noted key is
//!   kept in a task-local set up by the layer;
//! - the layer serves the tag only when the response body, the current
//!   `image_mem_cache` entry for that key and the remembered tag all point at
//!   the same allocation (same pointer and length). Two live buffers cannot
//!   share an address, so the tag is always the tag of these exact bytes;
//!   anything else (a race, a path that bypasses the memory cache, an evicted
//!   tag) just gets no `ETag`, never a wrong one.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;
use bytes::Bytes;
use sha2::{Digest, Sha256};

use crate::AppState;
use crate::cache::MemCacheEntry;

/// What the side map remembers per cache key: the tag of the last value
/// inserted under it, and that value's buffer identity.
#[derive(Clone)]
struct Tag {
    etag: Arc<str>,
    ptr: usize,
    len: usize,
}

/// Cache key → tag of the value last inserted into `image_mem_cache`. Small
/// values (no bytes); bounded and expiring a little after the memory cache's
/// own TTL (1 h), so it can never outgrow it by much.
fn tags() -> &'static moka::sync::Cache<String, Tag> {
    static TAGS: OnceLock<moka::sync::Cache<String, Tag>> = OnceLock::new();
    TAGS.get_or_init(|| {
        moka::sync::Cache::builder()
            .max_capacity(50_000)
            .time_to_live(Duration::from_secs(2 * 3600))
            .build()
    })
}

/// Set once a layer is installed; until then the weigher hook does nothing,
/// so `LOUK_HTTP_ETAG=false` costs no hashing at all.
static ENABLED: AtomicBool = AtomicBool::new(false);

tokio::task_local! {
    /// The cache key of the image this request is serving (last noted wins).
    static SERVING: RefCell<Option<String>>;
}

/// Strong entity tag of `bytes`: `"` + 32 hex digits of SHA-256 + `"`.
pub fn etag_for(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(34);
    s.push('"');
    for b in &digest[..16] {
        s.push_str(&format!("{b:02x}"));
    }
    s.push('"');
    s
}

/// Weigher hook (`main.rs`, test harness): called synchronously on every
/// insert into `image_mem_cache`, in the inserting task. Remembers the tag of
/// the inserted bytes (hashing only when the buffer is new: the 60 s
/// re-inserts of the same buffer reuse it) and notes the key for the layer.
pub fn note_insert(key: &str, value: &MemCacheEntry) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let ptr = value.bytes.as_ptr() as usize;
    let len = value.bytes.len();
    let known = tags()
        .get(key)
        .is_some_and(|t| t.ptr == ptr && t.len == len);
    if !known {
        let etag: Arc<str> = etag_for(&value.bytes).into();
        tags().insert(key.to_owned(), Tag { etag, ptr, len });
    }
    note_key(key);
}

/// `check_caches` hook: this request is serving the memory/disk entry `key`.
/// A no-op outside the layer's scope (background tasks, disabled layer).
pub fn note_key(key: &str) {
    let _ = SERVING.try_with(|k| *k.borrow_mut() = Some(key.to_owned()));
}

/// The tag for `body` if it is exactly the current memory-cache value of
/// `key` and its tag is known.
async fn tag_for(state: &AppState, key: &str, body: &Bytes) -> Option<Arc<str>> {
    let tag = tags().get(key)?;
    let (ptr, len) = (body.as_ptr() as usize, body.len());
    if len == 0 || tag.ptr != ptr || tag.len != len {
        return None;
    }
    let entry = state.image_mem_cache.get(key).await?;
    (entry.bytes.as_ptr() as usize == ptr && entry.bytes.len() == len).then_some(tag.etag)
}

/// RFC 9110 §13.1.2: does an `If-None-Match` value match `etag`? Weak
/// comparison (a `W/` prefix is ignored), `*` matches any current
/// representation, lists are comma-separated quoted tags.
pub fn if_none_match_matches(header_value: &str, etag: &str) -> bool {
    let value = header_value.trim();
    if value == "*" {
        return true;
    }
    let target = etag.trim_start_matches("W/");
    value
        .split(',')
        .map(|t| t.trim().trim_start_matches("W/"))
        .any(|t| t.len() >= 2 && t.starts_with('"') && t.ends_with('"') && t == target)
}

fn if_none_match(headers: &HeaderMap) -> Option<String> {
    let values: Vec<&str> = headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect();
    (!values.is_empty()).then(|| values.join(","))
}

/// The middleware: runs the image handler inside a [`SERVING`] scope, then adds
/// the tag to a successful image response or turns it into a 304.
pub async fn etag_layer(State(state): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    let conditional = if_none_match(req.headers());
    let is_get_or_head = matches!(*req.method(), Method::GET | Method::HEAD);
    let (res, key) = SERVING
        .scope(RefCell::new(None), async {
            let res = next.run(req).await;
            (res, SERVING.with(|k| k.borrow_mut().take()))
        })
        .await;

    let is_image = res
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("image/"));
    let Some(key) = key else { return res };
    if !is_get_or_head || res.status() != StatusCode::OK || !is_image {
        return res;
    }

    let (mut parts, body) = res.into_parts();
    // The image body is one buffered `Bytes`; collecting it is O(1) (no copy).
    let Ok(bytes) = axum::body::to_bytes(body, usize::MAX).await else {
        return Response::from_parts(parts, Body::empty());
    };
    let Some(etag) = tag_for(&state, &key, &bytes).await else {
        return Response::from_parts(parts, Body::from(bytes));
    };
    let Ok(etag_value) = HeaderValue::from_str(&etag) else {
        return Response::from_parts(parts, Body::from(bytes));
    };
    parts.headers.insert(header::ETAG, etag_value);

    if conditional.is_some_and(|v| if_none_match_matches(&v, &etag)) {
        // RFC 9110 §15.4.5: same validators and caching headers as the 200,
        // no body; Content-Length, if sent, must be the 200's length.
        parts.status = StatusCode::NOT_MODIFIED;
        parts.headers.remove(header::CONTENT_TYPE);
        parts
            .headers
            .insert(header::CONTENT_LENGTH, HeaderValue::from(bytes.len()));
        return Response::from_parts(parts, Body::empty());
    }
    Response::from_parts(parts, Body::from(bytes))
}

/// `routes/app.rs` hook: wrap the image routes unless `LOUK_HTTP_ETAG=false`.
pub fn layer(
    router: axum::Router<Arc<AppState>>,
    state: &Arc<AppState>,
) -> axum::Router<Arc<AppState>> {
    if state.config.louk.http_etag_disabled {
        return router;
    }
    ENABLED.store(true, Ordering::Relaxed);
    router.layer(axum::middleware::from_fn_with_state(
        state.clone(),
        etag_layer,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etag_is_quoted_sha256_prefix() {
        let e = etag_for(b"abc");
        // sha256("abc") = ba7816bf8f01cfea414140de5dae2223...
        assert_eq!(e, "\"ba7816bf8f01cfea414140de5dae2223\"");
        assert_ne!(etag_for(b"abd"), e);
    }

    #[test]
    fn if_none_match_parsing() {
        let e = "\"0123abcd\"";
        assert!(if_none_match_matches("\"0123abcd\"", e));
        assert!(if_none_match_matches("W/\"0123abcd\"", e));
        assert!(if_none_match_matches("*", e));
        assert!(if_none_match_matches(" \"x\", \"0123abcd\" ", e));
        assert!(if_none_match_matches("\"x\",W/\"0123abcd\"", e));
        assert!(!if_none_match_matches("\"x\", \"y\"", e));
        assert!(!if_none_match_matches("0123abcd", e));
        assert!(!if_none_match_matches("", e));
        assert!(!if_none_match_matches("\"", e));
    }

    #[test]
    fn note_insert_reuses_the_tag_of_the_same_buffer() {
        ENABLED.store(true, Ordering::Relaxed);
        let bytes = Bytes::from(vec![7u8; 1000]);
        let entry = MemCacheEntry {
            bytes: bytes.clone(),
            last_checked: std::time::Instant::now(),
        };
        note_insert("test/louk-http-cache-unit", &entry);
        let first = tags().get("test/louk-http-cache-unit").unwrap();
        assert_eq!(&*first.etag, etag_for(&bytes));
        note_insert("test/louk-http-cache-unit", &entry);
        let again = tags().get("test/louk-http-cache-unit").unwrap();
        assert!(
            Arc::ptr_eq(&first.etag, &again.etag),
            "same buffer must not be re-hashed"
        );
        let other = MemCacheEntry {
            bytes: Bytes::from(vec![8u8; 1000]),
            last_checked: std::time::Instant::now(),
        };
        note_insert("test/louk-http-cache-unit", &other);
        assert_eq!(
            &*tags().get("test/louk-http-cache-unit").unwrap().etag,
            etag_for(&other.bytes)
        );
    }
}
