//! Types shared by the Worker and the CLI. Keep this crate free of runtime-specific deps.

use serde::{Deserialize, Serialize};

/// Max size of an uploaded WebP cover. Keeps R2 well inside the free tier (see docs/free-tier.md).
pub const MAX_COVER_BYTES: usize = 300 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Game,
    Book,
    Show,
    Movie,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Game => "game",
            Kind::Book => "book",
            Kind::Show => "show",
            Kind::Movie => "movie",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Wanted,
    InProgress,
    Consumed,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Wanted => "wanted",
            Status::InProgress => "in_progress",
            Status::Consumed => "consumed",
        }
    }
}

/// Where an entry's metadata came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Tmdb,
    Igdb,
    OpenLibrary,
}

/// An entry as stored in D1 and returned by the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: i64,
    pub kind: Kind,
    pub slug: String,
    pub title: String,
    pub status: Status,
    pub rating: Option<u8>,
    pub notes: Option<String>,
    pub year: Option<i32>,
    pub source: Option<Source>,
    pub external_id: Option<String>,
    /// R2 object key of the WebP cover.
    pub cover_key: Option<String>,
    pub consumed_at: Option<String>,
}

/// Payload for creating an entry (admin UI and CLI both send this).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewEntry {
    pub kind: Kind,
    pub title: String,
    pub status: Status,
    pub rating: Option<u8>,
    pub notes: Option<String>,
    pub year: Option<i32>,
    pub source: Option<Source>,
    pub external_id: Option<String>,
}

/// Lowercase, hyphen-separated, ASCII-only slug.
pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slugify("100% Orange Juice"), "100-orange-juice");
        assert_eq!(slugify("  10 Things I Hate About You! "), "10-things-i-hate-about-you");
    }
}
