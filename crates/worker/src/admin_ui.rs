//! The /admin page: TMDB search, add, and browser-side WebP cover conversion.
//! Covers are encoded in the browser because the Worker's free-tier CPU limit is ~10 ms.

use catalogged_core::{Entry, MAX_COVER_BYTES};
use maud::{html, Markup, PreEscaped, DOCTYPE};

const SCRIPT: &str = r#"
const $ = (s) => document.querySelector(s);
const MAX = __MAX__;
const msg = (t) => ($('#msg').textContent = t);

async function search(ev) {
  ev.preventDefault();
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
    label.textContent = ` ${c.title}${c.year ? ` (${c.year})` : ''} `;
    const btn = document.createElement('button');
    btn.textContent = 'Add';
    btn.onclick = () => add(kind, c, btn);
    li.append(label, btn);
    ul.append(li);
  }
}

async function toWebp(url) {
  const img = new Image();
  img.crossOrigin = 'anonymous';
  img.src = url;
  await img.decode();
  const canvas = document.createElement('canvas');
  canvas.width = img.naturalWidth; canvas.height = img.naturalHeight;
  canvas.getContext('2d').drawImage(img, 0, 0);
  for (const q of [0.82, 0.7, 0.55, 0.4]) {
    const blob = await new Promise((res) => canvas.toBlob(res, 'image/webp', q));
    if (blob && blob.type === 'image/webp' && blob.size <= MAX) return blob;
  }
  throw new Error('could not encode a WebP under the size cap');
}

async function add(kind, c, btn) {
  btn.disabled = true;
  const status = $('#status').value;
  const today = new Date().toLocaleDateString('en-CA');
  const consumed_at = $('#date').value || (status === 'consumed' ? today : null);
  const r = await fetch('/admin/api/entries', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ kind, title: c.title, status, year: c.year, source: {movie:'tmdb',show:'tmdb',book:'open_library',game:'igdb'}[kind], external_id: c.id, consumed_at }),
  });
  if (!r.ok) { msg(`Add failed: ${r.status} ${(await r.text()).slice(0, 160)}`); btn.disabled = false; return; }
  const { slug } = await r.json();
  if (c.cover_url) {
    try {
      const blob = await toWebp(c.cover_url);
      const up = await fetch(`/admin/api/entries/${kind}/${slug}/cover`, { method: 'PUT', headers: { 'content-type': 'image/webp' }, body: blob });
      if (!up.ok) throw new Error(`cover upload ${up.status}`);
    } catch (e) { msg(`Added "${c.title}" but the cover failed: ${e.message}`); btn.textContent = 'Added (no cover)'; return; }
  }
  btn.textContent = 'Added ✓';
  msg(`Added "${c.title}". Public pages refresh within ~5 minutes.`);
}

$('#search').addEventListener('submit', search);

const val = (f, n) => f.elements[n].value.trim() || null;

document.addEventListener('submit', async (ev) => {
  const f = ev.target;
  if (!f.classList.contains('edit')) return;
  ev.preventDefault();
  const { kind, slug } = f.dataset;
  const rating = val(f, 'rating'), year = val(f, 'year');
  const r = await fetch(`/admin/api/entries/${kind}/${slug}`, {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      title: f.elements.title.value.trim(), status: f.elements.status.value,
      rating: rating && Number(rating), year: year && Number(year),
      consumed_at: val(f, 'consumed_at'), review: val(f, 'review'),
    }),
  });
  f.querySelector('.state').textContent = r.ok ? 'Saved ✓ (public pages refresh within ~5 min)' : `Failed: ${r.status} ${(await r.text()).slice(0, 120)}`;
});

document.addEventListener('click', async (ev) => {
  const b = ev.target.closest('button.del');
  if (!b) return;
  const f = b.closest('form'); const { kind, slug } = f.dataset;
  if (!confirm(`Delete "${f.elements.title.value}"? This also removes its cover.`)) return;
  const r = await fetch(`/admin/api/entries/${kind}/${slug}`, { method: 'DELETE' });
  if (r.ok) f.closest('li').remove(); else f.querySelector('.state').textContent = `Delete failed: ${r.status}`;
});
"#;

pub fn page(entries: &[Entry], page: u32, more: bool, q: &str) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="robots" content="noindex";
                title { "catalogged admin" }
                style { (PreEscaped("body{font:16px system-ui;max-width:48rem;margin:2rem auto;padding:0 1rem}li{margin:.4rem 0;list-style:none}ul#results li{display:flex;align-items:center;gap:.5rem}ul{padding:0}form.edit label{display:block;margin:.3rem 0}input,select,button{font:inherit}")) }
            }
            body {
                h1 { "Add to catalogue" }
                form #search {
                    select #kind { option value="movie" { "Movie" } option value="show" { "TV show" } option value="book" { "Book" } option value="game" { "Game" } }
                    " "
                    input #q type="search" placeholder="Search by title…" required;
                    " status "
                    select #status {
                        option value="wanted" { "wanted" }
                        option value="in_progress" { "in progress" }
                        option value="consumed" { "consumed" }
                    }
                    " date " input #date type="date" title="Defaults to today when status is consumed";
                    " " button type="submit" { "Search" }
                }
                p { small { "Games and books are added from the CLI until their providers land." } }
                p #msg role="status" {}
                ul #results {}
                h2 { "Catalogue" }
                form method="get" action="/admin" {
                    input type="search" name="q" value=(q) placeholder="Filter by title…";
                    " " button type="submit" { "Filter" }
                }
                ul.entries {
                    @for e in entries {
                        li {
                            details {
                                summary { (e.title) " · " (e.kind.as_str()) " · " (e.status.as_str()) @if e.cover_key.is_none() { " · no cover" } }
                                form.edit data-kind=(e.kind.as_str()) data-slug=(e.slug) {
                                    label { "Title " input name="title" value=(e.title) required; }
                                    label { "Status "
                                        select name="status" {
                                            @for st in ["wanted", "in_progress", "consumed"] {
                                                option value=(st) selected[st == e.status.as_str()] { (st) }
                                            }
                                        }
                                    }
                                    label { "Rating (1-10) " input name="rating" type="number" min="1" max="10" value=(e.rating.map(|r| r.to_string()).unwrap_or_default()); }
                                    label { "Year " input name="year" type="number" value=(e.year.map(|y| y.to_string()).unwrap_or_default()); }
                                    label { "Date logged " input name="consumed_at" type="date" value=(e.consumed_at.clone().unwrap_or_default()); }
                                    label { "Review " textarea name="review" rows="6" cols="60" { (e.review.clone().unwrap_or_default()) } }
                                    button type="submit" { "Save" }
                                    " " button.del type="button" { "Delete" }
                                    " " small.state {}
                                }
                            }
                        }
                    }
                }
                p {
                    @if page > 1 { a href={"/admin?page=" (page - 1) @if !q.is_empty() { "&q=" (q) }} { "← newer" } }
                    @if page > 1 && more { " · " }
                    @if more { a href={"/admin?page=" (page + 1) @if !q.is_empty() { "&q=" (q) }} { "older →" } }
                }
                script { (PreEscaped(SCRIPT.replace("__MAX__", &MAX_COVER_BYTES.to_string()))) }
            }
        }
    }
}
