//! Picking the provider record that matches a title (and optional year) from a search result list.
//! Pure logic so it can be tested; the CLI does the searching.

use crate::Candidate;

/// Lower-case alphanumerics only, so "Spider-Man: Homecoming" and "spider man homecoming" compare equal.
pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Confidence {
    /// Title matches exactly and the year agrees (or the match is the only exact title).
    High,
    /// Title matches exactly but several records share it and the year can't separate them.
    Check,
    /// No exact title match; candidates are only suggestions.
    None,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Confidence::High => "high",
            Confidence::Check => "check",
            Confidence::None => "none",
        }
    }
}

/// Returns the index of the chosen candidate (if any) and how sure we are.
/// Candidates are assumed to be in provider relevance order.
pub fn pick(title: &str, year: Option<i32>, found: &[Candidate]) -> (Option<usize>, Confidence) {
    let want = norm(title);
    let exact: Vec<usize> = found.iter().enumerate().filter(|(_, c)| norm(&c.title) == want).map(|(i, _)| i).collect();
    if exact.is_empty() {
        return (None, Confidence::None);
    }
    if let Some(y) = year {
        // Exact year first, then within a year (release dates differ by region).
        for tol in [0, 1] {
            if let Some(&i) = exact.iter().find(|&&i| found[i].year.is_some_and(|cy| (cy - y).abs() <= tol)) {
                return (Some(i), Confidence::High);
            }
        }
        return (Some(exact[0]), Confidence::Check);
    }
    if exact.len() == 1 {
        (Some(exact[0]), Confidence::High)
    } else {
        (Some(exact[0]), Confidence::Check)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, title: &str, year: Option<i32>) -> Candidate {
        Candidate { id: id.into(), title: title.into(), year, cover_url: None, thumb_url: None, overview: None }
    }

    #[test]
    fn normalises_punctuation_and_case() {
        assert_eq!(norm("Spider-Man: Homecoming"), norm("spider man homecoming"));
    }

    #[test]
    fn unique_exact_title_is_high() {
        let f = [c("1", "Inception", Some(2010)), c("2", "Inception: The Cobol Job", Some(2010))];
        assert_eq!(pick("inception", None, &f), (Some(0), Confidence::High));
    }

    #[test]
    fn year_separates_remakes() {
        let f = [c("1", "Total Recall", Some(2012)), c("2", "Total Recall", Some(1990))];
        assert_eq!(pick("Total Recall", Some(1990), &f), (Some(1), Confidence::High));
        assert_eq!(pick("Total Recall", None, &f), (Some(0), Confidence::Check));
    }

    #[test]
    fn year_off_by_one_is_accepted_but_far_is_check() {
        let f = [c("1", "Up", Some(2009))];
        assert_eq!(pick("Up", Some(2010), &f), (Some(0), Confidence::High));
        assert_eq!(pick("Up", Some(1990), &f), (Some(0), Confidence::Check));
    }

    #[test]
    fn misspelt_title_has_no_match() {
        let f = [c("1", "Memento", Some(2000))];
        assert_eq!(pick("Momento", None, &f), (None, Confidence::None));
    }
}
