mod admin_ui;
mod public_ui;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response as AxumResponse},
    routing::{get, post, put},
    Json, Router,
};
use catalogged_core::{igdb, openlibrary, slugify, tmdb, EditEntry, Entry, Kind, NewEntry, MAX_COVER_BYTES};
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
/// Cover keys contain a content hash, so a changed cover always gets a new URL.
const COVER_CACHE: &str = "public, max-age=31536000, immutable";

/// Where a cover is fetched from. With `COVERS_BASE_URL` set (the public R2 domain) images skip the
/// Worker entirely, so they do not count against the 100k/day Worker request cap. Otherwise the
/// Worker serves them from `/covers/...` (local dev, or before the domain exists).
fn cover_src(env: &Env, key: &str) -> String {
    match env.var("COVERS_BASE_URL").map(|v| v.to_string()).ok().filter(|b| !b.is_empty()) {
        Some(base) => format!("{}/{key}", base.trim_end_matches('/')),
        None => format!("/covers/{key}"),
    }
}

fn router(env: Env) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/index.json", get(index_json))
        .route("/favicon.svg", get(|| async { ([(header::CONTENT_TYPE, "image/svg+xml"), (header::CACHE_CONTROL, "public, max-age=86400")], public_ui::FAVICON_SVG) }))
        .route("/manifest.webmanifest", get(|| async { ([(header::CONTENT_TYPE, "application/manifest+json"), (header::CACHE_CONTROL, "public, max-age=86400")], public_ui::MANIFEST) }))
        .route("/entry/{kind}/{slug}", get(entry_page))
        .route("/covers/{kind}/{file}", get(serve_cover))
        .route("/healthz", get(|| async { "ok" }))
        // Everything under /admin is gated by Cloudflare Access (see docs/plan.md), and
        // additionally by the ADMIN_ENABLED kill switch.
        .route("/admin", get(admin_page))
        .route("/admin/", get(admin_page))
        .route("/admin/edit/{kind}/{slug}", get(admin_edit_page))
        .route("/admin/api/search", get(search))
        .route("/admin/api/lookup", get(lookup))
        .route("/admin/api/entries", post(create_entry))
        .route(
            "/admin/api/entries/{kind}/{slug}",
            get(get_entry).put(edit_entry).delete(delete_entry),
        )
        .route("/admin/api/entries/{kind}/{slug}/cover", put(put_cover))
        .with_state(AppState { env })
}

#[event(fetch)]
async fn fetch(req: HttpRequest, env: Env, _ctx: Context) -> Result<http::Response<worker::Body>> {
    console_error_panic_hook::set_once();
    // DISABLE_CACHE=1 is for the e2e tests only (so writes show up immediately); never set it in production.
    let cache_off = env.var("DISABLE_CACHE").map(|v| v.to_string()).ok().as_deref() == Some("1");
    let cacheable = !cache_off && req.method() == http::Method::GET && !req.uri().path().starts_with("/admin");
    let key = cache_key(&req.uri().to_string());
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

/// Only the query params the pages actually read may vary the cache key; otherwise anyone could
/// append `?x=1`, `?x=2`... to bypass the cache and burn D1 reads.
fn cache_key(uri: &str) -> String {
    let Ok(mut url) = Url::parse(uri) else { return uri.to_string() };
    let mut keep: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k == "kind" || k == "page")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    keep.sort();
    url.set_query(None);
    if !keep.is_empty() {
        url.query_pairs_mut().extend_pairs(keep);
    }
    url.to_string()
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
                script { (maud::PreEscaped(public_ui::THEME_INIT)) }
                style { (maud::PreEscaped(public_ui::CSS)) }
                (maud::PreEscaped(public_ui::FAVICON_LINKS))
            }
            body { (body) script { (maud::PreEscaped(public_ui::THEME_JS)) } }
        }
    }
}


const COLS: &str =
    "id, kind, slug, title, status, rating, review, year, source, external_id, cover_key, consumed_at";
const PAGE_SIZE: u32 = 60;
const MAX_PAGE: u32 = 200;

/// One page of entries, newest first. Fetches one extra row to know whether a next page exists
/// (no COUNT(*): D1 bills rows scanned). `q` filters by title (admin only; scans the table).
async fn load_entries(
    env: &Env,
    kind: Option<Kind>,
    q: Option<&str>,
    page: u32,
) -> Result<(Vec<Entry>, bool)> {
    let db = env.d1("DB")?;
    let offset = f64::from(page.saturating_sub(1).min(MAX_PAGE) * PAGE_SIZE);
    let limit = f64::from(PAGE_SIZE + 1);
    let like = q.filter(|q| !q.is_empty()).map(|q| format!("%{}%", q.replace(['%', '_'], "")));
    let stmt = match (kind, &like) {
        (Some(k), None) => db
            .prepare(format!("SELECT {COLS} FROM entries WHERE kind = ?1 ORDER BY updated_at DESC LIMIT ?2 OFFSET ?3"))
            .bind(&[k.as_str().into(), limit.into(), offset.into()])?,
        (None, None) => db
            .prepare(format!("SELECT {COLS} FROM entries ORDER BY updated_at DESC LIMIT ?1 OFFSET ?2"))
            .bind(&[limit.into(), offset.into()])?,
        (_, Some(l)) => db
            .prepare(format!("SELECT {COLS} FROM entries WHERE title LIKE ?1 ORDER BY updated_at DESC LIMIT ?2 OFFSET ?3"))
            .bind(&[l.clone().into(), limit.into(), offset.into()])?,
    };
    let mut rows = stmt.all().await?.results::<Entry>()?;
    let more = rows.len() > PAGE_SIZE as usize;
    rows.truncate(PAGE_SIZE as usize);
    Ok((rows, more))
}

async fn find_entry(env: &Env, kind: &str, slug: &str) -> Result<Option<Entry>> {
    env.d1("DB")?
        .prepare(format!("SELECT {COLS} FROM entries WHERE kind = ?1 AND slug = ?2"))
        .bind(&[kind.into(), slug.into()])?
        .first::<Entry>(None)
        .await
}

#[derive(Deserialize, Default)]
struct ListQuery {
    kind: Option<Kind>,
    page: Option<u32>,
    q: Option<String>,
}

/// Compact JSON of every entry for client-side search/filter/sort. SQLite builds the string so the
/// Worker never decodes thousands of rows (10 ms CPU limit). Cached for 15 min: a cache miss reads
/// the whole table once, so this bounds the D1 read cost (~5k rows per miss).
#[worker::send]
async fn index_json(State(state): State<AppState>) -> AxumResponse {
    let q = "SELECT json_group_array(json(a)) AS j FROM (SELECT json_array(kind, slug, title, status, rating, year, cover_key, consumed_at, updated_at) AS a FROM entries ORDER BY updated_at DESC)";
    let row = async {
        state.env.d1("DB")?.prepare(q).first::<serde_json::Value>(None).await
    }
    .await;
    let items = match row {
        Ok(Some(v)) => v.get("j").and_then(|j| j.as_str()).unwrap_or("[]").to_owned(),
        Ok(None) => "[]".into(),
        Err(e) => return internal(e).into_response(),
    };
    let body = format!(r#"{{"covers":{},"items":{items}}}"#, serde_json::json!(cover_src(&state.env, "")));
    (
        [(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "public, max-age=900")],
        body,
    )
        .into_response()
}

#[worker::send]
async fn home(State(state): State<AppState>, Query(lq): Query<ListQuery>) -> AxumResponse {
    let page = lq.page.unwrap_or(1).clamp(1, MAX_PAGE);
    let (entries, more) = match load_entries(&state.env, lq.kind, None, page).await {
        Ok(r) => r,
        Err(e) => return internal(e).into_response(),
    };
    let link = |k: Option<Kind>, p: u32| {
        let mut u = format!("/?page={p}");
        if let Some(k) = k {
            u.push_str(&format!("&kind={}", k.as_str()));
        }
        u
    };
    let page_html = layout(
        "catalogged",
        html! {
            header.top { h1 { "catalogged" } (public_ui::theme_picker()) }
            form #controls hidden {
                input type="search" name="q" placeholder="Search titles" aria-label="Search titles" autocomplete="off";
                select name="kind" aria-label="Kind" {
                    option value="" { "All kinds" }
                    @for k in [Kind::Movie, Kind::Show, Kind::Game, Kind::Book] { option value=(k.as_str()) { (k.as_str()) "s" } }
                }
                select name="status" aria-label="Status" {
                    option value="" { "Any status" }
                    option value="consumed" { "Consumed" }
                    option value="in_progress" { "In progress" }
                    option value="wanted" { "Wanted" }
                }
                select name="sort" aria-label="Sort" {
                    option value="recent" { "Recently updated" }
                    option value="logged" { "Date logged" }
                    option value="title" { "Title" }
                    option value="year" { "Year" }
                    option value="rating" { "Rating" }
                }
            }
            p #count {}
            nav #nav {
                a href="/" { "all" }
                @for k in [Kind::Movie, Kind::Show, Kind::Game, Kind::Book] { " · " a href={"/?kind=" (k.as_str())} { (k.as_str()) "s" } }
            }
            ul.grid #grid {
                @for e in &entries {
                    li {
                        a href={"/entry/" (e.kind.as_str()) "/" (e.slug)} {
                            @if let Some(key) = &e.cover_key { img src=(cover_src(&state.env, key)) alt="" loading="lazy" width="300" height="450"; }
                            div { (e.title) @if let Some(y) = e.year { " (" (y) ")" } }
                        }
                        small { (e.kind.as_str()) " · " (e.status.as_str()) @if let Some(r) = e.rating { " · " (r) "/10" } }
                    }
                }
            }
            p #pager {
                @if page > 1 { a href=(link(lq.kind, page - 1)) { "← newer" } }
                @if page > 1 && more { " · " }
                @if more { a href=(link(lq.kind, page + 1)) { "older →" } }
            }
            script { (maud::PreEscaped(public_ui::GRID_JS)) }
        },
    );
    ([(header::CACHE_CONTROL, PUBLIC_CACHE)], Html(page_html.into_string())).into_response()
}

#[worker::send]
async fn entry_page(State(state): State<AppState>, Path((kind, slug)): Path<(String, String)>) -> AxumResponse {
    let e_env = state.env.clone();
    let e = match find_entry(&state.env, &kind, &slug).await {
        Ok(Some(e)) => e,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return internal(e).into_response(),
    };
    let page_html = layout(
        &e.title,
        html! {
            header.top { p { a href="/" { "← all" } } (public_ui::theme_picker()) }
            h1 { (e.title) @if let Some(y) = e.year { " (" (y) ")" } }
            p { small { a href={"/admin/edit/" (e.kind.as_str()) "/" (e.slug)} { "Edit" } } }
            @if let Some(key) = &e.cover_key { img.cover src=(cover_src(&e_env, key)) alt="" width="300" height="450"; }
            p {
                (e.kind.as_str()) " · " (e.status.as_str())
                @if let Some(r) = e.rating { " · " (r) "/10" }
                @if let Some(d) = &e.consumed_at { " · " (d) }
            }
            @if let Some(review) = &e.review {
                @for para in review.split("\n\n") { p { (para) } }
            }
        },
    );
    ([(header::CACHE_CONTROL, PUBLIC_CACHE)], Html(page_html.into_string())).into_response()
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
async fn admin_page(State(state): State<AppState>, Query(lq): Query<ListQuery>) -> Result<Html<String>, ApiErr> {
    admin_guard(&state.env)?;
    let page = lq.page.unwrap_or(1).clamp(1, MAX_PAGE);
    let q = lq.q.as_deref().map(str::trim).unwrap_or("");
    let (entries, more) = load_entries(&state.env, None, Some(q), page).await.map_err(internal)?;
    Ok(Html(admin_ui::page(&entries, page, more, q).into_string()))
}

#[worker::send]
async fn admin_edit_page(State(state): State<AppState>, Path((kind, slug)): Path<(String, String)>) -> Result<Html<String>, ApiErr> {
    admin_guard(&state.env)?;
    let Some(e) = find_entry(&state.env, &kind, &slug).await.map_err(internal)? else {
        return Err((StatusCode::NOT_FOUND, "no such entry".into()));
    };
    let cover = e.cover_key.as_deref().map(|k| cover_src(&state.env, k));
    Ok(Html(admin_ui::edit_page(&e, cover).into_string()))
}

#[derive(Deserialize)]
struct LookupQuery {
    kind: Kind,
    id: String,
}

#[derive(serde::Serialize)]
struct Lookup {
    kind: Kind,
    #[serde(flatten)]
    candidate: catalogged_core::Candidate,
}

/// Fetch one record by provider id, so the admin can add by id. An IMDb id (`tt...`) resolves via
/// TMDB and may turn out to be a show even if "movie" was picked, so the real kind is returned.
#[worker::send]
async fn lookup(State(state): State<AppState>, Query(q): Query<LookupQuery>) -> Result<Json<Lookup>, ApiErr> {
    admin_guard(&state.env)?;
    let id = q.id.trim();
    let bad = |m: &str| (StatusCode::BAD_REQUEST, m.to_string());
    let (kind, candidate) = match q.kind {
        Kind::Movie | Kind::Show if tmdb::is_imdb_id(id) => {
            let body = tmdb_get(&state.env, tmdb::find_url(id)).await.map_err(internal)?;
            tmdb::parse_find(&body).map_err(internal)?.ok_or_else(|| bad("TMDB has no movie or show for that IMDb id"))?
        }
        Kind::Movie | Kind::Show => {
            let url = tmdb::details_url(q.kind, id).ok_or_else(|| bad("unsupported kind"))?;
            (q.kind, tmdb::parse_details(&tmdb_get(&state.env, url).await.map_err(internal)?).map_err(internal)?)
        }
        Kind::Book => {
            let body = open_library_get(&openlibrary::details_url(id)).await.map_err(internal)?;
            (Kind::Book, openlibrary::parse_details(&body).map_err(internal)?)
        }
        Kind::Game => {
            let body = igdb::details_body(id).ok_or_else(|| bad("IGDB ids are numbers"))?;
            let game = igdb::parse_games(&igdb_post(&state.env, body).await.map_err(internal)?)
                .map_err(internal)?
                .into_iter()
                .next()
                .ok_or_else(|| bad("IGDB has no game with that id"))?;
            (Kind::Game, game)
        }
    };
    Ok(Json(Lookup { kind, candidate }))
}

#[worker::send]
async fn get_entry(State(state): State<AppState>, Path((kind, slug)): Path<(String, String)>) -> Result<Json<Entry>, ApiErr> {
    admin_guard(&state.env)?;
    match find_entry(&state.env, &kind, &slug).await.map_err(internal)? {
        Some(e) => Ok(Json(e)),
        None => Err((StatusCode::NOT_FOUND, "no such entry".into())),
    }
}

#[worker::send]
async fn edit_entry(
    State(state): State<AppState>,
    Path((kind, slug)): Path<(String, String)>,
    Json(edit): Json<EditEntry>,
) -> Result<Json<Entry>, ApiErr> {
    admin_guard(&state.env)?;
    edit.validate().map_err(|m| (StatusCode::BAD_REQUEST, m))?;
    if find_entry(&state.env, &kind, &slug).await.map_err(internal)?.is_none() {
        return Err((StatusCode::NOT_FOUND, "no such entry".into()));
    }
    state
        .env
        .d1("DB")
        .map_err(internal)?
        .prepare(
            "UPDATE entries SET title = ?1, status = ?2, rating = ?3, review = ?4, year = ?5, consumed_at = ?6, \
             updated_at = datetime('now') WHERE kind = ?7 AND slug = ?8",
        )
        .bind(&[
            edit.title.trim().into(),
            edit.status.as_str().into(),
            opt(edit.rating.map(f64::from)),
            opt(edit.review.filter(|r| !r.trim().is_empty())),
            opt(edit.year.map(f64::from)),
            opt(edit.consumed_at.filter(|d| !d.is_empty())),
            kind.clone().into(),
            slug.clone().into(),
        ])
        .map_err(internal)?
        .run()
        .await
        .map_err(internal)?;
    let updated = find_entry(&state.env, &kind, &slug).await.map_err(internal)?;
    updated.map(Json).ok_or((StatusCode::NOT_FOUND, "no such entry".into()))
}

#[worker::send]
async fn delete_entry(
    State(state): State<AppState>,
    Path((kind, slug)): Path<(String, String)>,
) -> Result<StatusCode, ApiErr> {
    admin_guard(&state.env)?;
    let Some(entry) = find_entry(&state.env, &kind, &slug).await.map_err(internal)? else {
        return Err((StatusCode::NOT_FOUND, "no such entry".into()));
    };
    state
        .env
        .d1("DB")
        .map_err(internal)?
        .prepare("DELETE FROM entries WHERE kind = ?1 AND slug = ?2")
        .bind(&[kind.into(), slug.into()])
        .map_err(internal)?
        .run()
        .await
        .map_err(internal)?;
    // Remove the cover too, so deleted entries don't leave R2 storage behind.
    if let Some(key) = entry.cover_key {
        state.env.bucket("COVERS").map_err(internal)?.delete(key).await.map_err(internal)?;
    }
    Ok(StatusCode::NO_CONTENT)
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

async fn send_text(url: &str, init: RequestInit, what: &str) -> Result<String> {
    let mut res = Fetch::Request(Request::new_with_init(url, &init)?).send().await?;
    if res.status_code() != 200 {
        return Err(Error::RustError(format!("{what} returned {}", res.status_code())));
    }
    res.text().await
}

async fn open_library_get(url: &str) -> Result<String> {
    let headers = Headers::new();
    headers.set("Accept", "application/json")?;
    headers.set("User-Agent", "catalogged-personal/0.1")?;
    let mut init = RequestInit::new();
    init.with_headers(headers);
    send_text(url, init, "Open Library").await
}

/// IGDB needs a Twitch app token first. Two subrequests per admin search (limit 50), which is fine
/// for a single-user admin; revisit with Cache API caching if searches ever become frequent.
async fn igdb_post(env: &Env, body: String) -> Result<String> {
    let id = env.secret("IGDB_CLIENT_ID")?.to_string();
    let secret = env.secret("IGDB_CLIENT_SECRET")?.to_string();
    let mut init = RequestInit::new();
    init.with_method(Method::Post);
    let token = igdb::parse_token(&send_text(&igdb::token_url(&id, &secret), init, "Twitch token").await?)
        .map_err(|e| Error::RustError(e.to_string()))?;
    let headers = Headers::new();
    headers.set("Client-ID", &id)?;
    headers.set("Authorization", &format!("Bearer {token}"))?;
    headers.set("Accept", "application/json")?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post).with_headers(headers).with_body(Some(JsValue::from_str(&body)));
    send_text(igdb::GAMES_URL, init, "IGDB").await
}

#[worker::send]
async fn search(
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Vec<catalogged_core::Candidate>>, ApiErr> {
    admin_guard(&state.env)?;
    let term = q.q.trim();
    let mut hits = match q.kind {
        Kind::Movie | Kind::Show => {
            let url = tmdb::search_url(q.kind, term).ok_or((StatusCode::BAD_REQUEST, "unsupported kind".to_string()))?;
            tmdb::parse_search(&tmdb_get(&state.env, url).await.map_err(internal)?).map_err(internal)?
        }
        Kind::Book => openlibrary::parse_search(&open_library_get(&openlibrary::search_url(term)).await.map_err(internal)?)
            .map_err(internal)?,
        Kind::Game => igdb::parse_games(&igdb_post(&state.env, igdb::search_body(term)).await.map_err(internal)?)
            .map_err(internal)?,
    };
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
    new.validate().map_err(|m| (StatusCode::BAD_REQUEST, m))?;
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
        "INSERT INTO entries (kind, slug, title, status, rating, review, year, source, external_id, consumed_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )
    .bind(&[
        new.kind.as_str().into(),
        slug.clone().into(),
        new.title.into(),
        new.status.as_str().into(),
        opt(new.rating.map(f64::from)),
        opt(new.review),
        opt(new.year.map(f64::from)),
        opt(source),
        opt(new.external_id),
        opt(new.consumed_at),
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
        .prepare("SELECT cover_key FROM entries WHERE kind = ?1 AND slug = ?2")
        .bind(&[kind.clone().into(), slug.clone().into()])
        .map_err(internal)?
        .first::<serde_json::Value>(None)
        .await
        .map_err(internal)?;
    let Some(existing) = exists else {
        return Err((StatusCode::NOT_FOUND, "no such entry".into()));
    };
    let old_key = existing.get("cover_key").and_then(|v| v.as_str()).map(str::to_owned);
    // Content-hashed key: replacing a cover writes a new URL, so the immutable CDN cache never
    // serves a stale image. FNV-1a over <= 300 KB is far under the CPU limit.
    let hash = body.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100000001b3));
    let key = format!("{kind}/{slug}-{:08x}.webp", hash & 0xffff_ffff);
    let bucket = state.env.bucket("COVERS").map_err(internal)?;
    bucket
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
    if let Some(old) = old_key.filter(|o| *o != key) {
        // Best effort: a failed delete only leaves one orphan object.
        let _ = bucket.delete(old).await;
    }
    Ok(Json(serde_json::json!({ "cover_key": key, "url": cover_src(&state.env, &key) })))
}
