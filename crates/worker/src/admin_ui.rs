//! The /admin pages: search/add, add by id, manual add, and a per-entry edit page with cover
//! upload and a confirmed delete. Covers are encoded to WebP in the browser because the Worker's
//! free-tier CPU limit is ~10 ms.

use crate::public_ui::{theme_picker, CSS, FAVICON_LINKS, THEME_INIT, THEME_JS};
use catalogged_core::{Entry, Kind, MAX_COVER_BYTES};
use maud::{html, Markup, PreEscaped, DOCTYPE};

/// Shared browser helpers. `toWebp` takes an image URL or a File, scales to at most 500px wide
/// (poster size) and steps quality down until it fits under the size cap.
const COMMON_JS: &str = r#"
const $ = (s) => document.querySelector(s);
const MAX = __MAX__;
let msgTimer;
// Toast: busy (spinner) while a message ends in an ellipsis, error on failures, otherwise ok and auto-hides.
function msg(t, type) {
  const el = $('#msg'); clearTimeout(msgTimer);
  type = type || (/fail|error|refus|cannot/i.test(t) ? 'err' : /…$/.test(t) ? 'busy' : 'ok');
  el.textContent = t; el.className = t ? type : '';
  if (t && type === 'ok') msgTimer = setTimeout(() => { el.textContent = ''; el.className = ''; }, 8000);
}
// Disable a form's buttons and dim it while `fn` runs, so a slow save can't be double-submitted.
async function busy(el, fn) {
  const btns = [...el.querySelectorAll('button')];
  btns.forEach((b) => (b.disabled = true)); el.setAttribute('aria-busy', 'true');
  try { return await fn(); } finally { btns.forEach((b) => (b.disabled = false)); el.removeAttribute('aria-busy'); }
}
const SOURCE = { movie: 'tmdb', show: 'tmdb', book: 'open_library', game: 'igdb' };

async function loadImage(src) {
  const img = new Image();
  if (typeof src === 'string') { img.crossOrigin = 'anonymous'; img.src = src; }
  else img.src = URL.createObjectURL(src);
  await img.decode();
  return img;
}

async function toWebp(src) {
  let img;
  try { img = await loadImage(src); }
  catch (e) {
    // Most hosts send no CORS headers, so the canvas can't read them: relay through the Worker.
    if (typeof src !== 'string') throw e;
    img = await loadImage(`/admin/api/image?url=${encodeURIComponent(src)}`);
  }
  const scale = Math.min(1, 500 / img.naturalWidth);
  const canvas = document.createElement('canvas');
  canvas.width = Math.round(img.naturalWidth * scale); canvas.height = Math.round(img.naturalHeight * scale);
  canvas.getContext('2d').drawImage(img, 0, 0, canvas.width, canvas.height);
  for (const q of [0.82, 0.7, 0.55, 0.4]) {
    const blob = await new Promise((res) => canvas.toBlob(res, 'image/webp', q));
    if (blob && blob.type === 'image/webp' && blob.size <= MAX) return blob;
  }
  throw new Error('could not encode a WebP under the size cap');
}

async function putCover(kind, slug, src) {
  const blob = await toWebp(src);
  const r = await fetch(`/admin/api/entries/${kind}/${slug}/cover`, { method: 'PUT', headers: { 'content-type': 'image/webp' }, body: blob });
  if (!r.ok) throw new Error(`cover upload ${r.status}`);
  return r.json();
}
"#;

const ADMIN_JS: &str = r#"
function search(ev) { ev.preventDefault(); return busy($('#search'), () => doSearch()); }
async function doSearch() {
  const kind = $('#kind').value, q = $('#q').value.trim();
  if (!q) return;
  msg('Searching…');
  const r = await fetch(`/admin/api/search?kind=${kind}&q=${encodeURIComponent(q)}`);
  if (!r.ok) { msg(`Search failed: ${r.status} ${await r.text()}`); return; }
  const items = await r.json();
  msg(items.length ? '' : 'No results.');
  const ul = $('#results'); ul.replaceChildren();
  for (const c of items) {
    const li = document.createElement('li');
    if (c.thumb_url) {
      const img = document.createElement('img');
      img.src = c.thumb_url; img.width = 46; img.alt = '';
      li.append(img);
    }
    const label = document.createElement('span');
    label.textContent = ` ${c.title}${c.year ? ` (${c.year})` : ''}${c.creator ? ` · ${c.creator}` : ''} `;
    const btn = document.createElement('button');
    btn.textContent = 'Add';
    btn.onclick = () => add(kind, c, btn);
    li.append(label, btn);
    ul.append(li);
  }
}

function defaults() {
  const status = $('#status').value;
  const today = new Date().toLocaleDateString('en-CA');
  return { status, consumed_at: $('#date').value || (status === 'consumed' ? today : null) };
}

async function create(body) {
  const r = await fetch('/admin/api/entries', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });
  if (!r.ok) throw new Error(`${r.status} ${(await r.text()).slice(0, 160)}`);
  return (await r.json()).slug;
}

async function add(kind, c, btn) {
  if (btn) btn.disabled = true;
  let slug;
  try {
    slug = await create({ kind, title: c.title, year: c.year, creator: c.creator, source: SOURCE[kind], external_id: c.id, ...defaults() });
  } catch (e) { msg(`Add failed: ${e.message}`); if (btn) btn.disabled = false; return; }
  if (c.cover_url) {
    try { await putCover(kind, slug, c.cover_url); }
    catch (e) { msg(`Added "${c.title}" but the cover failed: ${e.message}. Add one from its edit page.`); if (btn) btn.textContent = 'Added (no cover)'; return; }
  }
  if (btn) btn.textContent = 'Added ✓';
  msg(`Added "${c.title}". Public pages update right away here; other locations catch up within minutes.`);
}

$('#search').addEventListener('submit', search);

// Add by provider id (TMDB id, IMDb tt id, Open Library work id, IGDB id).
$('#byid').addEventListener('submit', async (ev) => {
  ev.preventDefault();
  const kind = $('#idkind').value, id = $('#extid').value.trim();
  msg('Looking up…');
  const r = await fetch(`/admin/api/lookup?kind=${kind}&id=${encodeURIComponent(id)}`);
  if (!r.ok) { msg(`Lookup failed: ${r.status} ${(await r.text()).slice(0, 160)}`); return; }
  const c = await r.json();
  await add(c.kind, c, null);
});

// Manual entry (no provider), optional cover file.
$('#manual').addEventListener('submit', async (ev) => {
  ev.preventDefault();
  const f = ev.target, kind = f.elements.kind.value, title = f.elements.title.value.trim();
  const year = f.elements.year.value.trim();
  let slug;
  try { slug = await create({ kind, title, year: year ? Number(year) : null, creator: f.elements.creator.value.trim() || null, ...defaults() }); }
  catch (e) { msg(`Add failed: ${e.message}`); return; }
  const file = f.elements.cover.files[0];
  if (file) {
    try { await putCover(kind, slug, file); }
    catch (e) { msg(`Added "${title}" but the cover failed: ${e.message}`); return; }
  }
  msg(`Added "${title}". Public pages update right away here; other locations catch up within minutes.`);
  f.reset();
});
"#;

const EDIT_JS: &str = r#"
const form = $('#edit'); const { kind, slug } = form.dataset;
const val = (n) => form.elements[n].value.trim() || null;

form.addEventListener('submit', (ev) => { ev.preventDefault(); busy(form, save); });
async function save() {
  msg('Saving…');
  const rating = val('rating'), year = val('year');
  const r = await fetch(`/admin/api/entries/${kind}/${slug}`, {
    method: 'PUT', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ title: form.elements.title.value.trim(), status: form.elements.status.value,
      rating: rating && Number(rating), year: year && Number(year), consumed_at: val('consumed_at'), review: val('review'),
      source: val('external_id') && SOURCE[kind], external_id: val('external_id'), creator: val('creator') }),
  });
  msg(r.ok ? 'Saved ✓ (public pages update right away here; other locations catch up within minutes)' : `Failed: ${r.status} ${(await r.text()).slice(0, 120)}`);
}

async function setCover(src) {
  msg('Converting and uploading…');
  try {
    const { url } = await putCover(kind, slug, src);
    $('#coverph')?.remove();
    let img = $('#coverimg');
    if (!img) { img = document.createElement('img'); img.id = 'coverimg'; img.className = 'cover'; img.alt = ''; $('#coverbox').prepend(img); }
    img.src = url; msg('Cover updated ✓');
  } catch (e) { msg(`Cover failed: ${e.message}`); }
}

$('#coverfile').addEventListener('change', (ev) => { const file = ev.target.files[0]; if (file) setCover(file); });

// Cover from a link (the Worker relays the image because most hosts block direct reads).
$('#coverlink').addEventListener('submit', (ev) => { ev.preventDefault(); busy(ev.target, () => setCover($('#coverurl').value.trim())); });

// Cover from a provider search (same sources as adding an entry).
$('#coversearch').addEventListener('submit', (ev) => { ev.preventDefault(); busy(ev.target, coverSearch); });
async function coverSearch() {
  const q = $('#coverq').value.trim(); if (!q) return;
  msg('Searching…');
  const r = await fetch(`/admin/api/search?kind=${kind}&q=${encodeURIComponent(q)}`);
  if (!r.ok) { msg(`Search failed: ${r.status} ${(await r.text()).slice(0, 120)}`); return; }
  const items = (await r.json()).filter((c) => c.cover_url);
  msg(items.length ? '' : 'No results with a cover.');
  const ul = $('#coverresults'); ul.replaceChildren();
  for (const c of items) {
    const li = document.createElement('li');
    if (c.thumb_url) { const im = document.createElement('img'); im.src = c.thumb_url; im.width = 46; im.alt = ''; li.append(im); }
    const label = document.createElement('span'); label.textContent = ` ${c.title}${c.year ? ` (${c.year})` : ''} `;
    const btn = document.createElement('button'); btn.type = 'button'; btn.textContent = 'Use this cover';
    btn.onclick = async () => {
      await setCover(c.cover_url);
      // Picking the record is also saying which record this is: fill the provider id (and year) in the form.
      const f = form.elements;
      if (!f.external_id.value.trim()) {
        f.external_id.value = c.id; if (!f.year.value && c.year) f.year.value = c.year;
        if (!f.creator.value.trim() && c.creator) f.creator.value = c.creator;
        msg('Cover updated ✓ and the provider id filled in: press Save to keep it', 'ok');
      }
    };
    li.append(label, btn); ul.append(li);
  }
}

const dlg = $('#confirm');
$('#askdel').onclick = () => dlg.showModal();
$('#cancel').onclick = () => dlg.close();
$('#dodel').onclick = async () => {
  msg('Deleting…');
  const r = await fetch(`/admin/api/entries/${kind}/${slug}`, { method: 'DELETE' });
  if (r.ok) location.href = '/admin'; else { dlg.close(); msg(`Delete failed: ${r.status}`); }
};
"#;

fn shell(title: &str, body: Markup, js: &str) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="robots" content="noindex";
                title { (title) }
                script { (PreEscaped(THEME_INIT)) }
                style { (PreEscaped(CSS)) }
                (PreEscaped(FAVICON_LINKS))
            }
            body {
                header.top {
                    nav.bar { a href="/" { "← Public site" } a href="/admin" { "Admin home" } }
                    (theme_picker())
                }
                (body)
                script { (PreEscaped(THEME_JS)) }
                script { (PreEscaped(COMMON_JS.replace("__MAX__", &MAX_COVER_BYTES.to_string()))) (PreEscaped(js)) }
            }
        }
    }
}

pub fn page(entries: &[Entry], page: u32, more: bool, q: &str) -> Markup {
    let kinds = || html! {
        option value="movie" { "Movie" } option value="show" { "TV show" } option value="book" { "Book" } option value="game" { "Game" }
    };
    shell("catalogged admin", html! {
        h1 { "Add to catalogue" }
        div.row {
            label { "Status "
                select #status {
                    option value="wanted" { "wanted" }
                    option value="in_progress" { "in progress" }
                    option value="consumed" { "consumed" }
                }
            }
            label { "Date logged " input #date type="date" title="Defaults to today when status is consumed"; }
        }
        p { small { "Status and date apply to everything added below." } }
        form #search.row {
            select #kind { (kinds()) }
            input #q type="search" placeholder="Search by title…" required;
            button type="submit" { "Search" }
        }
        p #msg role="status" {}
        ul #results.plain {}
        details {
            summary { "Add by id" }
            form #byid.row {
                select #idkind { (kinds()) }
                input #extid placeholder="TMDB id, IMDb id (tt…), Open Library work id (OL…W), or IGDB id" size="48" required;
                button type="submit" { "Look up and add" }
            }
        }
        details {
            summary { "Add manually (no provider)" }
            form #manual {
                label { "Kind " select name="kind" { (kinds()) } }
                label { "Title " input name="title" required; }
                label { "Year " input name="year" type="number"; }
                label { "Author / director / developer (optional) " input name="creator"; }
                label { "Cover image (optional) " input name="cover" type="file" accept="image/*"; }
                button type="submit" { "Add" }
            }
        }
        h2 { "Catalogue" }
        form.row method="get" action="/admin" {
            input type="search" name="q" value=(q) placeholder="Filter by title…";
            button type="submit" { "Filter" }
        }
        ul.plain {
            @for e in entries {
                li {
                    a href={"/admin/edit/" (e.kind.as_str()) "/" (e.slug)} { (e.title) }
                    " · " (e.kind.as_str()) " · " (e.status.as_str())
                    @if e.cover_key.is_none() { " · no cover" }
                }
            }
        }
        p {
            @if page > 1 { a href={"/admin?page=" (page - 1) @if !q.is_empty() { "&q=" (q) }} { "← newer" } }
            @if page > 1 && more { " · " }
            @if more { a href={"/admin?page=" (page + 1) @if !q.is_empty() { "&q=" (q) }} { "older →" } }
        }
    }, ADMIN_JS)
}

fn creator_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Movie => "Director",
        Kind::Show => "Creator",
        Kind::Book => "Author",
        Kind::Game => "Developer",
    }
}

fn provider_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Movie | Kind::Show => "TMDB",
        Kind::Book => "Open Library work",
        Kind::Game => "IGDB",
    }
}

pub fn edit_page(e: &Entry, cover_url: Option<String>) -> Markup {
    shell(&format!("Edit {}", e.title), html! {
        h1 { "Edit: " (e.title) }
        p.bar {
            a href={"/entry/" (e.kind.as_str()) "/" (e.slug)} { "View public page" }
            " · " (e.kind.as_str())
        }
        p #msg role="status" aria-live="polite" {}
        div.edit {
        div #coverbox {
            @if let Some(u) = &cover_url { img #coverimg.cover src=(u) alt=""; } @else { div #coverph.cover.ph {} }
            label { "Upload or replace cover " input #coverfile type="file" accept="image/*"; }
            details {
                summary { "Find a cover (search " (e.kind.as_str()) " sources)" }
                form #coversearch.row {
                    input #coverq type="search" value=(e.title) required;
                    button type="submit" { "Search" }
                }
                ul #coverresults.plain {}
            }
            details {
                summary { "Use a link to an image" }
                form #coverlink.row {
                    input #coverurl type="url" placeholder="https://…/poster.jpg" required;
                    button type="submit" { "Use link" }
                }
            }
        }
        form #edit data-kind=(e.kind.as_str()) data-slug=(e.slug) {
            div.fields {
                label.wide { "Title " input name="title" value=(e.title) required; }
                label { "Status "
                    select name="status" {
                        @for st in ["wanted", "in_progress", "consumed"] {
                            option value=(st) selected[st == e.status.as_str()] { (st) }
                        }
                    }
                }
                label { "Date logged " input name="consumed_at" type="date" value=(e.consumed_at.clone().unwrap_or_default()); }
                label { "Rating (1-10) " input name="rating" type="number" min="1" max="10" value=(e.rating.map(|r| r.to_string()).unwrap_or_default()); }
                label { "Year " input name="year" type="number" value=(e.year.map(|y| y.to_string()).unwrap_or_default()); }
                label.wide { (creator_label(e.kind)) " " input name="creator" value=(e.creator.clone().unwrap_or_default()) placeholder="optional"; }
                label.wide { (provider_name(e.kind)) " id " input name="external_id" value=(e.external_id.clone().unwrap_or_default()) placeholder="optional, e.g. a TMDB id"; }
                label.wide { "Review " textarea name="review" rows="8" { (e.review.clone().unwrap_or_default()) } }
            }
            div.row {
                button type="submit" { "Save" }
                button #askdel.danger type="button" { "Delete…" }
            }
        }
        }
        dialog #confirm {
            h2 { "Delete this entry?" }
            p { "“" (e.title) "” and its cover will be removed. This cannot be undone." }
            div.row {
                button #cancel type="button" { "Cancel" }
                button #dodel.danger type="button" { "Yes, delete" }
            }
        }
    }, EDIT_JS)
}
