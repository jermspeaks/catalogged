//! Open Library (books). No key needed. Entries use the *work* id (e.g. `OL20893680W`), which is
//! stable across editions. No HTTP here, same as `tmdb`.

use crate::Candidate;
use serde::Deserialize;

const API: &str = "https://openlibrary.org";
const COVERS: &str = "https://covers.openlibrary.org/b/id";

pub fn search_url(query: &str) -> String {
    format!(
        "{API}/search.json?q={}&limit=10&fields=key,title,first_publish_year,cover_i,author_name",
        urlencoding::encode(query)
    )
}

pub fn details_url(work_id: &str) -> String {
    format!("{API}/works/{}.json", urlencoding::encode(work_id))
}

/// `-L` is the large size; `-S` the small one for result lists.
fn cover(id: i64, size: char) -> String {
    format!("{COVERS}/{id}-{size}.jpg")
}

#[derive(Deserialize)]
struct SearchDoc {
    key: String,
    title: Option<String>,
    first_publish_year: Option<i32>,
    cover_i: Option<i64>,
    #[serde(default)]
    author_name: Vec<String>,
}

#[derive(Deserialize)]
struct Search {
    #[serde(default)]
    docs: Vec<SearchDoc>,
}

pub fn parse_search(json: &str) -> Result<Vec<Candidate>, serde_json::Error> {
    Ok(serde_json::from_str::<Search>(json)?
        .docs
        .into_iter()
        .map(|d| Candidate {
            id: d.key.rsplit('/').next().unwrap_or_default().to_owned(),
            title: d.title.unwrap_or_default(),
            year: d.first_publish_year,
            cover_url: d.cover_i.filter(|c| *c > 0).map(|c| cover(c, 'L')),
            thumb_url: d.cover_i.filter(|c| *c > 0).map(|c| cover(c, 'S')),
            overview: (!d.author_name.is_empty()).then(|| d.author_name.join(", ")),
        })
        .collect())
}

#[derive(Deserialize)]
struct Work {
    key: String,
    title: Option<String>,
    #[serde(default)]
    covers: Vec<i64>,
    first_publish_date: Option<String>,
}

/// A work record has no author names (only author keys) and a free-text publish date.
pub fn parse_details(json: &str) -> Result<Candidate, serde_json::Error> {
    let w: Work = serde_json::from_str(json)?;
    let cover_id = w.covers.into_iter().find(|c| *c > 0);
    Ok(Candidate {
        id: w.key.rsplit('/').next().unwrap_or_default().to_owned(),
        title: w.title.unwrap_or_default(),
        year: w.first_publish_date.as_deref().and_then(year_in),
        cover_url: cover_id.map(|c| cover(c, 'L')),
        thumb_url: cover_id.map(|c| cover(c, 'S')),
        overview: None,
    })
}

/// First run of four digits in a date like "August 28, 2020" or "1999".
fn year_in(s: &str) -> Option<i32> {
    let b = s.as_bytes();
    (0..b.len().saturating_sub(3)).find_map(|i| {
        let w = &s[i..i + 4];
        (w.bytes().all(|c| c.is_ascii_digit()) && b.get(i + 4).is_none_or(|c| !c.is_ascii_digit())).then(|| w.parse().ok())?
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_and_details() {
        let hits = parse_search(r#"{"docs":[{"author_name":["Susanna Clarke"],"cover_i":10226290,"first_publish_year":2020,"key":"/works/OL20893680W","title":"Piranesi"},{"key":"/works/OL1W","title":"No cover"}]}"#).unwrap();
        assert_eq!((hits[0].id.as_str(), hits[0].year, hits[0].overview.as_deref()), ("OL20893680W", Some(2020), Some("Susanna Clarke")));
        assert_eq!(hits[0].cover_url.as_deref(), Some("https://covers.openlibrary.org/b/id/10226290-L.jpg"));
        assert!(hits[1].cover_url.is_none());
        let d = parse_details(r#"{"key":"/works/OL1W","title":"T","covers":[-1,55],"first_publish_date":"August 28, 2020"}"#).unwrap();
        assert_eq!((d.id.as_str(), d.year, d.cover_url.as_deref()), ("OL1W", Some(2020), Some("https://covers.openlibrary.org/b/id/55-L.jpg")));
        assert_eq!(year_in("1999"), Some(1999));
        assert_eq!(year_in("no year"), None);
    }
}
