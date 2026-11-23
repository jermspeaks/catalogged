//! IGDB (games). Authenticates with a Twitch app token (client credentials). Queries are POSTed
//! as Apicalypse text bodies. No HTTP here, same as `tmdb`.

use crate::Candidate;
use serde::Deserialize;

pub const GAMES_URL: &str = "https://api.igdb.com/v4/games";
const IMAGE: &str = "https://images.igdb.com/igdb/image/upload";
const FIELDS: &str = "fields name,first_release_date,cover.image_id,summary,involved_companies.developer,involved_companies.company.name;";

/// Token request URL (POST, no body). Tokens last ~2 months; fetch once per run.
pub fn token_url(client_id: &str, client_secret: &str) -> String {
    format!(
        "https://id.twitch.tv/oauth2/token?client_id={}&client_secret={}&grant_type=client_credentials",
        urlencoding::encode(client_id),
        urlencoding::encode(client_secret)
    )
}

#[derive(Deserialize)]
pub struct Token {
    pub access_token: String,
}

pub fn parse_token(json: &str) -> Result<String, serde_json::Error> {
    Ok(serde_json::from_str::<Token>(json)?.access_token)
}

pub fn search_body(query: &str) -> String {
    let q = query.replace('\\', "\\\\").replace('"', "\\\"");
    format!("search \"{q}\"; {FIELDS} limit 10;")
}

/// `id` must be digits; anything else is rejected so it can't inject query text.
pub fn details_body(id: &str) -> Option<String> {
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())).then(|| format!("{FIELDS} where id = {id}; limit 1;"))
}

#[derive(Deserialize)]
struct Cover {
    image_id: String,
}

#[derive(Deserialize)]
struct Company {
    name: Option<String>,
}

#[derive(Deserialize)]
struct Involved {
    #[serde(default)]
    developer: bool,
    company: Option<Company>,
}

#[derive(Deserialize)]
struct Game {
    id: i64,
    name: Option<String>,
    first_release_date: Option<i64>,
    cover: Option<Cover>,
    summary: Option<String>,
    #[serde(default)]
    involved_companies: Vec<Involved>,
}

pub fn parse_games(json: &str) -> Result<Vec<Candidate>, serde_json::Error> {
    Ok(serde_json::from_str::<Vec<Game>>(json)?
        .into_iter()
        .map(|g| Candidate {
            creator: {
                let v: Vec<String> = g.involved_companies.iter().filter(|i| i.developer).filter_map(|i| i.company.as_ref()?.name.clone()).take(3).collect();
                (!v.is_empty()).then(|| v.join(", "))
            },
            id: g.id.to_string(),
            title: g.name.unwrap_or_default(),
            year: g.first_release_date.map(year_of),
            cover_url: g.cover.as_ref().map(|c| format!("{IMAGE}/t_cover_big/{}.jpg", c.image_id)),
            thumb_url: g.cover.as_ref().map(|c| format!("{IMAGE}/t_cover_small/{}.jpg", c.image_id)),
            overview: g.summary.filter(|s| !s.is_empty()),
        })
        .collect())
}

/// Calendar year (UTC) of a Unix timestamp, via the days-to-civil algorithm.
fn year_of(secs: i64) -> i32 {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let y = yoe + era * 400 + i64::from(mp >= 10);
    y as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_builds() {
        let g = parse_games(r#"[{"id":113112,"cover":{"id":1,"image_id":"cob9kr"},"first_release_date":1600300800,"name":"Hades","summary":"x"},{"id":2,"name":"Bare"}]"#).unwrap();
        assert_eq!((g[0].id.as_str(), g[0].year), ("113112", Some(2020)));
        assert_eq!(g[0].cover_url.as_deref(), Some("https://images.igdb.com/igdb/image/upload/t_cover_big/cob9kr.jpg"));
        assert!(g[1].cover_url.is_none() && g[1].year.is_none());
        assert_eq!(year_of(799_286_400), 1995);
        assert_eq!(year_of(1_609_459_199), 2020); // 2020-12-31T23:59:59Z
        assert_eq!(year_of(1_609_459_200), 2021);
        assert_eq!(search_body("a\"b"), "search \"a\\\"b\"; fields name,first_release_date,cover.image_id,summary,involved_companies.developer,involved_companies.company.name; limit 10;");
        let h = parse_games(r#"[{"id":1,"name":"G","involved_companies":[{"developer":false,"company":{"name":"Publisher"}},{"developer":true,"company":{"name":"Supergiant Games"}}]}]"#).unwrap();
        assert_eq!(h[0].creator.as_deref(), Some("Supergiant Games")); // developers only, not publishers
        assert!(g[0].creator.is_none());
        assert!(details_body("12; drop").is_none() && details_body("12").is_some());
    }
}
