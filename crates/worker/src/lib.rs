mod admin_ui;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response as AxumResponse},
    routing::{get, post, put},
    Json, Router,
};
use catalogged_core::{slugify, tmdb, Entry, Kind, NewEntry, MAX_COVER_BYTES};
use maud::{html, Markup, DOCTYPE};
use serde::Deserialize;
use tower_service::Service;
use worker::wasm_bindgen::JsValue;
use worker::*;

#[derive(Clone)]
struct AppState {
    env: Env,
}

// Workers are single-threaded; axum requires Send + Sync.
unsafe impl Send for AppState {}
unsafe impl Sync for AppState {}

type ApiErr = (StatusCode, String);

/// Public pages and covers are cached at the edge for this long (keeps D1/R2 reads near zero).
const PUBLIC_CACHE: &str = "public, max-age=300";
const COVER_CACHE: &str = "public, max-age=86400";

fn router(env: Env) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/covers/{kind}/{file}", get(serve_cover))
        .route("/healthz", get(|| async { "ok" }))
        // Everything under /admin is gated by Cloudflare Access (see docs/plan.md), and
        // additionally by the ADMIN_ENABLED kill switch.
        .route("/admin", get(admin_page))
        .route("/admin/", get(admin_page))
        .route("/admin/api/search", get(search))
        .route("/admin/api/entries", post(create_entry))
        .route("/admin/api/entries/{kind}/{slug}/cover", put(put_cover))
        .with_state(AppState { env })
}

#[event(fetch)]
async fn fetch(req: HttpRequest, env: Env, _ctx: Context) -> Result<http::Response<worker::Body>> {
    console_error_panic_hook::set_once();
    let cacheable = req.method() == http::Method::GET && !req.uri().path().starts_with("/admin");
    let key = req.uri().to_string();
    let cache = Cache::default();
    if cacheable {
        if let Some(hit) = cache.get(&key, false).await? {
            return Ok(hit.try_into()?);
        }
    }
    let res = router(env).call(req).await.map_err(|e| Error::RustError(e.to_string()))?;
    let (parts, body) = res.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .map_err(|e| Error::RustError(e.to_string()))?;
    let mut out = Response::from_bytes(bytes.to_vec())?.with_status(parts.status.as_u16());
    for (name, value) in parts.headers.iter() {
        if let Ok(v) = value.to_str() {
            out.headers_mut().set(name.as_str(), v)?;
        }
    }
    if cacheable && parts.status == StatusCode::OK && parts.headers.contains_key(header::CACHE_CONTROL) {
        cache.put(&key, out.cloned()?).await?;
    }
    Ok(out.try_into()?)
}

fn admin_guard(env: &Env) -> Result<(), ApiErr> {
    // Kill switch: admin stays off unless ADMIN_ENABLED=1 is set.
    match env.var("ADMIN_ENABLED").map(|v| v.to_string()).ok().as_deref() {
        Some("1") => Ok(()),
        _ => Err((StatusCode::SERVICE_UNAVAILABLE, "admin disabled".into())),
    }
}

fn internal(e: impl ToString) -> ApiErr {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn layout(title: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) }
                style { (maud::PreEscaped("body{font:16px system-ui;max-width:60rem;margin:2rem auto;padding:0 1rem}ul.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(9rem,1fr));gap:1rem;padding:0;list-style:none}.grid img{width:100%;aspect-ratio:2/3;object-fit:cover;background:#ddd}.grid li{font-size:.85rem}")) }
            }
            body { (body) }
        }
    }
}

async fn load_entries(env: &Env) -> Result<Vec<Entry>> {
    let db = env.d1("DB")?;
    db.prepare(
        "SELECT id, kind, slug, title, status, rating, notes, year, source, external_id, cover_key, consumed_at \
         FROM entries ORDER BY updated_at DESC LIMIT 200",
    )
    .all()
    .await?
    .results::<Entry>()
}

#[worker::send]
async fn home(State(state): State<AppState>) -> AxumResponse {
    let entries = match load_entries(&state.env).await {
        Ok(e) => e,
        Err(e) => return internal(e).into_response(),
    };
    let page = layout(
        "catalogged",
        html! {
            h1 { "catalogged" }
            ul.grid {
                @for e in &entries {
                    li {
                        @if let Some(key) = &e.cover_key { img src={"/covers/" (key)} alt="" loading="lazy" width="300" height="450"; }
                        div { (e.title) @if let Some(y) = e.year { " (" (y) ")" } }
                        small { (e.kind.as_str()) " · " (e.status.as_str()) }
                    }
                }
            }
        },
    );
    ([(header::CACHE_CONTROL, PUBLIC_CACHE)], Html(page.into_string())).into_response()
}

#[worker::send]
async fn serve_cover(State(state): State<AppState>, Path((kind, file)): Path<(String, String)>) -> AxumResponse {
    let valid = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.');
    if !valid(&kind) || !valid(&file) || !file.ends_with(".webp") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let fetch = async {
        let bucket = state.env.bucket("COVERS")?;
        let Some(obj) = bucket.get(format!("{kind}/{file}")).execute().await? else { return Ok(None) };
        let Some(body) = obj.body() else { return Ok(None) };
        Ok::<_, Error>(Some(body.bytes().await?))
    };
    match fetch.await {
        Ok(Some(bytes)) => (
            [(header::CONTENT_TYPE, "image/webp"), (header::CACHE_CONTROL, COVER_CACHE)],
            bytes,
        )
            .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => internal(e).into_response(),
    }
}

#[worker::send]
async fn admin_page(State(state): State<AppState>) -> Result<Html<String>, ApiErr> {
    admin_guard(&state.env)?;
    let entries = load_entries(&state.env).await.map_err(internal)?;
    Ok(Html(admin_ui::page(&entries).into_string()))
}

#[derive(Deserialize)]
struct SearchQuery {
    kind: Kind,
    q: String,
}

async fn tmdb_get(env: &Env, url: String) -> Result<String> {
    let key = env.secret("TMDB_API_KEY")?.to_string();
    let (url, auth) = tmdb::authenticate(url, &key);
    let headers = Headers::new();
    headers.set("Accept", "application/json")?;
    if let Some(a) = auth {
        headers.set("Authorization", &a)?;
    }
    let mut init = RequestInit::new();
    init.with_headers(headers);
    let mut res = Fetch::Request(Request::new_with_init(&url, &init)?).send().await?;
    if res.status_code() != 200 {
        return Err(Error::RustError(format!("TMDB returned {}", res.status_code())));
    }
    res.text().await
}

#[worker::send]
async fn search(
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Vec<tmdb::Candidate>>, ApiErr> {
    admin_guard(&state.env)?;
    let url = tmdb::search_url(q.kind, q.q.trim())
        .ok_or((StatusCode::BAD_REQUEST, "kind not supported by TMDB".to_string()))?;
    let body = tmdb_get(&state.env, url).await.map_err(internal)?;
    let mut hits = tmdb::parse_search(&body).map_err(internal)?;
    hits.truncate(10);
    Ok(Json(hits))
}

fn opt<T: Into<JsValue>>(v: Option<T>) -> JsValue {
    v.map(Into::into).unwrap_or(JsValue::NULL)
}

#[worker::send]
async fn create_entry(
    State(state): State<AppState>,
    Json(new): Json<NewEntry>,
) -> Result<Json<serde_json::Value>, ApiErr> {
    admin_guard(&state.env)?;
    let db = state.env.d1("DB").map_err(internal)?;
    let slug = slugify(&new.title);
    if slug.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "title produces an empty slug".into()));
    }
    let source = new
        .source
        .and_then(|s| serde_json::to_value(s).ok())
        .and_then(|v| v.as_str().map(String::from));
    db.prepare(
        "INSERT INTO entries (kind, slug, title, status, rating, notes, year, source, external_id) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )
    .bind(&[
        new.kind.as_str().into(),
        slug.clone().into(),
        new.title.into(),
        new.status.as_str().into(),
        opt(new.rating.map(f64::from)),
        opt(new.notes),
        opt(new.year.map(f64::from)),
        opt(source),
        opt(new.external_id),
    ])
    .map_err(internal)?
    .run()
    .await
    // A UNIQUE violation (duplicate slug / external id) surfaces here.
    .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(serde_json::json!({ "slug": slug })))
}

fn is_webp(b: &[u8]) -> bool {
    b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP"
}

#[worker::send]
async fn put_cover(
    State(state): State<AppState>,
    Path((kind, slug)): Path<(String, String)>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiErr> {
    admin_guard(&state.env)?;
    // Free-tier guard: bounded size, and only real WebP (see docs/free-tier.md).
    if body.len() > MAX_COVER_BYTES {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, format!("cover exceeds {MAX_COVER_BYTES} bytes")));
    }
    if !is_webp(&body) {
        return Err((StatusCode::UNSUPPORTED_MEDIA_TYPE, "cover must be image/webp".into()));
    }
    let valid_kind = serde_json::from_value::<Kind>(serde_json::Value::String(kind.clone())).is_ok();
    if !valid_kind || slug.is_empty() || slugify(&slug) != slug {
        return Err((StatusCode::BAD_REQUEST, "bad kind or slug".into()));
    }
    let db = state.env.d1("DB").map_err(internal)?;
    let exists = db
        .prepare("SELECT 1 AS x FROM entries WHERE kind = ?1 AND slug = ?2")
        .bind(&[kind.clone().into(), slug.clone().into()])
        .map_err(internal)?
        .first::<serde_json::Value>(None)
        .await
        .map_err(internal)?;
    if exists.is_none() {
        return Err((StatusCode::NOT_FOUND, "no such entry".into()));
    }
    let key = format!("{kind}/{slug}.webp");
    state
        .env
        .bucket("COVERS")
        .map_err(internal)?
        .put(&key, body.to_vec())
        .http_metadata(HttpMetadata {
            content_type: Some("image/webp".into()),
            cache_control: Some(COVER_CACHE.into()),
            ..Default::default()
        })
        .execute()
        .await
        .map_err(internal)?;
    db.prepare("UPDATE entries SET cover_key = ?1, updated_at = datetime('now') WHERE kind = ?2 AND slug = ?3")
        .bind(&[key.clone().into(), kind.into(), slug.into()])
        .map_err(internal)?
        .run()
        .await
        .map_err(internal)?;
    Ok(Json(serde_json::json!({ "cover_key": key })))
}
