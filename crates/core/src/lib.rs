//! Types shared by the Worker and the CLI. Keep this crate free of runtime-specific deps.

use serde::{Deserialize, Serialize};

pub mod igdb;
pub mod openlibrary;
pub mod tmdb;

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
    pub review: Option<String>,
    pub year: Option<i32>,
    pub source: Option<Source>,
    pub external_id: Option<String>,
    /// R2 object key of the WebP cover.
    pub cover_key: Option<String>,
    /// Date logged / watched / read / played, `YYYY-MM-DD`.
    pub consumed_at: Option<String>,
}

/// Payload for creating an entry (admin UI and CLI both send this).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewEntry {
    pub kind: Kind,
    pub title: String,
    pub status: Status,
    pub rating: Option<u8>,
    /// Also accepted as `notes` (the old name) in CSV/JSON input.
    #[serde(default, alias = "notes")]
    pub review: Option<String>,
    pub year: Option<i32>,
    pub source: Option<Source>,
    pub external_id: Option<String>,
    #[serde(default)]
    pub consumed_at: Option<String>,
}

/// A search hit or fetched item, normalized across providers. `cover_url` is a full-size image
/// to convert to WebP; `thumb_url` is a small one for result lists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub id: String,
    pub title: String,
    pub year: Option<i32>,
    pub cover_url: Option<String>,
    pub thumb_url: Option<String>,
    pub overview: Option<String>,
}

/// Payload for replacing an entry's editable fields. The slug never changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditEntry {
    pub title: String,
    pub status: Status,
    pub rating: Option<u8>,
    pub review: Option<String>,
    pub year: Option<i32>,
    pub consumed_at: Option<String>,
}

pub const MAX_REVIEW_CHARS: usize = 20_000;

/// `YYYY-MM-DD` with a plausible month and day.
pub fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let num = |r: std::ops::Range<usize>| s.get(r).and_then(|x| x.parse::<u32>().ok());
    matches!((num(0..4), num(5..7), num(8..10)), (Some(y), Some(m), Some(d)) if y >= 1000 && (1..=12).contains(&m) && (1..=31).contains(&d))
}

fn validate_fields(title: &str, rating: Option<u8>, review: &Option<String>, date: &Option<String>) -> Result<(), String> {
    if slugify(title).is_empty() && title.trim().is_empty() {
        return Err("title is required".into());
    }
    if matches!(rating, Some(r) if !(1..=10).contains(&r)) {
        return Err("rating must be 1-10".into());
    }
    if review.as_deref().is_some_and(|r| r.chars().count() > MAX_REVIEW_CHARS) {
        return Err(format!("review exceeds {MAX_REVIEW_CHARS} characters"));
    }
    if date.as_deref().is_some_and(|d| !valid_date(d)) {
        return Err("consumed_at must be YYYY-MM-DD".into());
    }
    Ok(())
}

impl NewEntry {
    pub fn validate(&self) -> Result<(), String> {
        validate_fields(&self.title, self.rating, &self.review, &self.consumed_at)
    }
}

impl EditEntry {
    pub fn validate(&self) -> Result<(), String> {
        validate_fields(&self.title, self.rating, &self.review, &self.consumed_at)
    }
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
    fn dates_and_validation() {
        assert!(valid_date("2026-11-23"));
        assert!(!valid_date("2026-13-01") && !valid_date("26-11-23") && !valid_date("2026/11/23"));
        let mut e = EditEntry { title: "x".into(), status: Status::Consumed, rating: Some(11), review: None, year: None, consumed_at: None };
        assert!(e.validate().is_err());
        e.rating = Some(10);
        e.consumed_at = Some("2026-02-30x".into());
        assert!(e.validate().is_err());
        e.consumed_at = Some("2026-02-28".into());
        assert!(e.validate().is_ok());
    }

    #[test]
    fn notes_alias_for_review() {
        let e: NewEntry = serde_json::from_str(r#"{"kind":"movie","title":"t","status":"wanted","notes":"hi"}"#).unwrap();
        assert_eq!(e.review.as_deref(), Some("hi"));
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("100% Orange Juice"), "100-orange-juice");
        assert_eq!(slugify("  10 Things I Hate About You! "), "10-things-i-hate-about-you");
    }
}
