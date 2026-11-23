use axum::{extract::State, http::StatusCode, response::Html, routing::{get, post}, Json, Router};
use catalogged_core::{slugify, Entry, NewEntry};
use maud::{html, Markup, DOCTYPE};
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

fn router(env: Env) -> Router {
    Router::new()
        .route("/", get(home))
        // Everything under /admin is gated by Cloudflare Access (see docs/plan.md).
        .route("/admin/api/entries", post(create_entry))
        .route("/healthz", get(|| async { "ok" }))
        .with_state(AppState { env })
}

#[event(fetch)]
async fn fetch(
    req: HttpRequest,
    env: Env,
    _ctx: Context,
) -> Result<http::Response<axum::body::Body>> {
    console_error_panic_hook::set_once();
    Ok(router(env).call(req).await?)
}

fn layout(title: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head { meta charset="utf-8"; title { (title) } }
            body { (body) }
        }
    }
}

#[worker::send]
async fn home(State(state): State<AppState>) -> Html<String> {
    let entries = load_entries(&state.env).await.unwrap_or_default();
    let page = layout(
        "catalogged",
        html! {
            h1 { "catalogged" }
            ul {
                @for e in &entries { li { (e.title) " (" (e.kind.as_str()) ", " (e.status.as_str()) ")" } }
            }
        },
    );
    Html(page.into_string())
}

async fn load_entries(env: &Env) -> Result<Vec<Entry>> {
    let db = env.d1("DB")?;
    // Serde on the enums matches the snake_case strings stored in D1.
    db.prepare(
        "SELECT id, kind, slug, title, status, rating, notes, year, source, external_id, cover_key, consumed_at \
         FROM entries ORDER BY updated_at DESC LIMIT 200",
    )
    .all()
    .await?
    .results::<Entry>()
}

fn opt<T: Into<JsValue>>(v: Option<T>) -> JsValue {
    v.map(Into::into).unwrap_or(JsValue::NULL)
}

#[worker::send]
async fn create_entry(
    State(state): State<AppState>,
    Json(new): Json<NewEntry>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Kill switch: admin stays off until ADMIN_ENABLED=1 is set (after Cloudflare Access is in front).
    if state.env.var("ADMIN_ENABLED").map(|v| v.to_string()).ok().as_deref() != Some("1") {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "admin disabled".into()));
    }
    let err = |e: Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    let db = state.env.d1("DB").map_err(err)?;
    let slug = slugify(&new.title);
    if slug.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "title produces an empty slug".into()));
    }
    let source = new.source.map(|s| serde_json::to_value(s).ok().and_then(|v| v.as_str().map(String::from))).flatten();
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
    .map_err(err)?
    .run()
    .await
    // A UNIQUE violation (duplicate slug / external id) surfaces here.
    .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(serde_json::json!({ "slug": slug })))
}
