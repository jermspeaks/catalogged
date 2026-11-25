//! Styles and client script for the public site. No framework: a small script enhances the
//! server-rendered grid by loading the cached `/index.json` once and filtering locally, so
//! search, filter and sort cost zero D1 reads.
use catalogged_core::Status;

/// Palette: #c6d8ff #71a9f7 #6b5ca5 #72195a #4c1036 (Periwinkle, Cool Horizon, Dusty Grape, Royal Plum, Crimson Violet).
pub const CSS: &str = r#"
:root{--bg:#f6f8ff;--fg:#4c1036;--muted:#6b5ca5;--accent:#72195a;--card:#fff;--line:#c6d8ff;--ph:#c6d8ff}
@media(prefers-color-scheme:dark){:root:not([data-theme=light]){--bg:#1a0a14;--fg:#c6d8ff;--muted:#71a9f7;--accent:#71a9f7;--card:#2a1020;--line:#4c1036;--ph:#4c1036}}
:root[data-theme=dark]{--bg:#1a0a14;--fg:#c6d8ff;--muted:#71a9f7;--accent:#71a9f7;--card:#2a1020;--line:#4c1036;--ph:#4c1036}
*{box-sizing:border-box}
body{font:16px/1.5 system-ui,sans-serif;max-width:64rem;margin:0 auto;padding:1.5rem 1rem 4rem;background:var(--bg);color:var(--fg)}
a{color:var(--accent)}
nav.tabs{display:flex;gap:.25rem;margin:.5rem 0 1rem;border-bottom:1px solid var(--line)}
nav.tabs a{padding:.4rem .9rem;border-radius:.4rem .4rem 0 0;text-decoration:none;color:var(--muted)}
nav.tabs a[aria-current]{color:var(--fg);font-weight:600;border:1px solid var(--line);border-bottom-color:var(--bg);margin-bottom:-1px}
header.top{display:flex;justify-content:space-between;align-items:center;gap:1rem;flex-wrap:wrap}
h1{margin:.2em 0}
#controls[hidden]{display:none}
#controls{display:flex;gap:.5rem;flex-wrap:wrap;margin:1rem 0}
#controls input,#controls select,#theme,button.more{font:inherit;padding:.4rem .6rem;border:1px solid var(--line);border-radius:.4rem;background:var(--card);color:var(--fg)}
#controls input{flex:1 1 12rem}
#count{color:var(--muted);font-size:.9rem;margin:.25rem 0 1rem}
ul.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(9rem,1fr));gap:1rem;padding:0;list-style:none}
.grid img,.grid .ph{width:100%;height:auto;aspect-ratio:2/3;object-fit:cover;background:var(--ph);border-radius:.3rem;display:block}
.grid li{font-size:.85rem}
.grid a{text-decoration:none;color:inherit}
.grid small{color:var(--muted)}
button.more{display:block;margin:1.5rem auto;cursor:pointer}
img.cover{max-width:16rem;width:100%;height:auto;aspect-ratio:2/3;object-fit:cover;border-radius:.3rem}
.bar{display:flex;gap:1rem;align-items:center;flex-wrap:wrap;margin:.5rem 0}
.bar .grow{flex:1}
input,select,textarea,button{font:inherit;padding:.4rem .6rem;border:1px solid var(--line);border-radius:.4rem;background:var(--card);color:var(--fg)}
button{cursor:pointer}
button.danger{border-color:#b3261e;color:#b3261e}
label{display:block;margin:.5rem 0}
label input,label select,label textarea{display:block;margin-top:.2rem}
dialog{background:var(--card);color:var(--fg);border:1px solid var(--line);border-radius:.5rem;max-width:26rem}
dialog::backdrop{background:rgba(0,0,0,.5)}
ul.plain{padding:0;list-style:none}
ul.plain li{margin:.5rem 0}
ul#results li{display:flex;align-items:center;gap:.5rem}
fieldset{border:1px solid var(--line);border-radius:.4rem;margin:1rem 0}
.row{display:flex;gap:.5rem;flex-wrap:wrap;align-items:center}
.ph{background:var(--ph) url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%236b5ca5' stroke-width='1.2' stroke-linecap='round' stroke-linejoin='round'%3E%3Crect x='3' y='3' width='18' height='18' rx='2'/%3E%3Ccircle cx='9' cy='9' r='1.5'/%3E%3Cpath d='M21 15l-5-5L5 21'/%3E%3C/svg%3E") center/34% no-repeat}
.ph.cover{display:block;max-width:16rem;width:100%;aspect-ratio:2/3;border-radius:.3rem}
.edit{display:grid;gap:1.5rem;grid-template-columns:1fr;margin-top:1rem}
@media(min-width:48rem){.edit{grid-template-columns:16rem 1fr;align-items:start}}
.fields{display:grid;grid-template-columns:repeat(auto-fit,minmax(11rem,1fr));gap:.25rem 1rem}
.fields .wide{grid-column:1/-1}
.fields input,.fields select,.fields textarea{width:100%}
#coverbox details{margin:.75rem 0}
#coverbox input{width:100%;min-width:0}
#msg{position:fixed;left:50%;bottom:1rem;transform:translateX(-50%);max-width:90vw;margin:0;padding:.6rem 1rem;border-radius:.5rem;background:var(--card);color:var(--fg);border:1px solid var(--line);box-shadow:0 2px 10px rgba(0,0,0,.25);z-index:10;pointer-events:none}
#msg:empty{display:none}
#msg.ok{border-color:#2e7d32}
#msg.err{border-color:#b3261e;color:#b3261e}
#msg.busy::before{content:"";display:inline-block;width:.8em;height:.8em;margin-right:.5em;border:2px solid var(--line);border-top-color:var(--accent);border-radius:50%;animation:spin .8s linear infinite}
@keyframes spin{to{transform:rotate(360deg)}}
[aria-busy=true]{opacity:.7}
"#;

/// Runs before first paint so a saved theme never flashes.
pub const THEME_INIT: &str =
    "try{var t=localStorage.getItem('theme');if(t)document.documentElement.dataset.theme=t}catch(e){}";

pub const GRID_JS: &str = r#"
(()=>{
const $=s=>document.querySelector(s);
const grid=$('#grid'); if(!grid) return;
const STEP=60;
// Keep in step with rating_emoji() and month_year() below in this file's Rust helpers.
const emoji=n=>['😖😖','😖','🙁🙁','🙁','😐','🙂','😄','😄😄','😍','🤩'][Math.min(10,Math.max(1,n))-1];
const month=d=>{const m=+d.slice(5,7); return (m>=1&&m<=12?['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec'][m-1]+' ':'')+d.slice(0,4)};
const sorts={
 recent:(a,b)=>b.upd.localeCompare(a.upd),
 logged:(a,b)=>(b.date||'').localeCompare(a.date||'')||b.upd.localeCompare(a.upd),
 title:(a,b)=>a.title.localeCompare(b.title),
 year:(a,b)=>(b.year||0)-(a.year||0),
 rating:(a,b)=>(b.rating||0)-(a.rating||0)};
fetch('/index.json').then(r=>r.ok?r.json():Promise.reject(r.status)).then(d=>{
 const items=d.items.map(a=>({kind:a[0],slug:a[1],title:a[2],status:a[3],rating:a[4],year:a[5],cover:a[6],date:a[7],upd:a[8],lc:(a[2]+' '+(a[9]||'')).toLowerCase()}));
 const f=$('#controls'), q=f.q, kind=f.kind, sort=f.sort, status=grid.dataset.status;
 const p=new URLSearchParams(location.search);
 q.value=p.get('q')||''; kind.value=p.get('kind')||''; sort.value=sorts[p.get('sort')]?p.get('sort'):'logged';
 f.hidden=false; for(const id of ['#nav','#pager']){const n=$(id); if(n) n.hidden=true}
 let shown=STEP;
 const more=document.createElement('button'); more.className='more'; more.type='button'; more.textContent='Show more';
 more.onclick=()=>{shown+=STEP;draw()}; grid.after(more);
 function draw(){
  const t=q.value.trim().toLowerCase();
  const list=items.filter(e=>(!kind.value||e.kind===kind.value)&&e.status===status&&(!t||e.lc.includes(t))).sort(sorts[sort.value]);
  const u=new URLSearchParams(); if(t)u.set('q',q.value.trim()); if(kind.value)u.set('kind',kind.value); if(sort.value!=='logged')u.set('sort',sort.value);
  history.replaceState(null,'',u.toString()?'?'+u:location.pathname);
  $('#count').textContent=list.length+(list.length===1?' entry':' entries');
  grid.replaceChildren(...list.slice(0,shown).map(e=>{
   const li=document.createElement('li'), a=document.createElement('a');
   a.href='/entry/'+e.kind+'/'+encodeURIComponent(e.slug);
   let img; if(e.cover){img=document.createElement('img');img.src=d.covers+e.cover;img.alt='';img.loading='lazy';img.width=300;img.height=450}
   else{img=document.createElement('div');img.className='ph'}
   const t2=document.createElement('div'); t2.textContent=e.title+(e.year?' ('+e.year+')':'');
   a.append(img,t2);
   const s=document.createElement('small'); s.append(e.kind);
   if(e.rating){const r=document.createElement('span'); r.textContent=emoji(e.rating); r.title=e.rating+'/10'; r.setAttribute('role','img'); r.setAttribute('aria-label',e.rating+' out of 10'); s.append(' · ',r)}
   if(e.date) s.append(' · '+month(e.date));
   li.append(a,s); return li}));
  more.hidden=list.length<=shown;
 }
 let timer; q.oninput=()=>{clearTimeout(timer);timer=setTimeout(()=>{shown=STEP;draw()},120)};
 for(const c of [kind,sort]) c.onchange=()=>{shown=STEP;draw()};
 f.onsubmit=e=>e.preventDefault();
 draw();
}).catch(()=>{});
})();
"#;

/// Theme picker wiring (system / light / dark), shared by public and admin pages.
pub const THEME_JS: &str = r#"
(()=>{const ts=document.querySelector('#theme'),root=document.documentElement;
if(ts){ts.value=localStorage.getItem('theme')||'auto';ts.onchange=()=>{const v=ts.value;
 if(v==='auto'){localStorage.removeItem('theme');root.removeAttribute('data-theme')}
 else{localStorage.setItem('theme',v);root.setAttribute('data-theme',v)}}}})();
"#;

pub fn theme_picker() -> maud::Markup {
    maud::html! {
        select #theme aria-label="Theme" {
            option value="auto" { "Theme: system" }
            option value="light" { "Light" }
            option value="dark" { "Dark" }
        }
    }
}

/// Palette-coloured book icon; one SVG serves as favicon and manifest icon.
pub const FAVICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="12" fill="#4c1036"/><rect x="16" y="12" width="32" height="40" rx="3" fill="#c6d8ff"/><rect x="16" y="12" width="8" height="40" rx="3" fill="#6b5ca5"/><rect x="29" y="22" width="14" height="3" fill="#72195a"/><rect x="29" y="30" width="10" height="3" fill="#71a9f7"/></svg>"##;

pub const APPLE_TOUCH_ICON: &[u8] = include_bytes!("icons/apple-touch-icon.png");
pub const ICON_192: &[u8] = include_bytes!("icons/icon-192.png");
pub const ICON_512: &[u8] = include_bytes!("icons/icon-512.png");

pub const MANIFEST: &str = r##"{"name":"catalogged","short_name":"catalogged","start_url":"/","display":"standalone","background_color":"#f6f8ff","theme_color":"#4c1036","icons":[{"src":"/favicon.svg","sizes":"any","type":"image/svg+xml","purpose":"any"},{"src":"/icon-192.png","sizes":"192x192","type":"image/png"},{"src":"/icon-512.png","sizes":"512x512","type":"image/png"}]}"##;

/// Head links for the favicon and web manifest (routes in lib.rs).
pub const FAVICON_LINKS: &str = r##"<link rel="icon" href="/favicon.svg" type="image/svg+xml"><link rel="apple-touch-icon" href="/apple-touch-icon.png"><link rel="manifest" href="/manifest.webmanifest"><meta name="theme-color" content="#4c1036">"##;

/// The 1 to 10 rating as faces: 1 😖😖, 2 😖, 3 🙁🙁, 4 🙁, 5 😐, 6 🙂, 7 😄, 8 😄😄, 9 😍, 10 🤩.
/// Mirrors `emoji` in the grid script.
pub fn rating_emoji(r: u8) -> &'static str {
    ["😖😖", "😖", "🙁🙁", "🙁", "😐", "🙂", "😄", "😄😄", "😍", "🤩"][usize::from(r.clamp(1, 10) - 1)]
}

/// The path of the public list for a status.
pub fn status_path(s: Status) -> &'static str {
    match s {
        Status::Consumed => "/",
        Status::InProgress => "/in-progress",
        Status::Wanted => "/wanted",
    }
}

/// The top navigation between the three lists (plain links, so it works without JavaScript).
pub fn status_tabs(current: Status) -> maud::Markup {
    maud::html! {
        nav.tabs aria-label="Lists" {
            @for (s, label) in [(Status::Consumed, "Consumed"), (Status::InProgress, "In progress"), (Status::Wanted, "Wanted")] {
                a href=(status_path(s)) aria-current=[(s == current).then_some("page")] { (label) }
            }
        }
    }
}

/// `2015-03-19` as `Mar 2015`. The log never tracked the day before 2016, so only month and year are shown.
/// Anything that is not a `YYYY-MM-..` date is returned unchanged.
pub fn month_year(date: &str) -> String {
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    match (date.get(0..4), date.get(5..7).and_then(|m| m.parse::<usize>().ok())) {
        (Some(y), Some(m)) if (1..=12).contains(&m) => format!("{} {y}", M[m - 1]),
        _ => date.to_string(),
    }
}

/// The small line under a card or on a detail page: kind, a status label (detail pages only, and not
/// the assumed `consumed`), the rating as a face, and when it was logged.
pub fn meta_line(kind: &str, status: Option<&str>, rating: Option<u8>, consumed_at: Option<&str>) -> maud::Markup {
    maud::html! {
        (kind)
        @if let Some(st) = status.filter(|s| *s != "consumed") { " · " (st.replace('_', " ")) }
        @if let Some(r) = rating { " · " span role="img" title={(r) "/10"} aria-label={(r) " out of 10"} { (rating_emoji(r)) } }
        @if let Some(d) = consumed_at.filter(|d| !d.is_empty()) { " · " (month_year(d)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_cover_the_whole_scale() {
        let faces: Vec<_> = (1..=10).map(rating_emoji).collect();
        assert_eq!(faces, ["😖😖", "😖", "🙁🙁", "🙁", "😐", "🙂", "😄", "😄😄", "😍", "🤩"]);
    }

    #[test]
    fn dates_show_month_and_year() {
        assert_eq!(month_year("2015-03-19"), "Mar 2015");
        assert_eq!(month_year("2024-12-01"), "Dec 2024");
        assert_eq!(month_year("2020"), "2020");
        assert_eq!(month_year("2020-13-01"), "2020-13-01");
    }
}
