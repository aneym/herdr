const SCORE_MATCH: i32 = 16;
const BONUS_BOUNDARY: i32 = 8;
const BONUS_CAMEL: i32 = 7;
const BONUS_CONSECUTIVE: i32 = 4;
const PENALTY_GAP_START: i32 = 3;
const PENALTY_GAP_EXTEND: i32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FuzzyMatch {
    pub score: i32,
    pub positions: Vec<usize>,
}

pub(crate) fn fuzzy_match(needle: &str, haystack: &str) -> Option<FuzzyMatch> {
    let needle_chars = needle.chars().collect::<Vec<_>>();
    let haystack_chars = haystack.chars().collect::<Vec<_>>();
    if needle_chars.is_empty() || haystack_chars.is_empty() {
        return None;
    }

    let mut previous = vec![None::<FuzzyMatch>; haystack_chars.len()];
    for (needle_idx, needle_char) in needle_chars.iter().copied().enumerate() {
        let mut current = vec![None; haystack_chars.len()];
        for (haystack_idx, haystack_char) in haystack_chars.iter().copied().enumerate() {
            if !chars_equal_ignore_case(needle_char, haystack_char) {
                continue;
            }

            let char_score = match_score(&haystack_chars, haystack_idx, needle_char);
            if needle_idx == 0 {
                let gap_penalty = if haystack_idx == 0 {
                    0
                } else {
                    PENALTY_GAP_START + PENALTY_GAP_EXTEND * haystack_idx.saturating_sub(1) as i32
                };
                current[haystack_idx] = Some(FuzzyMatch {
                    score: char_score - gap_penalty,
                    positions: vec![haystack_idx],
                });
                continue;
            }

            for (previous_idx, prior_match) in previous[..haystack_idx].iter().enumerate() {
                let Some(prior_match) = prior_match else {
                    continue;
                };
                let gap = haystack_idx - previous_idx - 1;
                let transition = if gap == 0 {
                    BONUS_CONSECUTIVE
                } else {
                    -(PENALTY_GAP_START + PENALTY_GAP_EXTEND * gap.saturating_sub(1) as i32)
                };
                let score = prior_match.score + char_score + transition;
                if current[haystack_idx]
                    .as_ref()
                    .is_none_or(|best| score > best.score)
                {
                    let mut positions = prior_match.positions.clone();
                    positions.push(haystack_idx);
                    current[haystack_idx] = Some(FuzzyMatch { score, positions });
                }
            }
        }
        previous = current;
    }

    previous
        .into_iter()
        .flatten()
        .max_by_key(|matched| matched.score)
}

pub(crate) fn fuzzy_match_words(query: &str, haystack: &str) -> Option<FuzzyMatch> {
    let mut words = query.split_whitespace();
    let first = words.next()?;
    let mut matched = fuzzy_match(first, haystack)?;
    for word in words {
        let word_match = fuzzy_match(word, haystack)?;
        matched.score += word_match.score;
        matched.positions.extend(word_match.positions);
    }
    matched.positions.sort_unstable();
    matched.positions.dedup();
    Some(matched)
}

fn match_score(haystack: &[char], idx: usize, needle_char: char) -> i32 {
    let haystack_char = haystack[idx];
    let mut score = SCORE_MATCH;
    if idx == 0 || is_separator(haystack[idx - 1]) {
        score += BONUS_BOUNDARY;
    }
    if idx > 0 && haystack[idx - 1].is_lowercase() && haystack_char.is_uppercase() {
        score += BONUS_CAMEL;
    }
    if needle_char == haystack_char {
        score += 1;
    }
    score
}

fn chars_equal_ignore_case(left: char, right: char) -> bool {
    left.to_lowercase().eq(right.to_lowercase())
}

fn is_separator(ch: char) -> bool {
    matches!(ch, ' ' | '-' | '_' | '/' | '.' | ':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_bonus_beats_mid_word_scatter() {
        assert!(
            fuzzy_match("fb", "foo bar").unwrap().score
                > fuzzy_match("fb", "fizzbuzz").unwrap().score
        );
    }

    #[test]
    fn consecutive_match_beats_gapped_match() {
        assert!(
            fuzzy_match("abc", "abc").unwrap().score > fuzzy_match("abc", "axbxc").unwrap().score
        );
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert!(fuzzy_match("LOGIN", "login page").is_some());
    }

    #[test]
    fn exact_case_breaks_tie() {
        assert!(
            fuzzy_match("Log", "Login").unwrap().score > fuzzy_match("Log", "login").unwrap().score
        );
    }

    #[test]
    fn words_use_and_semantics() {
        assert!(fuzzy_match_words("login page", "login page redesign").is_some());
        assert!(fuzzy_match_words("login missing", "login page redesign").is_none());
    }

    #[test]
    fn positions_are_original_haystack_char_indices() {
        assert_eq!(
            fuzzy_match("fbr", "foo bar").unwrap().positions,
            vec![0, 4, 6]
        );
    }

    #[test]
    fn empty_needle_does_not_match() {
        assert!(fuzzy_match("", "haystack").is_none());
        assert!(fuzzy_match_words("   ", "haystack").is_none());
    }
}
