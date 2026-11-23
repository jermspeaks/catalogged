use anyhow::{bail, Context, Result};
use catalogged_core::{tmdb, EditEntry, Entry, Kind, NewEntry, Source, Status, MAX_COVER_BYTES};
use clap::{Parser, Subcommand};
use serde::de::DeserializeOwned;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "catalogged", about = "Add entries to the catalogue")]
struct Cli {
    /// Base URL of the deployed Worker, e.g. https://catalogged.example.workers.dev
    #[arg(long, env = "CATALOGGED_URL", global = true)]
    url: Option<String>,
    /// Cloudflare Access service token (client id)
    #[arg(long, env = "CF_SERVICE_CLIENT_ID", global = true, hide_env_values = true)]
    access_id: Option<String>,
    /// Cloudflare Access service token (client secret)
    #[arg(long, env = "CF_SERVICE_CLIENT_SECRET", global = true, hide_env_values = true)]
    access_secret: Option<String>,
    /// TMDB v4 read token or v3 key, for metadata and covers
    #[arg(long, env = "TMDB_API_KEY", global = true, hide_env_values = true)]
    tmdb_key: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Add a single entry
    Add {
        /// game | book | show | movie
        #[arg(long)]
        kind: String,
        /// Optional when --source tmdb --external-id is given (fetched from TMDB)
        #[arg(long)]
        title: Option<String>,
        /// wanted | in_progress | consumed
        #[arg(long, default_value = "wanted")]
        status: String,
        #[arg(long)]
        rating: Option<u8>,
        #[arg(long)]
        year: Option<i32>,
        /// Your review text
        #[arg(long)]
        review: Option<String>,
        /// Date logged/watched, YYYY-MM-DD (defaults to today when status is consumed)
        #[arg(long)]
        date: Option<String>,
        /// tmdb | igdb | open_library
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        external_id: Option<String>,
        /// Skip fetching and uploading the cover
        #[arg(long)]
        no_cover: bool,
    },
    /// Edit an entry's fields (only the flags you pass change). Empty string clears --review/--date; --rating 0 clears the rating
    Edit {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        slug: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        rating: Option<u8>,
        #[arg(long)]
        review: Option<String>,
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        year: Option<i32>,
    },
    /// Delete an entry (and its cover)
    Delete {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        slug: String,
        /// Required: deleting cannot be undone
        #[arg(long)]
        yes: bool,
    },
    /// Search TMDB for movies or shows and print their ids
    Search {
        /// movie | show
        #[arg(long)]
        kind: String,
        query: String,
    },
    /// Bulk-add entries from a CSV (see docs/seed-example.csv). Columns, all optional except that a
    /// row needs a title or an external_id: kind,title,status,rating,review,year,source,external_id,consumed_at.
    /// An external_id like tt0133093 is an IMDb id: kind and title are looked up on TMDB.
    /// Blank status means consumed; a blank consumed_at stays blank (imports never default to today).
    Import {
        file: PathBuf,
        /// Resolve ids and print what would be added; send nothing to the Worker
        #[arg(long)]
        dry_run: bool,
        /// Treat "already exists" (409) as skipped instead of failed, so a run can be repeated safely
        #[arg(long)]
        skip_existing: bool,
        /// Skip fetching and uploading covers
        #[arg(long)]
        no_cover: bool,
    },
}

fn today_if_consumed(status: Status) -> Option<String> {
    (status == Status::Consumed).then(|| chrono::Local::now().format("%Y-%m-%d").to_string())
}

fn parse<T: DeserializeOwned>(s: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(s.to_owned()))
        .with_context(|| format!("invalid value {s:?}"))
}

/// The Worker answered 409: this id or slug already exists.
#[derive(Debug)]
struct Conflict(String);

impl std::fmt::Display for Conflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "already exists: {}", self.0)
    }
}

impl std::error::Error for Conflict {}

/// One CSV row. Looser than `NewEntry` so a sheet can be just ids.
#[derive(serde::Deserialize)]
struct Row {
    kind: Option<Kind>,
    #[serde(default)]
    title: String,
    status: Option<Status>,
    rating: Option<u8>,
    #[serde(default, alias = "notes")]
    review: Option<String>,
    year: Option<i32>,
    source: Option<Source>,
    external_id: Option<String>,
    #[serde(default)]
    consumed_at: Option<String>,
}

struct Client {
    http: reqwest::blocking::Client,
    url: String,
    headers: reqwest::header::HeaderMap,
}

impl Client {
    fn new(cli: &Cli) -> Result<Self> {
        let url = cli.url.clone().context("set --url or CATALOGGED_URL")?;
        let mut headers = reqwest::header::HeaderMap::new();
        if let (Some(id), Some(secret)) = (&cli.access_id, &cli.access_secret) {
            headers.insert("CF-Access-Client-Id", id.parse()?);
            headers.insert("CF-Access-Client-Secret", secret.parse()?);
        }
        Ok(Self { // Never follow redirects: an Access login redirect would otherwise look like success.
            http: reqwest::blocking::Client::builder().redirect(reqwest::redirect::Policy::none()).build()?, url: url.trim_end_matches('/').into(), headers })
    }

    fn create(&self, e: &NewEntry) -> Result<String> {
        let res = self
            .http
            .post(format!("{}/admin/api/entries", self.url))
            .headers(self.headers.clone())
            .json(e)
            .send()?;
        if res.status().is_redirection() {
            bail!("{} redirect: Cloudflare Access rejected the credentials (check the service token and policy)", res.status());
        }
        if res.status() == reqwest::StatusCode::CONFLICT {
            return Err(Conflict(res.text().unwrap_or_default()).into());
        }
        if !res.status().is_success() {
            bail!("{}: {}", res.status(), res.text().unwrap_or_default());
        }
        #[derive(serde::Deserialize)]
        struct Created {
            slug: String,
        }
        Ok(res.json::<Created>()?.slug)
    }

    fn entry_url(&self, kind: Kind, slug: &str) -> String {
        format!("{}/admin/api/entries/{}/{slug}", self.url, kind.as_str())
    }

    fn check(res: reqwest::blocking::Response) -> Result<reqwest::blocking::Response> {
        if res.status().is_redirection() {
            bail!("{} redirect: Cloudflare Access rejected the credentials", res.status());
        }
        if !res.status().is_success() {
            bail!("{}: {}", res.status(), res.text().unwrap_or_default());
        }
        Ok(res)
    }

    fn get_entry(&self, kind: Kind, slug: &str) -> Result<Entry> {
        let res = self.http.get(self.entry_url(kind, slug)).headers(self.headers.clone()).send()?;
        Ok(Self::check(res)?.json()?)
    }

    fn edit_entry(&self, kind: Kind, slug: &str, e: &EditEntry) -> Result<()> {
        let res = self.http.put(self.entry_url(kind, slug)).headers(self.headers.clone()).json(e).send()?;
        Self::check(res).map(drop)
    }

    fn delete_entry(&self, kind: Kind, slug: &str) -> Result<()> {
        let res = self.http.delete(self.entry_url(kind, slug)).headers(self.headers.clone()).send()?;
        Self::check(res).map(drop)
    }

    fn put_cover(&self, kind: Kind, slug: &str, webp: Vec<u8>) -> Result<()> {
        let res = self
            .http
            .put(format!("{}/admin/api/entries/{}/{slug}/cover", self.url, kind.as_str()))
            .headers(self.headers.clone())
            .header("content-type", "image/webp")
            .body(webp)
            .send()?;
        if res.status().is_redirection() {
            bail!("{} redirect: Cloudflare Access rejected the credentials", res.status());
        }
        if !res.status().is_success() {
            bail!("cover upload {}: {}", res.status(), res.text().unwrap_or_default());
        }
        Ok(())
    }
}

/// Talks to TMDB directly (the key stays on this machine) and builds WebP covers.
struct Tmdb {
    http: reqwest::blocking::Client,
    key: String,
}

impl Tmdb {
    fn new(key: Option<&String>) -> Result<Self> {
        let key = key.cloned().context("set TMDB_API_KEY (or --tmdb-key)")?;
        // TMDB can be slow; don't hang forever.
        let http = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(30)).build()?;
        Ok(Self { http, key })
    }

    fn get(&self, url: String) -> Result<reqwest::blocking::Response> {
        let (url, auth) = tmdb::authenticate(url, &self.key);
        let mut req = self.http.get(url);
        if let Some(a) = auth {
            req = req.header("Authorization", a);
        }
        let res = req.send()?;
        if !res.status().is_success() {
            bail!("TMDB returned {}", res.status());
        }
        Ok(res)
    }

    fn search(&self, kind: Kind, q: &str) -> Result<Vec<tmdb::Candidate>> {
        let url = tmdb::search_url(kind, q).context("TMDB only supports movie and show")?;
        Ok(tmdb::parse_search(&self.get(url)?.text()?)?)
    }

    /// IMDb id -> (kind, TMDB record).
    fn find(&self, imdb_id: &str) -> Result<(Kind, tmdb::Candidate)> {
        tmdb::parse_find(&self.get(tmdb::find_url(imdb_id))?.text()?)?
            .with_context(|| format!("TMDB has no movie or show for {imdb_id}"))
    }

    fn details(&self, kind: Kind, id: &str) -> Result<tmdb::Candidate> {
        let url = tmdb::details_url(kind, id).context("TMDB only supports movie and show")?;
        Ok(tmdb::parse_details(&self.get(url)?.text()?)?)
    }

    /// Downloads the poster and re-encodes it as a WebP under the free-tier size cap.
    fn cover_webp(&self, poster_path: &str) -> Result<Vec<u8>> {
        let bytes = self.http.get(tmdb::poster_url(poster_path)).send()?.error_for_status()?.bytes()?;
        let img = image::load_from_memory(&bytes).context("decoding poster")?;
        let enc = webp::Encoder::from_image(&img).map_err(|e| anyhow::anyhow!("webp: {e}"))?;
        for quality in [82.0, 70.0, 55.0, 40.0] {
            let out = enc.encode(quality).to_vec();
            if out.len() <= MAX_COVER_BYTES {
                return Ok(out);
            }
        }
        bail!("could not encode a WebP under {MAX_COVER_BYTES} bytes")
    }
}

/// Fill in blanks from TMDB and return the poster path (if any) for the cover step.
fn enrich(tmdb: &Tmdb, e: &mut NewEntry) -> Result<Option<String>> {
    let (Some(Source::Tmdb), Some(id)) = (e.source, e.external_id.clone()) else {
        if e.title.trim().is_empty() {
            bail!("title is required unless --source tmdb with an --external-id is given");
        }
        return Ok(None);
    };
    let found = tmdb.details(e.kind, &id)?;
    if e.title.trim().is_empty() {
        e.title = found.title;
    }
    e.year = e.year.or(found.year);
    Ok(found.poster_path)
}

/// Turn a sheet row into a full entry, resolving IMDb ids through TMDB.
fn resolve_row(tmdb: Option<&Tmdb>, row: Row) -> Result<NewEntry> {
    let mut kind = row.kind;
    let mut source = row.source;
    let mut external_id = row.external_id.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    let mut title = row.title;
    let mut year = row.year;
    if let Some(id) = external_id.clone() {
        if tmdb::is_imdb_id(&id) {
            let t = tmdb.context("IMDb ids need TMDB_API_KEY to resolve")?;
            let (found_kind, c) = t.find(&id)?;
            if kind.is_some_and(|k| k != found_kind) {
                bail!("{id} is a {}, but the row says {}", found_kind.as_str(), kind.map(|k| k.as_str()).unwrap_or(""));
            }
            kind = Some(found_kind);
            source = Some(Source::Tmdb);
            external_id = Some(c.id);
            if title.trim().is_empty() {
                title = c.title;
            }
            year = year.or(c.year);
        } else if source.is_none() {
            source = Some(Source::Tmdb);
        }
    }
    let kind = kind.context("kind is required unless external_id is an IMDb id (tt...)")?;
    if let (true, Some(Source::Tmdb), Some(id), Some(t)) = (title.trim().is_empty(), source, &external_id, tmdb) {
        let c = t.details(kind, id)?;
        title = c.title;
        year = year.or(c.year);
    }
    if title.trim().is_empty() && external_id.is_none() {
        bail!("a row needs a title or an external_id");
    }
    Ok(NewEntry {
        kind,
        title,
        status: row.status.unwrap_or(Status::Consumed),
        rating: row.rating.filter(|r| *r != 0),
        review: row.review.filter(|r| !r.is_empty()),
        year,
        source,
        external_id,
        consumed_at: row.consumed_at.filter(|d| !d.is_empty()),
    })
}

/// Create the entry, then (best effort) upload its cover. A cover failure never loses the entry.
fn add_one(client: &Client, tmdb: Option<&Tmdb>, mut e: NewEntry, covers: bool) -> Result<String> {
    let poster = match (tmdb, e.source) {
        (Some(t), Some(Source::Tmdb)) => enrich(t, &mut e)?,
        _ => {
            if e.title.trim().is_empty() {
                bail!("title is required");
            }
            None
        }
    };
    let slug = client.create(&e)?;
    if let (true, Some(t), Some(p)) = (covers, tmdb, poster) {
        match t.cover_webp(&p).and_then(|w| client.put_cover(e.kind, &slug, w)) {
            Ok(()) => {}
            Err(err) => eprintln!("warning: {} was added but its cover failed: {err:#}", e.title),
        }
    }
    Ok(e.title)
}

fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();
    match &cli.cmd {
        Cmd::Add { kind, title, status, rating, year, review, date, source, external_id, no_cover } => {
            let status = parse::<Status>(status)?;
            let entry = NewEntry {
                kind: parse::<Kind>(kind)?,
                title: title.clone().unwrap_or_default(),
                status,
                rating: *rating,
                review: review.clone(),
                year: *year,
                source: source.as_deref().map(parse::<Source>).transpose()?,
                external_id: external_id.clone(),
                consumed_at: date.clone().or_else(|| today_if_consumed(status)),
            };
            let tmdb = Tmdb::new(cli.tmdb_key.as_ref()).ok();
            let title = add_one(&Client::new(&cli)?, tmdb.as_ref(), entry, !no_cover)?;
            println!("added {title}");
        }
        Cmd::Edit { kind, slug, title, status, rating, review, date, year } => {
            let kind = parse::<Kind>(kind)?;
            let client = Client::new(&cli)?;
            let cur = client.get_entry(kind, slug)?;
            let blank = |s: &Option<String>| s.as_ref().map(|v| if v.is_empty() { None } else { Some(v.clone()) });
            let edit = EditEntry {
                title: title.clone().unwrap_or(cur.title),
                status: status.as_deref().map(parse::<Status>).transpose()?.unwrap_or(cur.status),
                rating: rating.map(|r| if r == 0 { None } else { Some(r) }).unwrap_or(cur.rating),
                review: blank(review).unwrap_or(cur.review),
                year: year.or(cur.year),
                consumed_at: blank(date).unwrap_or(cur.consumed_at),
            };
            edit.validate().map_err(anyhow::Error::msg)?;
            client.edit_entry(kind, slug, &edit)?;
            println!("updated {}", edit.title);
        }
        Cmd::Delete { kind, slug, yes } => {
            if !yes {
                bail!("refusing to delete without --yes (this cannot be undone)");
            }
            Client::new(&cli)?.delete_entry(parse::<Kind>(kind)?, slug)?;
            println!("deleted {slug}");
        }
        Cmd::Search { kind, query } => {
            let kind = parse::<Kind>(kind)?;
            for c in Tmdb::new(cli.tmdb_key.as_ref())?.search(kind, query)?.into_iter().take(10) {
                println!("{:>8}  {}{}", c.id, c.title, c.year.map(|y| format!(" ({y})")).unwrap_or_default());
            }
        }
        Cmd::Import { file, dry_run, skip_existing, no_cover } => {
            let mut rdr = csv::Reader::from_path(file)?;
            let rows = rdr
                .deserialize::<Row>()
                .enumerate()
                .map(|(i, r)| r.with_context(|| format!("row {}", i + 2)))
                .collect::<Result<Vec<_>>>()?;
            println!("read {} rows", rows.len());
            let tmdb = Tmdb::new(cli.tmdb_key.as_ref()).ok();
            let client = if *dry_run { None } else { Some(Client::new(&cli)?) };
            let (mut added, mut skipped) = (0, 0);
            let mut failures: Vec<String> = vec![];
            for (i, row) in rows.into_iter().enumerate() {
                let line = i + 2;
                let entry = match resolve_row(tmdb.as_ref(), row).and_then(|e| e.validate().map_err(anyhow::Error::msg).map(|_| e)) {
                    Ok(e) => e,
                    Err(err) => {
                        eprintln!("FAIL  row {line}: {err:#}");
                        failures.push(format!("row {line}: {err:#}"));
                        continue;
                    }
                };
                let label = format!("{} ({} {})", entry.title, entry.kind.as_str(), entry.year.map(|y| y.to_string()).unwrap_or_default());
                let Some(client) = &client else {
                    println!("would add  {label}");
                    continue;
                };
                match add_one(client, tmdb.as_ref(), entry, !no_cover) {
                    Ok(title) => {
                        added += 1;
                        println!("ok    {title}");
                    }
                    Err(err) if *skip_existing && err.downcast_ref::<Conflict>().is_some() => {
                        skipped += 1;
                        println!("skip  {label}");
                    }
                    Err(err) => {
                        eprintln!("FAIL  row {line} {label}: {err:#}");
                        failures.push(format!("row {line} {label}: {err:#}"));
                    }
                }
                // Be polite to TMDB and the Worker; also keeps a big import from bursting.
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            println!("added {added}, skipped {skipped}, failed {}", failures.len());
            if !failures.is_empty() {
                bail!("{} rows failed:\n  {}", failures.len(), failures.join("\n  "));
            }
        }
    }
    Ok(())
}
