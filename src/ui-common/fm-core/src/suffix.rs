//! One rule for "which registered extension does this file name end with".
//!
//! A plugin claims `.tar.gz` and another claims `.gz`; a name that ends with
//! both belongs to the longer claim. The rule was written five different ways
//! across the application — `Path::extension` alone answers `gz` for
//! `holiday.tar.gz` — so everything that dispatches by extension asks here.

/// The list a plugin registers with, as the host reads it: comma separated,
/// lower case, each one starting with a dot and naming something after it,
/// each one only once.
///
/// Sorted longest first, so whoever walks it asks about `.tar.gz` before
/// `.gz` and the first answer is the right one. Equal lengths keep the order
/// the plugin wrote them in.
pub fn claimed(extensions: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for claim in extensions.split(',') {
        let lowered = claim.trim().to_lowercase();
        if !lowered.starts_with('.') || !lowered.chars().any(|c| c != '.') {
            continue;
        }
        if !found.contains(&lowered) {
            found.push(lowered);
        }
    }
    found.sort_by(|left, right| right.len().cmp(&left.len()));
    found
}

/// Which claim the name ends with, the longest winning.
///
/// `claimed` hands the list over longest first, so the answer is usually the
/// first thing asked about. A list from somewhere else is still answered
/// correctly: a shorter claim never displaces a longer one already found, and
/// between equal claims the earlier one keeps the file.
pub fn longest_match<'a>(
    name: &str,
    claims: impl IntoIterator<Item = &'a String>,
) -> Option<&'a String> {
    let lowered = name.to_lowercase();
    let mut best: Option<&String> = None;
    for claim in claims {
        if !lowered.ends_with(claim.as_str()) {
            continue;
        }
        if best.is_none_or(|held| claim.len() > held.len()) {
            best = Some(claim);
        }
    }
    best
}

/// Whether the name ends with any of them at all.
pub fn matches_any<'a>(name: &str, claims: impl IntoIterator<Item = &'a String>) -> bool {
    longest_match(name, claims).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(of: &str) -> Vec<String> {
        claimed(of)
    }

    #[test]
    fn the_longer_claim_wins() {
        let held = claims(".gz,.tar.gz");
        assert_eq!(
            longest_match("holiday.tar.gz", &held).map(String::as_str),
            Some(".tar.gz")
        );
        assert_eq!(
            longest_match("holiday.gz", &held).map(String::as_str),
            Some(".gz")
        );
    }

    /// Two plugins claiming the same suffix must not swap places between one
    /// question and the next: whoever registered first keeps the file.
    #[test]
    fn a_tie_goes_to_the_first_one_registered() {
        let held = vec![".log".to_string(), ".log".to_string()];
        assert!(std::ptr::eq(
            longest_match("today.log", &held).expect("a claim"),
            &held[0]
        ));
    }

    /// A list nobody sorted is still answered correctly — the matcher does not
    /// rely on the order it was handed.
    #[test]
    fn an_unsorted_list_still_answers_the_longest() {
        let held = vec![".gz".to_string(), ".tar.gz".to_string()];
        assert_eq!(
            longest_match("holiday.tar.gz", &held).map(String::as_str),
            Some(".tar.gz")
        );
    }

    /// The order matters as much as the answer: whoever walks the list asks
    /// about the longest claim first and can stop at the first hit.
    #[test]
    fn the_list_comes_back_longest_first() {
        assert_eq!(
            claimed(".gz,.tar.gz,.tbz2,.zip"),
            vec![
                ".tar.gz".to_string(),
                ".tbz2".to_string(),
                ".zip".to_string(),
                ".gz".to_string()
            ]
        );
    }

    #[test]
    fn the_same_claim_twice_is_one_claim() {
        assert_eq!(claimed(".zip, .ZIP ,.zip"), vec![".zip".to_string()]);
    }

    #[test]
    fn the_name_is_read_whatever_its_case() {
        let held = claims(".ZIP");
        assert_eq!(held, vec![".zip".to_string()], "claims are lowered");
        assert!(matches_any("Holiday.Zip", &held));
    }

    /// A claim with no dot, or nothing but dots, is not a claim: it would
    /// match names nobody meant it to.
    #[test]
    fn nonsense_claims_are_dropped() {
        assert_eq!(claimed("zip, ., .., .7z ,"), vec![".7z".to_string()]);
    }

    #[test]
    fn nothing_claimed_means_nothing_matches() {
        assert!(longest_match("holiday.tar.gz", &Vec::new()).is_none());
        assert!(!matches_any("holiday.tar.gz", &claims(".zip")));
    }
}
