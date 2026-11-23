//! TMDB request building and response parsing. No HTTP here: the CLI (blocking reqwest) and the
//! Worker (`worker::Fetch`) each do the I/O so this crate stays runtime-free.

use crate::Kind;
use serde::Deserialize;

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
    Some(format!("{API}/{}/{}?append_to_response=credits", path_kind(kind)?, urlencoding::encode(id)))
}

/// Looks up a TMDB record by IMDb id (`tt1234567`). Returns movie and tv matches.
pub fn find_url(imdb_id: &str) -> String {
    format!("{API}/find/{}?external_source=imdb_id", urlencoding::encode(imdb_id))
}

pub fn is_imdb_id(s: &str) -> bool {
    s.len() > 2 && s.starts_with("tt") && s[2..].bytes().all(|b| b.is_ascii_digit())
}

#[derive(Deserialize)]
struct Found {
    #[serde(default)]
    movie_results: Vec<Raw>,
    #[serde(default)]
    tv_results: Vec<Raw>,
}

/// First match from a `/find` response, with its kind. Movies win if both lists have one.
pub fn parse_find(json: &str) -> Result<Option<(Kind, Candidate)>, serde_json::Error> {
    let f: Found = serde_json::from_str(json)?;
    Ok(f.movie_results
        .into_iter()
        .next()
        .map(|r| (Kind::Movie, r.into()))
        .or_else(|| f.tv_results.into_iter().next().map(|r| (Kind::Show, r.into()))))
}

pub fn poster_url(poster_path: &str) -> String {
    format!("{IMAGE}/{POSTER_SIZE}{poster_path}")
}

fn thumb_url(poster_path: &str) -> String {
    format!("{IMAGE}/w92{poster_path}")
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

pub use crate::Candidate;

#[derive(Deserialize)]
struct Raw {
    id: i64,
    title: Option<String>,
    name: Option<String>,
    release_date: Option<String>,
    first_air_date: Option<String>,
    poster_path: Option<String>,
    overview: Option<String>,
    /// Only on details (`append_to_response=credits`): directors come from the crew list.
    credits: Option<Credits>,
    /// Only on show details.
    #[serde(default)]
    created_by: Vec<Person>,
}

#[derive(Deserialize)]
struct Credits {
    #[serde(default)]
    crew: Vec<Person>,
}

#[derive(Deserialize)]
struct Person {
    name: Option<String>,
    job: Option<String>,
}

fn names(people: impl Iterator<Item = Option<String>>) -> Option<String> {
    let v: Vec<String> = people.flatten().filter(|n| !n.is_empty()).take(3).collect();
    (!v.is_empty()).then(|| v.join(", "))
}

#[derive(Deserialize)]
struct Search {
    results: Vec<Raw>,
}

impl From<Raw> for Candidate {
    fn from(r: Raw) -> Self {
        let date = r.release_date.or(r.first_air_date).unwrap_or_default();
        let directors = r.credits.map(|c| c.crew).unwrap_or_default().into_iter().filter(|p| p.job.as_deref() == Some("Director")).map(|p| p.name);
        let creator = names(directors).or_else(|| names(r.created_by.into_iter().map(|p| p.name)));
        Candidate {
            id: r.id.to_string(),
            title: r.title.or(r.name).unwrap_or_default(),
            year: date.get(..4).and_then(|y| y.parse().ok()),
            cover_url: r.poster_path.as_deref().filter(|p| !p.is_empty()).map(poster_url),
            thumb_url: r.poster_path.as_deref().filter(|p| !p.is_empty()).map(thumb_url),
            overview: r.overview.filter(|o| !o.is_empty()),
            creator,
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
    fn directors_and_show_creators() {
        let m = parse_details(r#"{"id":1,"title":"M","credits":{"crew":[{"name":"Editor Person","job":"Editor"},{"name":"Lana Wachowski","job":"Director"},{"name":"Lilly Wachowski","job":"Director"}]}}"#).unwrap();
        assert_eq!(m.creator.as_deref(), Some("Lana Wachowski, Lilly Wachowski"));
        let t = parse_details(r#"{"id":2,"name":"S","created_by":[{"name":"Vince Gilligan"}]}"#).unwrap();
        assert_eq!(t.creator.as_deref(), Some("Vince Gilligan"));
        assert!(parse_details(r#"{"id":3,"title":"Search hit"}"#).unwrap().creator.is_none());
        assert!(details_url(Kind::Movie, "603").unwrap().ends_with("append_to_response=credits"));
    }

    #[test]
    fn parses_movie_and_show() {
        let m = parse_details(r#"{"id":4951,"title":"10 Things I Hate About You","release_date":"1999-03-31","poster_path":"/a.jpg","overview":""}"#).unwrap();
        assert_eq!((m.id.as_str(), m.year, m.overview), ("4951", Some(1999), None));
        assert_eq!(m.cover_url.as_deref(), Some("https://image.tmdb.org/t/p/w500/a.jpg"));
        let s = parse_search(r#"{"results":[{"id":1,"name":"Show","first_air_date":"","poster_path":null}]}"#).unwrap();
        assert_eq!((s[0].title.as_str(), s[0].year, s[0].cover_url.clone()), ("Show", None, None));
    }

    #[test]
    fn find_by_imdb_id() {
        assert!(is_imdb_id("tt0133093") && !is_imdb_id("603") && !is_imdb_id("tt"));
        let (k, c) = parse_find(r#"{"movie_results":[],"tv_results":[{"id":1396,"name":"Breaking Bad","first_air_date":"2008-01-20"}]}"#).unwrap().unwrap();
        assert_eq!((k, c.id.as_str(), c.year), (Kind::Show, "1396", Some(2008)));
        assert!(parse_find(r#"{"movie_results":[],"tv_results":[]}"#).unwrap().is_none());
    }

    #[test]
    fn urls_and_auth() {
        assert_eq!(search_url(Kind::Show, "a b").unwrap(), format!("{API}/search/tv?query=a%20b&include_adult=false"));
        assert!(details_url(Kind::Game, "1").is_none());
        assert_eq!(authenticate("u?x=1".into(), "eyJabc").1.as_deref(), Some("Bearer eyJabc"));
        assert_eq!(authenticate("u".into(), "k1").0, "u?api_key=k1");
    }
}
