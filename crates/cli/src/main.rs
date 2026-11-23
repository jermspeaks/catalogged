use anyhow::{bail, Context, Result};
use catalogged_core::{igdb, matching, openlibrary, tmdb, Candidate, EditEntry, Entry, Kind, NewEntry, Source, Status, MAX_COVER_BYTES};
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
    /// IGDB (Twitch app) client id, for games
    #[arg(long, env = "IGDB_CLIENT_ID", global = true, hide_env_values = true)]
    igdb_id: Option<String>,
    /// IGDB (Twitch app) client secret, for games
    #[arg(long, env = "IGDB_CLIENT_SECRET", global = true, hide_env_values = true)]
    igdb_secret: Option<String>,
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
        /// Optional when --external-id is given (fetched from the provider)
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
        /// Author, director or developer (filled from the provider when --external-id is given)
        #[arg(long)]
        creator: Option<String>,
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
        /// Provider id (TMDB id, Open Library work id or IGDB id; the provider follows from the kind). Empty string clears it
        #[arg(long)]
        external_id: Option<String>,
        /// Author, director or developer. Empty string clears it
        #[arg(long)]
        creator: Option<String>,
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
    /// Search the provider for this kind (TMDB, Open Library or IGDB) and print their ids
    Search {
        /// game | book | show | movie
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
    /// Find and upload covers for entries that have none (for example title-only imports): reads the
    /// public /index.json, searches the provider for each cover-less entry by title and year, and
    /// uploads the cover of an exact-title match and stores the provider id (and year, if missing) on the entry. Re-runnable: entries that already have a cover are skipped.
    Covers {
        /// Only this kind: game | book | show | movie
        #[arg(long)]
        kind: Option<String>,
        /// Print what would be uploaded; send nothing to the Worker
        #[arg(long)]
        dry_run: bool,
        /// Also accept the provider's first exact-title result when several records share the title (check)
        #[arg(long)]
        accept_check: bool,
        /// Stop after this many uploads (a first try before the full run)
        #[arg(long)]
        limit: Option<usize>,
        /// Don't touch covers: store the provider id on entries that have none, including ones that already have a cover
        #[arg(long)]
        ids_only: bool,
        /// Don't search: for entries that already have a provider id but no creator, fetch the creator (author, director, developer) from the provider
        #[arg(long, conflicts_with = "ids_only")]
        creators: bool,
    },
    /// Find provider ids for rows that have none: searches TMDB / Open Library / IGDB by title (and
    /// author for books) and writes a new CSV with source, external_id and year filled in, plus
    /// match (high | check | none), match_title and candidates columns to review before `import`.
    /// Rows that already have an external_id pass through unchanged. Nothing is sent to the Worker.
    Match {
        input: PathBuf,
        output: PathBuf,
        /// Ask about every row that is not a sure match: pick a candidate by number, accept the suggestion with Enter, skip, or quit (rows done so far are kept)
        #[arg(long, short)]
        interactive: bool,
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
    /// Author, director or developer; `author` is accepted as an alias.
    #[serde(default, alias = "author")]
    creator: Option<String>,
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

/// Talks to the metadata providers directly (keys stay on this machine) and builds WebP covers.
/// Movies/shows: TMDB. Books: Open Library (no key). Games: IGDB (Twitch app token, fetched once).
struct Meta {
    http: reqwest::blocking::Client,
    tmdb_key: Option<String>,
    igdb: Option<(String, String)>,
    igdb_token: std::cell::OnceCell<String>,
}

/// The provider an entry of this kind uses by default.
fn default_source(kind: Kind) -> Source {
    match kind {
        Kind::Movie | Kind::Show => Source::Tmdb,
        Kind::Book => Source::OpenLibrary,
        Kind::Game => Source::Igdb,
    }
}

impl Meta {
    fn new(cli: &Cli) -> Result<Self> {
        // Providers can be slow; don't hang forever. Open Library asks for an identifying User-Agent.
        let http = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("catalogged-personal-cli/0.1")
            .build()?;
        let igdb = cli.igdb_id.clone().zip(cli.igdb_secret.clone());
        Ok(Self { http, tmdb_key: cli.tmdb_key.clone(), igdb, igdb_token: Default::default() })
    }

    fn tmdb_get(&self, url: String) -> Result<String> {
        let key = self.tmdb_key.as_ref().context("set TMDB_API_KEY (or --tmdb-key)")?;
        let (url, auth) = tmdb::authenticate(url, key);
        let mut req = self.http.get(url);
        if let Some(a) = auth {
            req = req.header("Authorization", a);
        }
        let res = req.send()?;
        if !res.status().is_success() {
            bail!("TMDB returned {}", res.status());
        }
        Ok(res.text()?)
    }

    fn get_text(&self, url: String, what: &str) -> Result<String> {
        let res = self.http.get(url).send()?;
        if !res.status().is_success() {
            bail!("{what} returned {}", res.status());
        }
        Ok(res.text()?)
    }

    fn igdb_post(&self, body: String) -> Result<String> {
        let (id, secret) = self.igdb.as_ref().context("set IGDB_CLIENT_ID and IGDB_CLIENT_SECRET")?;
        if self.igdb_token.get().is_none() {
            let res = self.http.post(igdb::token_url(id, secret)).send()?;
            if !res.status().is_success() {
                bail!("Twitch token request returned {} (check the IGDB client id and secret)", res.status());
            }
            let _ = self.igdb_token.set(igdb::parse_token(&res.text()?)?);
        }
        let res = self
            .http
            .post(igdb::GAMES_URL)
            .header("Client-ID", id)
            .bearer_auth(self.igdb_token.get().map(String::as_str).unwrap_or_default())
            .header("Accept", "application/json")
            .body(body)
            .send()?;
        if !res.status().is_success() {
            bail!("IGDB returned {}", res.status());
        }
        Ok(res.text()?)
    }

    fn search(&self, kind: Kind, q: &str) -> Result<Vec<Candidate>> {
        Ok(match kind {
            Kind::Movie | Kind::Show => {
                tmdb::parse_search(&self.tmdb_get(tmdb::search_url(kind, q).context("unsupported kind")?)?)?
            }
            Kind::Book => openlibrary::parse_search(&self.get_text(openlibrary::search_url(q), "Open Library")?)?,
            Kind::Game => igdb::parse_games(&self.igdb_post(igdb::search_body(q))?)?,
        })
    }

    /// IMDb id -> (kind, TMDB record).
    fn find(&self, imdb_id: &str) -> Result<(Kind, Candidate)> {
        tmdb::parse_find(&self.tmdb_get(tmdb::find_url(imdb_id))?)?
            .with_context(|| format!("TMDB has no movie or show for {imdb_id}"))
    }

    fn details(&self, kind: Kind, id: &str) -> Result<Candidate> {
        match kind {
            Kind::Movie | Kind::Show => {
                Ok(tmdb::parse_details(&self.tmdb_get(tmdb::details_url(kind, id).context("unsupported kind")?)?)?)
            }
            Kind::Book => Ok(openlibrary::parse_details(&self.get_text(openlibrary::details_url(id), "Open Library")?)?),
            Kind::Game => {
                let body = igdb::details_body(id).context("IGDB ids are numbers")?;
                igdb::parse_games(&self.igdb_post(body)?)?
                    .into_iter()
                    .next()
                    .with_context(|| format!("IGDB has no game {id}"))
            }
        }
    }

    /// Downloads the cover and re-encodes it as a WebP under the free-tier size cap.
    fn cover_webp(&self, url: &str) -> Result<Vec<u8>> {
        let bytes = self.http.get(url).send()?.error_for_status()?.bytes()?;
        let img = image::load_from_memory(&bytes).context("decoding cover")?;
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

/// Fill in blanks from the provider and return the cover URL (if any) for the cover step.
fn enrich(meta: &Meta, e: &mut NewEntry) -> Result<Option<String>> {
    let (Some(source), Some(id)) = (e.source, e.external_id.clone()) else {
        if e.title.trim().is_empty() {
            bail!("title is required unless an --external-id is given");
        }
        return Ok(None);
    };
    if source != default_source(e.kind) {
        bail!("source {source:?} cannot supply a {}; use {:?}", e.kind.as_str(), default_source(e.kind));
    }
    let found = meta.details(e.kind, &id)?;
    if e.title.trim().is_empty() {
        e.title = found.title;
    }
    e.year = e.year.or(found.year);
    e.creator = e.creator.take().or(found.creator);
    Ok(found.cover_url)
}

/// Turn a sheet row into a full entry, resolving IMDb ids through TMDB.
fn resolve_row(meta: &Meta, row: Row) -> Result<NewEntry> {
    let mut kind = row.kind;
    let mut source = row.source;
    let mut external_id = row.external_id.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    let mut title = row.title;
    let mut year = row.year;
    if let Some(id) = external_id.clone() {
        if tmdb::is_imdb_id(&id) {
            let (found_kind, c) = meta.find(&id)?;
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
            source = kind.map(default_source);
        }
    }
    let kind = kind.context("kind is required unless external_id is an IMDb id (tt...)")?;
    if let (true, Some(_), Some(id)) = (title.trim().is_empty(), source, &external_id) {
        let c = meta.details(kind, id)?;
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
        creator: row.creator.map(|c| c.trim().to_owned()).filter(|c| !c.is_empty()),
        consumed_at: row.consumed_at.filter(|d| !d.is_empty()),
    })
}

/// Create the entry, then (best effort) upload its cover. A cover failure never loses the entry.
fn add_one(client: &Client, meta: &Meta, mut e: NewEntry, covers: bool) -> Result<String> {
    let cover = enrich(meta, &mut e)?;
    let slug = client.create(&e)?;
    if let (true, Some(url)) = (covers, cover) {
        if let Err(err) = meta.cover_webp(&url).and_then(|w| client.put_cover(e.kind, &slug, w)) {
            eprintln!("warning: {} was added but its cover failed: {err:#}", e.title);
        }
    }
    Ok(e.title)
}

/// You can't have consumed something before it came out: drop records dated after the year it was
/// logged (a 2010 entry is not the 2025 remake). Allows one year of slack for regional release dates.
fn released_by(found: Vec<Candidate>, logged: Option<&str>) -> Vec<Candidate> {
    match logged.and_then(|d| d.get(..4)).and_then(|y| y.parse::<i32>().ok()) {
        Some(ly) => found.into_iter().filter(|c| c.year.is_none_or(|cy| cy <= ly + 1)).collect(),
        None => found,
    }
}

/// What the owner decided for one uncertain row in `match --interactive`.
enum Choice {
    Pick(usize),
    Skip,
    Quit,
}

fn describe(c: &Candidate) -> String {
    format!("{}{}{}", c.title, c.year.map(|y| format!(" ({y})")).unwrap_or_default(), c.creator.as_ref().map(|a| format!("  by {a}")).unwrap_or_default())
}

/// Show the candidates for a row and read the answer from stdin.
fn ask(title: &str, year: Option<i32>, creator: &str, found: &[Candidate], suggested: Option<usize>, conf: matching::Confidence) -> Result<Choice> {
    use std::io::Write;
    println!("\n? {title}{}{}   [{}]", year.map(|y| format!(" ({y})")).unwrap_or_default(), if creator.is_empty() { String::new() } else { format!("  by {creator}") }, conf.as_str());
    if found.is_empty() {
        println!("  (no results)");
        return Ok(Choice::Skip);
    }
    for (i, c) in found.iter().take(6).enumerate() {
        println!("  {}) {}{}", i + 1, describe(c), if suggested == Some(i) { "   <- suggested" } else { "" });
    }
    loop {
        print!("  number, Enter = {}, s = skip, q = quit: ", if suggested.is_some() { "accept suggestion" } else { "skip" });
        std::io::stdout().flush()?;
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line)? == 0 {
            return Ok(Choice::Quit);
        }
        match line.trim() {
            "" => return Ok(suggested.map_or(Choice::Skip, Choice::Pick)),
            "s" => return Ok(Choice::Skip),
            "q" => return Ok(Choice::Quit),
            n => match n.parse::<usize>() {
                Ok(i) if (1..=found.len().min(6)).contains(&i) => return Ok(Choice::Pick(i - 1)),
                _ => println!("  not a choice"),
            },
        }
    }
}

/// Search each id-less row and write the review CSV (see `Cmd::Match`).
fn match_csv(meta: &Meta, input: &PathBuf, output: &PathBuf, interactive: bool) -> Result<()> {
    let mut rdr = csv::Reader::from_path(input)?;
    let mut headers: Vec<String> = rdr.headers()?.iter().map(str::to_owned).collect();
    // `author` is the old name of `creator`; keep one column so `import` does not see both.
    if let Some(i) = headers.iter().position(|h| h == "author") {
        if !headers.iter().any(|h| h == "creator") {
            headers[i] = "creator".into();
        }
    }
    let mut col = |name: &str| -> usize {
        headers.iter().position(|h| h == name).unwrap_or_else(|| {
            headers.push(name.to_owned());
            headers.len() - 1
        })
    };
    let (i_kind, i_title, i_year) = (col("kind"), col("title"), col("year"));
    let (i_src, i_id, i_creator) = (col("source"), col("external_id"), col("creator"));
    let (i_match, i_mtitle, i_cands) = (col("match"), col("match_title"), col("candidates"));
    let i_logged = headers.iter().position(|h| h == "consumed_at");
    let mut wtr = csv::Writer::from_path(output)?;
    wtr.write_record(&headers)?;
    let (mut high, mut check, mut none, mut manual, mut errors, mut skipped) = (0, 0, 0, 0, 0, 0);
    let mut quit = false;
    for (n, rec) in rdr.records().enumerate() {
        let mut f: Vec<String> = rec?.iter().map(str::to_owned).collect();
        f.resize(headers.len(), String::new());
        let kind = parse::<Kind>(&f[i_kind]).ok();
        let Some(kind) = kind.filter(|_| !quit && f[i_id].trim().is_empty() && !f[i_title].trim().is_empty()) else {
            skipped += 1;
            wtr.write_record(&f)?;
            continue;
        };
        let year = f[i_year].trim().parse::<i32>().ok();
        let creator_in = f[i_creator].trim().to_owned();
        let query = if kind == Kind::Book && !creator_in.is_empty() { format!("{} {}", f[i_title], creator_in) } else { f[i_title].clone() };
        match meta.search(kind, &query).or_else(|_| meta.search(kind, &f[i_title])) {
            Ok(found) => {
                let found = released_by(found, i_logged.map(|i| f[i].as_str()));
                let (suggested, conf) = matching::pick(&f[i_title], year, (!creator_in.is_empty()).then_some(creator_in.as_str()), &found);
                let (mut chosen, mut label) = (suggested, conf.as_str());
                if interactive && conf != matching::Confidence::High {
                    match ask(&f[i_title], year, &creator_in, &found, suggested, conf)? {
                        Choice::Pick(i) => {
                            chosen = Some(i);
                            label = "manual";
                        }
                        Choice::Skip => {
                            chosen = None;
                            label = "skipped";
                        }
                        Choice::Quit => {
                            quit = true;
                            wtr.write_record(&f)?;
                            continue;
                        }
                    }
                }
                f[i_match] = label.to_owned();
                if let Some(c) = chosen.map(|i| &found[i]) {
                    // TMDB search results carry no director: one details call for the picked record.
                    let c = if c.creator.is_none() { meta.details(kind, &c.id).map(|d| Candidate { creator: d.creator, ..c.clone() }).unwrap_or_else(|_| c.clone()) } else { c.clone() };
                    f[i_src] = default_source(kind).as_str().to_owned();
                    f[i_id] = c.id.clone();
                    f[i_mtitle] = format!("{}{}", c.title, c.year.map(|y| format!(" ({y})")).unwrap_or_default());
                    if f[i_year].trim().is_empty() {
                        f[i_year] = c.year.map(|y| y.to_string()).unwrap_or_default();
                    }
                    if creator_in.is_empty() {
                        f[i_creator] = c.creator.clone().unwrap_or_default();
                    }
                }
                f[i_cands] = found.iter().take(3).map(|c| format!("{}: {}", c.id, describe(c))).collect::<Vec<_>>().join(" | ");
                match (label, conf) {
                    ("manual", _) => manual += 1,
                    (_, matching::Confidence::High) => high += 1,
                    (_, matching::Confidence::Check) if label != "skipped" => check += 1,
                    _ => none += 1,
                }
            }
            Err(err) => {
                eprintln!("FAIL  row {}: {err:#}", n + 2);
                f[i_match] = "error".into();
                errors += 1;
            }
        }
        wtr.write_record(&f)?;
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    wtr.flush()?;
    println!("matched: {high} high, {check} to check, {manual} picked by hand, {none} none, {errors} errors, {skipped} left as they were -> {}", output.display());
    Ok(())
}

/// Fills in the creator of entries that have a provider id but no creator, from the provider's details.
fn fill_creators(client: &Client, meta: &Meta, only: Option<Kind>, dry_run: bool, limit: Option<usize>) -> Result<()> {
    let index: serde_json::Value = client.http.get(format!("{}/index.json", client.url)).send()?.error_for_status()?.json()?;
    let items = index["items"].as_array().context("unexpected /index.json shape")?;
    let (mut done, mut none, mut failed) = (0, 0, vec![]);
    for it in items {
        let (Some(kind), Some(slug)) = (it[0].as_str().and_then(|k| parse::<Kind>(k).ok()), it[1].as_str()) else { continue };
        if it[9].as_str().is_some_and(|c| !c.is_empty()) || only.is_some_and(|k| k != kind) {
            continue;
        }
        if limit.is_some_and(|l| done >= l) {
            break;
        }
        let cur = match client.get_entry(kind, slug) {
            Ok(e) => e,
            Err(e) => {
                failed.push(format!("{slug}: {e:#}"));
                continue;
            }
        };
        let (Some(source), Some(id)) = (cur.source, cur.external_id.clone()) else { continue };
        if source != default_source(kind) || cur.creator.is_some() {
            continue;
        }
        let creator = match meta.details(kind, &id) {
            Ok(d) => d.creator,
            Err(e) => {
                failed.push(format!("{}: {e:#}", cur.title));
                continue;
            }
        };
        let Some(creator) = creator else {
            none += 1;
            continue;
        };
        if dry_run {
            println!("would set creator  {} -> {creator}", cur.title);
        } else {
            let edit = EditEntry {
                title: cur.title.clone(),
                status: cur.status,
                rating: cur.rating,
                review: cur.review,
                year: cur.year,
                consumed_at: cur.consumed_at,
                source: cur.source,
                external_id: cur.external_id,
                creator: Some(creator.clone()),
            };
            match client.edit_entry(kind, slug, &edit) {
                Ok(()) => println!("creator  {} -> {creator}", cur.title),
                Err(e) => {
                    failed.push(format!("{}: {e:#}", cur.title));
                    continue;
                }
            }
        }
        done += 1;
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    println!("{} {done} creators; {none} had none on the provider, {} failed", if dry_run { "would set" } else { "set" }, failed.len());
    if !failed.is_empty() {
        bail!("{} failed:\n  {}", failed.len(), failed.join("\n  "));
    }
    Ok(())
}

/// Stores the provider id on an existing entry (unless it already has one).
fn set_provider_id(client: &Client, kind: Kind, slug: &str, c: &Candidate) -> Result<()> {
    let cur = client.get_entry(kind, slug)?;
    if cur.external_id.is_some() {
        return Ok(());
    }
    let edit = EditEntry {
        title: cur.title,
        status: cur.status,
        rating: cur.rating,
        review: cur.review,
        year: cur.year.or(c.year),
        consumed_at: cur.consumed_at,
        source: Some(default_source(kind)),
        external_id: Some(c.id.clone()),
        creator: cur.creator.or_else(|| c.creator.clone()),
    };
    client.edit_entry(kind, slug, &edit)
}

/// Cover-less entries from the public index, searched on the provider and filled in (see `Cmd::Covers`).
fn find_covers(client: &Client, meta: &Meta, only: Option<Kind>, dry_run: bool, accept_check: bool, limit: Option<usize>, ids_only: bool) -> Result<()> {
    // Rows are [kind, slug, title, status, rating, year, cover_key, consumed_at, updated_at].
    let index: serde_json::Value = client.http.get(format!("{}/index.json", client.url)).send()?.error_for_status()?.json()?;
    let items = index["items"].as_array().context("unexpected /index.json shape")?;
    let (mut done, mut unmatched, mut failed) = (0, vec![], vec![]);
    let mut seen = 0;
    for it in items {
        let (Some(kind), Some(slug), Some(title)) = (it[0].as_str().and_then(|k| parse::<Kind>(k).ok()), it[1].as_str(), it[2].as_str()) else {
            continue;
        };
        let has_cover = it[6].as_str().is_some_and(|k| !k.is_empty());
        if (has_cover && !ids_only) || only.is_some_and(|k| k != kind) {
            continue;
        }
        if ids_only && client.get_entry(kind, slug).is_ok_and(|e| e.external_id.is_some()) {
            continue;
        }
        if limit.is_some_and(|l| done >= l) {
            break;
        }
        seen += 1;
        let year = it[5].as_i64().map(|y| y as i32);
        let found = match meta.search(kind, title) {
            Ok(f) => f,
            Err(e) => {
                failed.push(format!("{title}: search failed: {e:#}"));
                continue;
            }
        };
        // You can't have consumed something before it came out: drop later-dated records (a 2010 log
        // entry is not the 2025 remake). Allows one year of slack for regional release dates.
        let found = released_by(found, it[7].as_str());
        let creator = it[9].as_str().filter(|c| !c.is_empty());
        let (pick, conf) = matching::pick(title, year, creator, &found);
        let take = match conf {
            matching::Confidence::High => true,
            matching::Confidence::Check => accept_check,
            matching::Confidence::None => false,
        };
        let Some(c) = pick.map(|i| &found[i]).filter(|c| take && (ids_only || c.cover_url.is_some())) else {
            let first = found.first().map(|c| format!(" (first result: {}{})", c.title, c.year.map(|y| format!(" {y}")).unwrap_or_default())).unwrap_or_default();
            unmatched.push(format!("{} [{}]{first}", title, conf.as_str()));
            std::thread::sleep(std::time::Duration::from_millis(250));
            continue;
        };
        let label = format!("{title} -> {}{}", c.title, c.year.map(|y| format!(" ({y})")).unwrap_or_default());
        // TMDB search results carry no director: look it up once the match is accepted.
        let enriched;
        let c = if !dry_run && c.creator.is_none() {
            enriched = meta.details(kind, &c.id).map(|d| Candidate { creator: d.creator, ..c.clone() }).unwrap_or_else(|_| c.clone());
            &enriched
        } else {
            c
        };
        if dry_run {
            println!("would set {}  {label}  [{}]", if ids_only { "id" } else { "cover" }, conf.as_str());
        } else if ids_only {
            match set_provider_id(client, kind, slug, c) {
                Ok(()) => println!("id  {label}"),
                Err(e) => {
                    failed.push(format!("{title}: {e:#}"));
                    continue;
                }
            }
        } else {
            match meta.cover_webp(c.cover_url.as_deref().unwrap_or_default()).and_then(|w| client.put_cover(kind, slug, w)) {
                Ok(()) => {
                    println!("cover  {label}");
                    // Remember which provider record this is (needs the entry's other fields, as PUT replaces them).
                    if let Err(e) = set_provider_id(client, kind, slug, c) {
                        failed.push(format!("{title}: cover set, but saving the id failed: {e:#}"));
                    }
                }
                Err(e) => {
                    failed.push(format!("{title}: {e:#}"));
                    continue;
                }
            }
        }
        done += 1;
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    println!("{} {done} of {seen} cover-less entries; {} not matched, {} failed", if dry_run { "would set" } else { "set" }, unmatched.len(), failed.len());
    for u in &unmatched {
        println!("  no cover: {u}");
    }
    if !failed.is_empty() {
        bail!("{} failed:\n  {}", failed.len(), failed.join("\n  "));
    }
    Ok(())
}

fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();
    match &cli.cmd {
        Cmd::Add { kind, title, status, rating, year, review, date, source, external_id, creator, no_cover } => {
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
                creator: creator.clone(),
                consumed_at: date.clone().or_else(|| today_if_consumed(status)),
            };
            let mut entry = entry;
            if entry.source.is_none() && entry.external_id.is_some() {
                entry.source = Some(default_source(entry.kind));
            }
            let title = add_one(&Client::new(&cli)?, &Meta::new(&cli)?, entry, !no_cover)?;
            println!("added {title}");
        }
        Cmd::Edit { kind, slug, title, status, rating, review, date, year, external_id, creator } => {
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
                source: match blank(external_id) {
                    Some(None) => None,
                    Some(Some(_)) => Some(default_source(kind)),
                    None => cur.source,
                },
                external_id: blank(external_id).unwrap_or(cur.external_id),
                creator: blank(creator).unwrap_or(cur.creator),
            };
            edit.validate_for(kind).map_err(anyhow::Error::msg)?;
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
            for c in Meta::new(&cli)?.search(kind, query)?.into_iter().take(10) {
                println!("{:>8}  {}{}{}", c.id, c.title, c.year.map(|y| format!(" ({y})")).unwrap_or_default(), c.creator.map(|a| format!("  by {a}")).unwrap_or_default());
            }
        }
        Cmd::Covers { kind, dry_run, accept_check, limit, ids_only, creators } => {
            let kind = kind.as_deref().map(parse::<Kind>).transpose()?;
            if *creators {
                fill_creators(&Client::new(&cli)?, &Meta::new(&cli)?, kind, *dry_run, *limit)?
            } else {
                find_covers(&Client::new(&cli)?, &Meta::new(&cli)?, kind, *dry_run, *accept_check, *limit, *ids_only)?
            }
        }
        Cmd::Match { input, output, interactive } => match_csv(&Meta::new(&cli)?, input, output, *interactive)?,
        Cmd::Import { file, dry_run, skip_existing, no_cover } => {
            let mut rdr = csv::Reader::from_path(file)?;
            let rows = rdr
                .deserialize::<Row>()
                .enumerate()
                .map(|(i, r)| r.with_context(|| format!("row {}", i + 2)))
                .collect::<Result<Vec<_>>>()?;
            println!("read {} rows", rows.len());
            let meta = Meta::new(&cli)?;
            let client = if *dry_run { None } else { Some(Client::new(&cli)?) };
            let (mut added, mut skipped) = (0, 0);
            let mut failures: Vec<String> = vec![];
            for (i, row) in rows.into_iter().enumerate() {
                let line = i + 2;
                let entry = match resolve_row(&meta, row).and_then(|e| e.validate().map_err(anyhow::Error::msg).map(|_| e)) {
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
                match add_one(client, &meta, entry, !no_cover) {
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
