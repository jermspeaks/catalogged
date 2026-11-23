//! TMDB request building and response parsing. No HTTP here: the CLI (blocking reqwest) and the
//! Worker (`worker::Fetch`) each do the I/O so this crate stays runtime-free.

use crate::Kind;
use serde::{Deserialize, Serialize};

const API: &str = "https://api.themoviedb.org/3";
const IMAGE: &str = "https://image.tmdb.org/t/p";

/// Poster width requested from TMDB. w500 is ~500x750, small enough that a WebP stays tiny.
pub const POSTER_SIZE: &str = "w500";

fn path_kind(kind: Kind) -> Option<&'static str> {
    match kind {
        Kind::Movie => Some("movie"),
        Kind::Show => Some("tv"),
        _ => None,
    }
}

pub fn search_url(kind: Kind, query: &str) -> Option<String> {
    Some(format!(
        "{API}/search/{}?query={}&include_adult=false",
        path_kind(kind)?,
        urlencoding::encode(query)
    ))
}

pub fn details_url(kind: Kind, id: &str) -> Option<String> {
    Some(format!("{API}/{}/{}", path_kind(kind)?, urlencoding::encode(id)))
}

pub fn poster_url(poster_path: &str) -> String {
    format!("{IMAGE}/{POSTER_SIZE}{poster_path}")
}

/// TMDB accepts a v4 read-access token (JWT, `Authorization: Bearer`) or a v3 key (`api_key=`).
/// Returns the final URL and the Authorization header value, if any.
pub fn authenticate(url: String, key: &str) -> (String, Option<String>) {
    if key.starts_with("eyJ") {
        (url, Some(format!("Bearer {key}")))
    } else {
        let sep = if url.contains('?') { '&' } else { '?' };
        (format!("{url}{sep}api_key={}", urlencoding::encode(key)), None)
    }
}

/// A search hit or fetched item, normalized across movies and shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub id: String,
    pub title: String,
    pub year: Option<i32>,
    pub poster_path: Option<String>,
    pub overview: Option<String>,
}

#[derive(Deserialize)]
struct Raw {
    id: i64,
    title: Option<String>,
    name: Option<String>,
    release_date: Option<String>,
    first_air_date: Option<String>,
    poster_path: Option<String>,
    overview: Option<String>,
}

#[derive(Deserialize)]
struct Search {
    results: Vec<Raw>,
}

impl From<Raw> for Candidate {
    fn from(r: Raw) -> Self {
        let date = r.release_date.or(r.first_air_date).unwrap_or_default();
        Candidate {
            id: r.id.to_string(),
            title: r.title.or(r.name).unwrap_or_default(),
            year: date.get(..4).and_then(|y| y.parse().ok()),
            poster_path: r.poster_path.filter(|p| !p.is_empty()),
            overview: r.overview.filter(|o| !o.is_empty()),
        }
    }
}

pub fn parse_search(json: &str) -> Result<Vec<Candidate>, serde_json::Error> {
    Ok(serde_json::from_str::<Search>(json)?.results.into_iter().map(Into::into).collect())
}

pub fn parse_details(json: &str) -> Result<Candidate, serde_json::Error> {
    Ok(serde_json::from_str::<Raw>(json)?.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_movie_and_show() {
        let m = parse_details(r#"{"id":4951,"title":"10 Things I Hate About You","release_date":"1999-03-31","poster_path":"/a.jpg","overview":""}"#).unwrap();
        assert_eq!((m.id.as_str(), m.year, m.overview), ("4951", Some(1999), None));
        let s = parse_search(r#"{"results":[{"id":1,"name":"Show","first_air_date":"","poster_path":null}]}"#).unwrap();
        assert_eq!((s[0].title.as_str(), s[0].year, s[0].poster_path.clone()), ("Show", None, None));
    }

    #[test]
    fn urls_and_auth() {
        assert_eq!(search_url(Kind::Show, "a b").unwrap(), format!("{API}/search/tv?query=a%20b&include_adult=false"));
        assert!(details_url(Kind::Game, "1").is_none());
        assert_eq!(authenticate("u?x=1".into(), "eyJabc").1.as_deref(), Some("Bearer eyJabc"));
        assert_eq!(authenticate("u".into(), "k1").0, "u?api_key=k1");
    }
}
