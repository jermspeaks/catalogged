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

/// One work by id. The work record itself has only author *keys*, so this uses a key search, which
/// returns author names, the first publish year and the cover id in one request.
pub fn details_url(work_id: &str) -> String {
    format!(
        "{API}/search.json?q=key:/works/{}&limit=1&fields=key,title,first_publish_year,cover_i,author_name",
        urlencoding::encode(work_id)
    )
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
            overview: None,
            creator: (!d.author_name.is_empty()).then(|| d.author_name.iter().take(3).cloned().collect::<Vec<_>>().join(", ")),
        })
        .collect())
}

/// The record for a work id (see `details_url`); errors if the search found nothing.
pub fn parse_details(json: &str) -> Result<Candidate, serde_json::Error> {
    parse_search(json)?
        .into_iter()
        .next()
        .ok_or_else(|| <serde_json::Error as serde::de::Error>::custom("Open Library has no work with that id"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_and_details() {
        let hits = parse_search(r#"{"docs":[{"author_name":["Susanna Clarke"],"cover_i":10226290,"first_publish_year":2020,"key":"/works/OL20893680W","title":"Piranesi"},{"key":"/works/OL1W","title":"No cover"}]}"#).unwrap();
        assert_eq!((hits[0].id.as_str(), hits[0].year, hits[0].creator.as_deref()), ("OL20893680W", Some(2020), Some("Susanna Clarke")));
        assert_eq!(hits[0].cover_url.as_deref(), Some("https://covers.openlibrary.org/b/id/10226290-L.jpg"));
        assert!(hits[1].cover_url.is_none());
        // details come from a key search: author, year and cover in one record
        let d = parse_details(r#"{"docs":[{"author_name":["A","B","C","D"],"cover_i":55,"first_publish_year":2020,"key":"/works/OL1W","title":"T"}]}"#).unwrap();
        assert_eq!((d.id.as_str(), d.year, d.creator.as_deref()), ("OL1W", Some(2020), Some("A, B, C")));
        assert!(parse_details(r#"{"docs":[]}"#).is_err());
        assert!(details_url("OL1W").contains("q=key:/works/OL1W"));
    }
}
