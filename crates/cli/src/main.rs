use anyhow::{bail, Context, Result};
use catalogged_core::{tmdb, Kind, NewEntry, Source, Status, MAX_COVER_BYTES};
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
        #[arg(long)]
        notes: Option<String>,
        /// tmdb | igdb | open_library
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        external_id: Option<String>,
        /// Skip fetching and uploading the cover
        #[arg(long)]
        no_cover: bool,
    },
    /// Search TMDB for movies or shows and print their ids
    Search {
        /// movie | show
        #[arg(long)]
        kind: String,
        query: String,
    },
    /// Bulk-add entries from a CSV with columns:
    /// kind,title,status,rating,notes,year,source,external_id
    Import {
        file: PathBuf,
        /// Parse and validate only; send nothing
        #[arg(long)]
        dry_run: bool,
        /// Skip fetching and uploading covers
        #[arg(long)]
        no_cover: bool,
    },
}

fn parse<T: DeserializeOwned>(s: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(s.to_owned()))
        .with_context(|| format!("invalid value {s:?}"))
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
        if !res.status().is_success() {
            bail!("{}: {}", res.status(), res.text().unwrap_or_default());
        }
        #[derive(serde::Deserialize)]
        struct Created {
            slug: String,
        }
        Ok(res.json::<Created>()?.slug)
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
        Cmd::Add { kind, title, status, rating, year, notes, source, external_id, no_cover } => {
            let entry = NewEntry {
                kind: parse::<Kind>(kind)?,
                title: title.clone().unwrap_or_default(),
                status: parse::<Status>(status)?,
                rating: *rating,
                notes: notes.clone(),
                year: *year,
                source: source.as_deref().map(parse::<Source>).transpose()?,
                external_id: external_id.clone(),
            };
            let tmdb = Tmdb::new(cli.tmdb_key.as_ref()).ok();
            let title = add_one(&Client::new(&cli)?, tmdb.as_ref(), entry, !no_cover)?;
            println!("added {title}");
        }
        Cmd::Search { kind, query } => {
            let kind = parse::<Kind>(kind)?;
            for c in Tmdb::new(cli.tmdb_key.as_ref())?.search(kind, query)?.into_iter().take(10) {
                println!("{:>8}  {}{}", c.id, c.title, c.year.map(|y| format!(" ({y})")).unwrap_or_default());
            }
        }
        Cmd::Import { file, dry_run, no_cover } => {
            let mut rdr = csv::Reader::from_path(file)?;
            let entries = rdr
                .deserialize::<NewEntry>()
                .enumerate()
                .map(|(i, r)| r.with_context(|| format!("row {}", i + 2)))
                .collect::<Result<Vec<_>>>()?;
            println!("parsed {} entries", entries.len());
            if *dry_run {
                return Ok(());
            }
            let client = Client::new(&cli)?;
            let tmdb = Tmdb::new(cli.tmdb_key.as_ref()).ok();
            let mut failed = 0;
            for e in entries {
                let label = if e.title.is_empty() { e.external_id.clone().unwrap_or_default() } else { e.title.clone() };
                match add_one(&client, tmdb.as_ref(), e, !no_cover) {
                    Ok(title) => println!("ok    {title}"),
                    Err(err) => {
                        failed += 1;
                        eprintln!("FAIL  {label}: {err:#}");
                    }
                }
            }
            if failed > 0 {
                bail!("{failed} entries failed");
            }
        }
    }
    Ok(())
}
