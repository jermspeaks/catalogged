//! Styles and client script for the public site. No framework: a small script enhances the
//! server-rendered grid by loading the cached `/index.json` once and filtering locally, so
//! search, filter and sort cost zero D1 reads.

/// Palette from docs/discovery.md: #c6d8ff #71a9f7 #6b5ca5 #72195a #4c1036.
pub const CSS: &str = r#"
:root{--bg:#f6f8ff;--fg:#4c1036;--muted:#6b5ca5;--accent:#72195a;--card:#fff;--line:#c6d8ff;--ph:#c6d8ff}
@media(prefers-color-scheme:dark){:root:not([data-theme=light]){--bg:#1a0a14;--fg:#c6d8ff;--muted:#71a9f7;--accent:#71a9f7;--card:#2a1020;--line:#4c1036;--ph:#4c1036}}
:root[data-theme=dark]{--bg:#1a0a14;--fg:#c6d8ff;--muted:#71a9f7;--accent:#71a9f7;--card:#2a1020;--line:#4c1036;--ph:#4c1036}
*{box-sizing:border-box}
body{font:16px/1.5 system-ui,sans-serif;max-width:64rem;margin:0 auto;padding:1.5rem 1rem 4rem;background:var(--bg);color:var(--fg)}
a{color:var(--accent)}
header.top{display:flex;justify-content:space-between;align-items:center;gap:1rem;flex-wrap:wrap}
h1{margin:.2em 0}
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
"#;

/// Runs before first paint so a saved theme never flashes.
pub const THEME_INIT: &str =
    "try{var t=localStorage.getItem('theme');if(t)document.documentElement.dataset.theme=t}catch(e){}";

pub const GRID_JS: &str = r#"
(()=>{
const $=s=>document.querySelector(s);
const grid=$('#grid'); if(!grid) return;
const STEP=60;
const sorts={
 recent:(a,b)=>b.upd.localeCompare(a.upd),
 logged:(a,b)=>(b.date||'').localeCompare(a.date||''),
 title:(a,b)=>a.title.localeCompare(b.title),
 year:(a,b)=>(b.year||0)-(a.year||0),
 rating:(a,b)=>(b.rating||0)-(a.rating||0)};
fetch('/index.json').then(r=>r.ok?r.json():Promise.reject(r.status)).then(d=>{
 const items=d.items.map(a=>({kind:a[0],slug:a[1],title:a[2],status:a[3],rating:a[4],year:a[5],cover:a[6],date:a[7],upd:a[8],lc:a[2].toLowerCase()}));
 const f=$('#controls'), q=f.q, kind=f.kind, status=f.status, sort=f.sort;
 const p=new URLSearchParams(location.search);
 q.value=p.get('q')||''; kind.value=p.get('kind')||''; status.value=p.get('status')||''; sort.value=sorts[p.get('sort')]?p.get('sort'):'recent';
 f.hidden=false; for(const id of ['#nav','#pager']){const n=$(id); if(n) n.hidden=true}
 let shown=STEP;
 const more=document.createElement('button'); more.className='more'; more.type='button'; more.textContent='Show more';
 more.onclick=()=>{shown+=STEP;draw()}; grid.after(more);
 function draw(){
  const t=q.value.trim().toLowerCase();
  const list=items.filter(e=>(!kind.value||e.kind===kind.value)&&(!status.value||e.status===status.value)&&(!t||e.lc.includes(t))).sort(sorts[sort.value]);
  const u=new URLSearchParams(); if(t)u.set('q',q.value.trim()); if(kind.value)u.set('kind',kind.value); if(status.value)u.set('status',status.value); if(sort.value!=='recent')u.set('sort',sort.value);
  history.replaceState(null,'',u.toString()?'?'+u:location.pathname);
  $('#count').textContent=list.length+(list.length===1?' entry':' entries');
  grid.replaceChildren(...list.slice(0,shown).map(e=>{
   const li=document.createElement('li'), a=document.createElement('a');
   a.href='/entry/'+e.kind+'/'+encodeURIComponent(e.slug);
   let img; if(e.cover){img=document.createElement('img');img.src=d.covers+e.cover;img.alt='';img.loading='lazy';img.width=300;img.height=450}
   else{img=document.createElement('div');img.className='ph'}
   const t2=document.createElement('div'); t2.textContent=e.title+(e.year?' ('+e.year+')':'');
   a.append(img,t2);
   const s=document.createElement('small'); s.textContent=e.kind+' · '+e.status.replace('_',' ')+(e.rating?' · '+e.rating+'/10':'');
   li.append(a,s); return li}));
  more.hidden=list.length<=shown;
 }
 let timer; q.oninput=()=>{clearTimeout(timer);timer=setTimeout(()=>{shown=STEP;draw()},120)};
 for(const c of [kind,status,sort]) c.onchange=()=>{shown=STEP;draw()};
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

pub const MANIFEST: &str = r##"{"name":"catalogged","short_name":"catalogged","start_url":"/","display":"standalone","background_color":"#f6f8ff","theme_color":"#4c1036","icons":[{"src":"/favicon.svg","sizes":"any","type":"image/svg+xml","purpose":"any"}]}"##;

/// Head links for the favicon and web manifest (routes in lib.rs).
pub const FAVICON_LINKS: &str = r##"<link rel="icon" href="/favicon.svg" type="image/svg+xml"><link rel="manifest" href="/manifest.webmanifest"><meta name="theme-color" content="#4c1036">"##;
