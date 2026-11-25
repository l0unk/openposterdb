//! Fork admin routes. Merged into upstream's `admin_routes()` with one
//! `louk-fork` line, so every route here sits behind the same admin auth
//! (`require_auth` in `routes/app.rs`).

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};

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
}
