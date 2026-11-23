use anyhow::{bail, Context, Result};
use catalogged_core::{Kind, NewEntry, Source, Status};
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
        #[arg(long)]
        title: String,
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
    },
    /// Bulk-add entries from a CSV with columns:
    /// kind,title,status,rating,notes,year,source,external_id
    Import {
        file: PathBuf,
        /// Parse and validate only; send nothing
        #[arg(long)]
        dry_run: bool,
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

    fn create(&self, e: &NewEntry) -> Result<()> {
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
        Ok(())
    }
}

fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();
    match &cli.cmd {
        Cmd::Add { kind, title, status, rating, year, notes, source, external_id } => {
            let entry = NewEntry {
                kind: parse::<Kind>(kind)?,
                title: title.clone(),
                status: parse::<Status>(status)?,
                rating: *rating,
                notes: notes.clone(),
                year: *year,
                source: source.as_deref().map(parse::<Source>).transpose()?,
                external_id: external_id.clone(),
            };
            Client::new(&cli)?.create(&entry)?;
            println!("added {}", entry.title);
        }
        Cmd::Import { file, dry_run } => {
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
            let mut failed = 0;
            for e in &entries {
                match client.create(e) {
                    Ok(()) => println!("ok    {}", e.title),
                    Err(err) => {
                        failed += 1;
                        eprintln!("FAIL  {}: {err}", e.title);
                    }
                }
            }
            if failed > 0 {
                bail!("{failed} of {} entries failed", entries.len());
            }
        }
    }
    Ok(())
}
