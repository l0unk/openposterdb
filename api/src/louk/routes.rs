//! Fork admin routes. Merged into upstream's `admin_routes()` with one
//! `louk-fork` line, so every route here sits behind the same admin auth
//! (`require_auth` in `routes/app.rs`).

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};

use crate::louk::overrides::handlers as overrides;

use crate::AppState;
use crate::louk::handlers;

pub fn louk_admin_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/admin/louk/titles", get(handlers::list_titles))
        .route(
            "/api/admin/louk/titles/by-alias/{id_type}/{id_value}",
            get(handlers::title_by_alias),
        )
        .route(
            "/api/admin/louk/titles/{media_type}/{tmdb_id}",
            get(handlers::title_detail),
        )
        .route(
            "/api/admin/louk/titles/{media_type}/{tmdb_id}/purge",
            post(handlers::purge_title),
        )
        .route(
            "/api/admin/louk/titles/{media_type}/{tmdb_id}/refetch",
            post(handlers::refetch_title),
        )
        // Per-title overrides, uploads and candidates (louk::overrides).
        .route(
            "/api/admin/louk/overrides/{media_type}/{tmdb_id}",
            get(overrides::list_overrides),
        )
        .route(
            "/api/admin/louk/overrides/{media_type}/{tmdb_id}/{kind}",
            get(overrides::get_override)
                .put(overrides::put_override)
                .delete(overrides::delete_override),
        )
        .route("/api/admin/louk/uploads", post(overrides::upload))
        .route("/api/admin/louk/uploads/{sha256}", get(overrides::get_upload))
        .route(
            "/api/admin/louk/candidates/{media_type}/{tmdb_id}/{kind}",
            get(overrides::list_candidates),
        )
}
