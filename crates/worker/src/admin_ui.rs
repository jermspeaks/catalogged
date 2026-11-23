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
    if (c.poster_path) {
      const img = document.createElement('img');
      img.src = `https://image.tmdb.org/t/p/w92${c.poster_path}`; img.width = 46; img.alt = '';
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
  const r = await fetch('/admin/api/entries', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ kind, title: c.title, status, year: c.year, source: 'tmdb', external_id: c.id }),
  });
  if (!r.ok) { msg(`Add failed: ${r.status} ${(await r.text()).slice(0, 160)}`); btn.disabled = false; return; }
  const { slug } = await r.json();
  if (c.poster_path) {
    try {
      const blob = await toWebp(`https://image.tmdb.org/t/p/w500${c.poster_path}`);
      const up = await fetch(`/admin/api/entries/${kind}/${slug}/cover`, { method: 'PUT', headers: { 'content-type': 'image/webp' }, body: blob });
      if (!up.ok) throw new Error(`cover upload ${up.status}`);
    } catch (e) { msg(`Added "${c.title}" but the cover failed: ${e.message}`); btn.textContent = 'Added (no cover)'; return; }
  }
  btn.textContent = 'Added ✓';
  msg(`Added "${c.title}". Public pages refresh within ~5 minutes.`);
}

$('#search').addEventListener('submit', search);
"#;

pub fn page(entries: &[Entry]) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="robots" content="noindex";
                title { "catalogged admin" }
                style { (PreEscaped("body{font:16px system-ui;max-width:48rem;margin:2rem auto;padding:0 1rem}li{margin:.4rem 0;list-style:none;display:flex;align-items:center;gap:.5rem}ul{padding:0}input,select,button{font:inherit}")) }
            }
            body {
                h1 { "Add to catalogue" }
                form #search {
                    select #kind { option value="movie" { "Movie" } option value="show" { "TV show" } }
                    " "
                    input #q type="search" placeholder="Search TMDB…" required;
                    " status "
                    select #status {
                        option value="wanted" { "wanted" }
                        option value="in_progress" { "in progress" }
                        option value="consumed" { "consumed" }
                    }
                    " " button type="submit" { "Search" }
                }
                p { small { "Games and books are added from the CLI until their providers land." } }
                p #msg role="status" {}
                ul #results {}
                h2 { "Catalogue (" (entries.len()) ")" }
                ul {
                    @for e in entries {
                        li { (e.title) " · " (e.kind.as_str()) " · " (e.status.as_str()) @if e.cover_key.is_none() { " · no cover" } }
                    }
                }
                script { (PreEscaped(SCRIPT.replace("__MAX__", &MAX_COVER_BYTES.to_string()))) }
            }
        }
    }
}
