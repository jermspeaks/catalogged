//! Picking the provider record that matches a title (and optional year and creator) from a search
//! result list. Pure logic so it can be tested; the CLI does the searching.
//!
//! Matches come in tiers. Only an exact title (after normalising case, punctuation, "&", a leading
//! article and "Part N" / roman numerals) can be `High`, and only when the year agrees (or the title is
//! unique). A match on the part before a subtitle, or within a typo or two, is always `Check`: a human
//! confirms it.

use crate::Candidate;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Confidence {
    /// Exact title and the year agrees (or the exact title is unique) and the creator does not disagree.
    High,
    /// A plausible match that needs a human: several records share the title, the year or creator
    /// disagrees, or the title only matches up to a subtitle or a typo.
    Check,
    /// Nothing close.
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

/// Lower-case alphanumerics only. Kept for callers that want a quick equality key.
pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

fn roman(t: &str) -> Option<&'static str> {
    Some(match t {
        "ii" => "2",
        "iii" => "3",
        "iv" => "4",
        "v" => "5",
        "vi" => "6",
        "vii" => "7",
        "viii" => "8",
        "ix" => "9",
        "x" => "10",
        _ => return None,
    })
}

/// Comparable tokens: lower case, "&" is "and", apostrophes vanish, other punctuation separates,
/// a leading article is dropped, "Part N" becomes "N" (and a "Part 1 / I / One" suffix disappears),
/// and a trailing roman numeral becomes a digit.
pub fn canon(s: &str) -> Vec<String> {
    let mut text = String::new();
    for c in s.chars() {
        match c {
            '&' => text.push_str(" and "),
            '\'' | '\u{2019}' | '`' => {}
            '\u{bd}' => text.push_str(" 1 2 "), // ½
            c if c.is_alphanumeric() => text.extend(c.to_lowercase()),
            _ => text.push(' '),
        }
    }
    let mut t: Vec<String> = text.split_whitespace().map(str::to_owned).collect();
    if t.len() > 1 && ["the", "a", "an"].contains(&t[0].as_str()) {
        t.remove(0);
    }
    let had_part = t.len() > 1 && t.iter().any(|x| x == "part");
    t.retain(|x| x != "part");
    if had_part && t.len() > 1 && ["1", "i", "one"].contains(&t[t.len() - 1].as_str()) {
        t.pop();
    }
    if t.len() > 1 {
        if let Some(d) = t.last().and_then(|l| roman(l)) {
            *t.last_mut().unwrap() = d.to_owned();
        }
    }
    // "Seven Years in Tibet" and "7 Years in Tibet" are the same title.
    for x in t.iter_mut() {
        if let Some(d) = number_word(x) {
            *x = d.to_owned();
        }
    }
    t
}

fn number_word(t: &str) -> Option<&'static str> {
    Some(match t {
        "one" => "1",
        "two" => "2",
        "three" => "3",
        "four" => "4",
        "five" => "5",
        "six" => "6",
        "seven" => "7",
        "eight" => "8",
        "nine" => "9",
        "ten" => "10",
        _ => return None,
    })
}

/// The title before a subtitle ("Rework: Change the Way..." -> "Rework").
fn primary(s: &str) -> &str {
    let cut = [": ", ". ", " - ", " \u{2013} ", " \u{2014} "].iter().filter_map(|d| s.find(d)).min();
    // "St. Elmo's Fire" must not shrink to "St": a very short head is not a title.
    cut.filter(|&i| s[..i].chars().filter(|c| c.is_alphanumeric()).count() >= 4).map_or(s, |i| &s[..i])
}

/// Other ways to search for the same title, best first: the title as written, without a
/// parenthetical ("Das Wunder von Bern (The Miracle of Bern)" -> "Das Wunder von Bern"), the
/// parenthetical alone (usually the English title; skipped for books, where it is often the author),
/// and the part before a subtitle (long subtitles make provider searches come back empty).
pub fn variants(title: &str, allow_inner: bool) -> Vec<String> {
    let title = title.trim();
    let mut v = vec![title.to_owned()];
    if let (Some(open), Some(close)) = (title.find('('), title.rfind(')')) {
        if open < close {
            let outer = format!("{} {}", &title[..open], &title[close + 1..]);
            v.push(outer.split_whitespace().collect::<Vec<_>>().join(" "));
            let inner = title[open + 1..close].trim();
            if allow_inner && inner.chars().filter(|c| c.is_alphabetic()).count() >= 4 {
                v.push(inner.to_owned());
            }
        }
    }
    let p = primary(title).trim();
    if p != title {
        v.push(p.to_owned());
    }
    let mut seen = std::collections::HashSet::new();
    v.retain(|x| !x.is_empty() && seen.insert(x.to_lowercase()));
    v
}

fn lev(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push((prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// 0 = exact, 1 = equal up to a subtitle, 2 = within a typo; None = no match. Second value: edit distance.
fn tier(want: &str, cand: &str) -> Option<(u8, usize)> {
    let (w, c) = (canon(want), canon(cand));
    if w.is_empty() || c.is_empty() {
        return None;
    }
    if w == c {
        return Some((0, 0));
    }
    let (wp, cp) = (canon(primary(want)), canon(primary(cand)));
    if (wp == c || w == cp || wp == cp) && !wp.is_empty() && !cp.is_empty() {
        return Some((1, 0));
    }
    let (wj, cj): (Vec<char>, Vec<char>) = (w.join(" ").chars().collect(), c.join(" ").chars().collect());
    let d = lev(&wj, &cj);
    // Very short titles must match exactly: one typo in "Up" is a different word.
    (wj.len() >= 5 && d <= (wj.len() / 8).max(1)).then_some((2, d))
}

fn name_tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphabetic()).filter(|t| t.chars().count() >= 3).map(str::to_lowercase).collect()
}

/// Whether two creator lists share a name token (surname, usually). Unknown on either side is not a disagreement.
fn creator_agrees(want: Option<&str>, cand: Option<&str>) -> Option<bool> {
    let (w, c) = (name_tokens(want?), name_tokens(cand?));
    (!w.is_empty() && !c.is_empty()).then(|| w.iter().any(|t| c.contains(t)))
}

/// Returns the index of the chosen candidate (if any) and how sure we are.
/// Candidates are assumed to be in provider relevance order.
pub fn pick(title: &str, year: Option<i32>, creator: Option<&str>, found: &[Candidate]) -> (Option<usize>, Confidence) {
    let scored: Vec<(usize, u8, usize)> = found.iter().enumerate().filter_map(|(i, c)| tier(title, &c.title).map(|(t, d)| (i, t, d))).collect();
    let Some(best) = scored.iter().map(|s| s.1).min() else { return (None, Confidence::None) };
    let mut group: Vec<(usize, usize)> = scored.iter().filter(|s| s.1 == best).map(|s| (s.0, s.2)).collect();
    group.sort_by_key(|&(i, d)| (d, i)); // closest spelling first, then provider order
    // Prefer records whose creator agrees (books: same author), then keep provider order.
    group.sort_by_key(|&(i, _)| creator_agrees(creator, found[i].creator.as_deref()) == Some(false));
    let idx: Vec<usize> = group.into_iter().map(|g| g.0).collect();
    let first = idx[0];
    let disagrees = creator_agrees(creator, found[first].creator.as_deref()) == Some(false);
    if best > 0 {
        return (Some(first), Confidence::Check);
    }
    if let Some(y) = year {
        // Exact year first, then within a year (release dates differ by region).
        for tol in [0, 1] {
            if let Some(&i) = idx.iter().find(|&&i| found[i].year.is_some_and(|cy| (cy - y).abs() <= tol)) {
                let bad = creator_agrees(creator, found[i].creator.as_deref()) == Some(false);
                return (Some(i), if bad { Confidence::Check } else { Confidence::High });
            }
        }
        return (Some(first), Confidence::Check);
    }
    // No year to compare: a unique exact title is only a sure match when the record itself has a year
    // (an undated record is usually an obscure entry that happens to share a common title).
    let unique = idx.len() == 1 && found[first].year.is_some();
    (Some(first), if unique && !disagrees { Confidence::High } else { Confidence::Check })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, title: &str, year: Option<i32>) -> Candidate {
        Candidate { id: id.into(), title: title.into(), year, cover_url: None, thumb_url: None, overview: None, creator: None }
    }
    fn by(mut c: Candidate, who: &str) -> Candidate {
        c.creator = Some(who.into());
        c
    }

    #[test]
    fn canon_normalises() {
        assert_eq!(canon("Spider-Man: Homecoming"), canon("spider man homecoming"));
        assert_eq!(canon("The Boondock Saints"), canon("Boondock Saints"));
        assert_eq!(canon("Bill & Ted's Excellent Adventure"), canon("Bill and Teds Excellent Adventure"));
        assert_eq!(canon("Red Cliff: Part 1"), canon("Red Cliff"));
        assert_eq!(canon("Red Cliff: Part 2"), canon("Red Cliff II"));
        assert_eq!(canon("The Godfather: Part I"), canon("The Godfather"));
        assert_ne!(canon("The Godfather: Part II"), canon("The Godfather"));
        assert_eq!(canon("Naked Gun 2 1/2"), canon("The Naked Gun 2\u{bd}"));
    }

    #[test]
    fn number_words_and_short_heads() {
        assert_eq!(canon("7 Years in Tibet"), canon("Seven Years in Tibet"));
        let f = [c("1", "Omnivore's Dilemma. A Natural History of Four Meals", Some(2006))];
        assert_eq!(pick("The Omnivore's Dilemma", None, None, &f), (Some(0), Confidence::Check));
        assert_eq!(pick("St. Elmo's Fire", None, None, &[c("1", "St. Louis Blues", Some(1939))]), (None, Confidence::None));
    }

    #[test]
    fn search_variants() {
        assert_eq!(variants("Das Wunder von Bern (The Miracle of Bern)", true), ["Das Wunder von Bern (The Miracle of Bern)", "Das Wunder von Bern", "The Miracle of Bern"]);
        assert_eq!(variants("Republic (Plato)", false), ["Republic (Plato)", "Republic"]);
        assert_eq!(variants("Vagabonding: An Uncommon Guide", false), ["Vagabonding: An Uncommon Guide", "Vagabonding"]);
        assert_eq!(variants("Inception", true), ["Inception"]);
    }

    #[test]
    fn unique_exact_title_is_high() {
        let f = [c("1", "Inception", Some(2010)), c("2", "Inception: The Cobol Job", Some(2010))];
        assert_eq!(pick("inception", None, None, &f), (Some(0), Confidence::High));
    }

    #[test]
    fn article_and_ampersand_do_not_block_a_high_match() {
        let f = [c("1", "The Boondock Saints", Some(1999)), c("2", "The Boondock Saints II: All Saints Day", Some(2009))];
        assert_eq!(pick("Boondock Saints", None, None, &f), (Some(0), Confidence::High));
        let g = [c("1", "Bill & Ted's Excellent Adventure", Some(1989))];
        assert_eq!(pick("Bill and Ted's Excellent Adventure", None, None, &g), (Some(0), Confidence::High));
    }

    #[test]
    fn part_numbers_and_roman_numerals() {
        let f = [c("1", "Red Cliff", Some(2008)), c("2", "Red Cliff II", Some(2009))];
        assert_eq!(pick("Red Cliff: Part 1", None, None, &f), (Some(0), Confidence::High));
        assert_eq!(pick("Red Cliff: Part 2", None, None, &f), (Some(1), Confidence::High));
    }

    #[test]
    fn year_separates_remakes() {
        let f = [c("1", "Total Recall", Some(2012)), c("2", "Total Recall", Some(1990))];
        assert_eq!(pick("Total Recall", Some(1990), None, &f), (Some(1), Confidence::High));
        assert_eq!(pick("Total Recall", None, None, &f), (Some(0), Confidence::Check));
    }

    #[test]
    fn year_off_by_one_is_accepted_but_far_is_check() {
        let f = [c("1", "Up", Some(2009))];
        assert_eq!(pick("Up", Some(2010), None, &f), (Some(0), Confidence::High));
        assert_eq!(pick("Up", Some(1990), None, &f), (Some(0), Confidence::Check));
    }

    #[test]
    fn typos_and_subtitles_are_check_never_high() {
        let f = [c("1", "Memento", Some(2000))];
        assert_eq!(pick("Momento", None, None, &f), (Some(0), Confidence::Check));
        let g = [c("1", "Bonk", Some(2008))];
        assert_eq!(pick("Bonk: The Curious Coupling of Science and Sex", None, None, &g), (Some(0), Confidence::Check));
        let h = [c("1", "Alien: Covenant", Some(2017))];
        assert_eq!(pick("Alien", None, None, &h), (Some(0), Confidence::Check));
    }

    #[test]
    fn unrelated_and_very_short_titles_do_not_match() {
        assert_eq!(pick("Julie and Julia", None, None, &[c("1", "Romeo + Juliet", Some(1996))]), (None, Confidence::None));
        assert_eq!(pick("Up", None, None, &[c("1", "Us", Some(2019))]), (None, Confidence::None));
        assert_eq!(pick("Rocky", None, None, &[c("1", "Rockwell", Some(2001))]), (None, Confidence::None));
    }

    #[test]
    fn undated_unique_record_is_only_check() {
        assert_eq!(pick("Momento", None, None, &[c("1", "Momento", None)]), (Some(0), Confidence::Check));
        assert_eq!(pick("Momento", None, None, &[c("1", "Momento", Some(2022))]), (Some(0), Confidence::High));
    }

    #[test]
    fn creator_picks_between_same_titled_books_and_demotes_a_mismatch() {
        let f = [by(c("1", "Rework", Some(2010)), "Someone Else"), by(c("2", "Rework", Some(2010)), "Jason Fried, David Heinemeier Hansson")];
        assert_eq!(pick("Rework", Some(2010), Some("Jason Fried; David Heinemeier Hansson"), &f), (Some(1), Confidence::High));
        let only = [by(c("1", "Naked", Some(1997)), "Somebody Different")];
        assert_eq!(pick("Naked", None, Some("David Sedaris"), &only), (Some(0), Confidence::Check));
        // unknown creator on either side is not a disagreement
        assert_eq!(pick("Naked", None, None, &only), (Some(0), Confidence::High));
    }
}
