//! Admin endpoints for overrides, uploads and candidates (fork-only, under
//! `/api/admin/louk/`, behind upstream's admin auth). Errors are upstream's
//! `{"error": "..."}` JSON. Contract: docs/api.md "Per-title overrides".

use std::sync::Arc;

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::candidates::{self, CandidateList, TMDB_THUMB};
use super::uploads::{self, MAX_UPLOAD_BYTES, UploadError};
use super::{
    OverrideKey, OverrideSource, TitleOverride, parse_kind, parse_media_type, parse_tmdb_id, store,
};
use crate::AppState;
use crate::cache::ImageType;
use crate::error::AppError;

#[derive(Deserialize, Default)]
pub struct KeyQuery {
    pub season: Option<String>,
    pub episode: Option<String>,
    pub lang: Option<String>,
}

/// An override as the API returns it.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct OverrideOut {
    pub media_type: &'static str,
    pub tmdb_id: u64,
    pub kind: &'static str,
    /// Episode stills only; null otherwise.
    pub season: Option<i32>,
    pub episode: Option<i32>,
    /// Request language it applies to; null = any.
    pub lang: Option<String>,
    /// `auto` | `tmdb` | `fanart` | `upload`.
    pub source: &'static str,
    /// TMDB file_path, fanart URL, upload sha256, or "" for `auto`.
    pub value: String,
    pub fanart_id: Option<String>,
    pub no_badges: bool,
    pub updated_at: i64,
    /// Preview of the source: a TMDB w300 / fanart preview URL (public), or
    /// the admin upload route (needs the bearer token); null for `auto`.
    pub thumb_url: Option<String>,
}

pub fn upload_url(sha256: &str) -> String {
    format!("/api/admin/louk/uploads/{sha256}")
}

pub fn thumb_url(source: &OverrideSource) -> Option<String> {
    match source {
        OverrideSource::Auto => None,
        OverrideSource::Tmdb { file_path } => Some(format!("{TMDB_THUMB}{file_path}")),
        OverrideSource::Fanart { url, .. } => Some(candidates::fanart_preview_url(url)),
        OverrideSource::Upload { sha256 } => Some(upload_url(sha256)),
    }
}

pub fn override_out(key: &OverrideKey, ov: &TitleOverride) -> OverrideOut {
    let ep = key.kind == ImageType::Episode;
    OverrideOut {
        media_type: key.media_type,
        tmdb_id: key.tmdb_id,
        kind: key.kind.label(),
        season: ep.then_some(key.season),
        episode: ep.then_some(key.episode),
        lang: Some(key.lang.clone()).filter(|l| !l.is_empty()),
        source: ov.source.name(),
        value: ov.source.value().to_owned(),
        fanart_id: ov.source.fanart_id().map(str::to_owned),
        no_badges: ov.no_badges,
        updated_at: ov.updated_at,
        thumb_url: thumb_url(&ov.source),
    }
}

fn key_from(
    media_type: &str,
    tmdb_id: &str,
    kind: &str,
    q: &KeyQuery,
) -> Result<OverrideKey, AppError> {
    OverrideKey::parse(
        media_type,
        tmdb_id,
        kind,
        q.season.as_deref(),
        q.episode.as_deref(),
        q.lang.as_deref(),
    )
}

fn json_error(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(json!({ "error": msg.into() }))).into_response()
}

// ---------------------------------------------------------------------------
// GET /api/admin/louk/overrides/{media_type}/{tmdb_id}
// ---------------------------------------------------------------------------

pub async fn list_overrides(
    State(state): State<Arc<AppState>>,
    Path((media_type, tmdb_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let media_type = parse_media_type(&media_type)?;
    let tmdb_id = parse_tmdb_id(&tmdb_id)?;
    let rows = store(&state)
        .list_title(&state.db, media_type, tmdb_id)
        .await?;
    let out: Vec<OverrideOut> = rows.iter().map(|(k, o)| override_out(k, o)).collect();
    Ok(Json(json!({ "overrides": out })))
}

// ---------------------------------------------------------------------------
// GET / PUT / DELETE /api/admin/louk/overrides/{media_type}/{tmdb_id}/{kind}
// ---------------------------------------------------------------------------

pub async fn get_override(
    State(state): State<Arc<AppState>>,
    Path((media_type, tmdb_id, kind)): Path<(String, String, String)>,
    Query(q): Query<KeyQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let key = key_from(&media_type, &tmdb_id, &kind, &q)?;
    let ov = store(&state).get_exact(&state.db, &key).await?;
    Ok(Json(
        json!({ "override": ov.map(|o| override_out(&key, &o)) }),
    ))
}

#[derive(Deserialize)]
struct PutBody {
    source: String,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    fanart_id: Option<String>,
    #[serde(default)]
    no_badges: Option<bool>,
}

pub async fn put_override(
    State(state): State<Arc<AppState>>,
    Path((media_type, tmdb_id, kind)): Path<(String, String, String)>,
    Query(q): Query<KeyQuery>,
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, AppError> {
    let key = key_from(&media_type, &tmdb_id, &kind, &q)?;
    let body: PutBody = serde_json::from_slice(&body).map_err(|e| {
        AppError::BadRequest(format!(
            "body must be JSON {{source, value, fanart_id?, no_badges?}}: {e}"
        ))
    })?;
    let source = OverrideSource::from_parts(
        &body.source,
        body.value.as_deref().unwrap_or(""),
        body.fanart_id.as_deref(),
    )
    .map_err(AppError::BadRequest)?;
    if let OverrideSource::Upload { sha256 } = &source {
        if uploads::find_upload(&state.config.db_dir, sha256)
            .await
            .is_none()
        {
            return Err(AppError::BadRequest(format!(
                "no upload with sha256 {sha256}"
            )));
        }
    }
    let ov = TitleOverride {
        source,
        no_badges: body.no_badges.unwrap_or(false),
        updated_at: chrono::Utc::now().timestamp(),
    };
    store(&state).set(&state.db, &key, &ov).await?;
    let purge = super::purge::purge_for(&state, &key).await;
    tracing::info!(
        media_type = key.media_type,
        tmdb_id = key.tmdb_id,
        kind = key.kind.label(),
        source = ov.source.name(),
        "louk override set"
    );
    Ok(Json(
        json!({ "override": override_out(&key, &ov), "purge": purge }),
    ))
}

pub async fn delete_override(
    State(state): State<Arc<AppState>>,
    Path((media_type, tmdb_id, kind)): Path<(String, String, String)>,
    Query(q): Query<KeyQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let key = key_from(&media_type, &tmdb_id, &kind, &q)?;
    let deleted = store(&state).clear(&state.db, &key).await?;
    let purge = if deleted {
        tracing::info!(
            media_type = key.media_type,
            tmdb_id = key.tmdb_id,
            kind = key.kind.label(),
            "louk override cleared"
        );
        Some(super::purge::purge_for(&state, &key).await)
    } else {
        None
    };
    Ok(Json(json!({ "deleted": deleted, "purge": purge })))
}

// ---------------------------------------------------------------------------
// POST /api/admin/louk/uploads, GET /api/admin/louk/uploads/{sha256}
// ---------------------------------------------------------------------------

fn is_length_limit(e: &axum::Error) -> bool {
    let mut src: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(e);
    while let Some(s) = src {
        if s.to_string() == "length limit exceeded" {
            return true;
        }
        src = s.source();
    }
    e.to_string().contains("length limit exceeded")
}

/// Raw-body upload: the request body IS the image file (any content type;
/// the format is sniffed). Over 15 MiB → 413 without reading further.
pub async fn upload(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let declared = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|n| n > MAX_UPLOAD_BYTES as u64) {
        return json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            UploadError::TooLarge.to_string(),
        );
    }
    let bytes = match axum::body::to_bytes(body, MAX_UPLOAD_BYTES).await {
        Ok(b) => b,
        Err(e) if is_length_limit(&e) => {
            return json_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                UploadError::TooLarge.to_string(),
            );
        }
        Err(e) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                format!("could not read the request body: {e}"),
            );
        }
    };
    let multipart = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("multipart/"));
    match uploads::save_upload(&state.config.db_dir, bytes.to_vec()).await {
        Ok(saved) => Json(json!({
            "sha256": saved.sha256,
            "format": saved.format.name(),
            "width": saved.width,
            "height": saved.height,
            "bytes": saved.bytes,
            "created": saved.created,
            "url": upload_url(&saved.sha256),
        }))
        .into_response(),
        Err(UploadError::TooLarge) => json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            UploadError::TooLarge.to_string(),
        ),
        Err(UploadError::NotAnImage) if multipart => json_error(
            StatusCode::BAD_REQUEST,
            "send the image file itself as the request body, not multipart/form-data",
        ),
        Err(UploadError::Io(e)) => {
            tracing::error!(error = %e, "louk overrides: storing an upload failed");
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
        }
        Err(e) => json_error(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

pub async fn get_upload(
    State(state): State<Arc<AppState>>,
    Path(sha256): Path<String>,
) -> Response {
    if !uploads::is_sha256_hex(&sha256) {
        return json_error(
            StatusCode::BAD_REQUEST,
            "sha256 must be 64 lowercase hex characters",
        );
    }
    let Some((path, format)) = uploads::find_upload(&state.config.db_dir, &sha256).await else {
        return json_error(
            StatusCode::NOT_FOUND,
            format!("no upload with sha256 {sha256}"),
        );
    };
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, format.content_type()),
                (
                    header::CACHE_CONTROL,
                    "private, max-age=31536000, immutable",
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "louk overrides: reading an upload failed");
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
        }
    }
}

// ---------------------------------------------------------------------------
// GET /api/admin/louk/candidates/{media_type}/{tmdb_id}/{kind}
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct CandidatesOut {
    pub media_type: &'static str,
    pub tmdb_id: u64,
    pub kind: &'static str,
    pub season: Option<i32>,
    pub episode: Option<i32>,
    /// Language the automatic pick was computed for.
    pub lang: String,
    /// The override a request in `lang` uses now (exact lang, base, then any).
    pub current: Option<OverrideOut>,
    #[serde(flatten)]
    pub list: CandidateList,
}

pub async fn list_candidates(
    State(state): State<Arc<AppState>>,
    Path((media_type, tmdb_id, kind)): Path<(String, String, String)>,
    Query(q): Query<KeyQuery>,
) -> Result<Json<CandidatesOut>, AppError> {
    let media_type = parse_media_type(&media_type)?;
    let tmdb_id_n = parse_tmdb_id(&tmdb_id)?;
    let kind_t = parse_kind(&kind)?;
    // Validate season/episode like a key; `lang` here is the request language.
    let lang = q
        .lang
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("en")
        .to_owned();
    if !super::is_lang(&lang) {
        return Err(AppError::BadRequest(
            "lang must look like en or pt-BR".into(),
        ));
    }
    let key = OverrideKey::parse(
        media_type,
        &tmdb_id,
        &kind,
        q.season.as_deref(),
        q.episode.as_deref(),
        None,
    )?;
    let current = store(&state)
        .lookup(
            &state.db,
            media_type,
            tmdb_id_n,
            kind_t,
            key.season,
            key.episode,
            &lang,
        )
        .await;
    let list = candidates::list(
        &state,
        media_type,
        tmdb_id_n,
        kind_t,
        key.season,
        key.episode,
        &lang,
        current.as_ref().map(|o| &o.source),
    )
    .await;
    // `current` names the stored row; find its key (exact lang, base, any).
    let current_out = match current {
        Some(ov) => {
            let rows = store(&state)
                .list_title(&state.db, media_type, tmdb_id_n)
                .await?;
            rows.into_iter()
                .find(|(k, o)| {
                    k.kind == kind_t
                        && k.season == key.season
                        && k.episode == key.episode
                        && *o == ov
                })
                .map(|(k, o)| override_out(&k, &o))
        }
        None => None,
    };
    let ep = kind_t == ImageType::Episode;
    Ok(Json(CandidatesOut {
        media_type,
        tmdb_id: tmdb_id_n,
        kind: kind_t.label(),
        season: ep.then_some(key.season),
        episode: ep.then_some(key.episode),
        lang,
        current: current_out,
        list,
    }))
}
