//! Case-insensitive filename matching and monotonic same-letter jump selection.

use std::ffi::OsStr;

/// Priority tier for a filename match within a folder listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MatchTier {
    /// Query matches at character index `0` of the filename.
    Prefix,
    /// Query matches after character index `0` of the filename.
    Substring,
}

/// Winning candidate chosen by type-to-jump evaluation for one pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JumpPick {
    /// Index into the selectable-candidates slice supplied to selection.
    pub candidate_index: usize,
    /// Winning priority tier (`Prefix` takes priority over `Substring`).
    pub tier: MatchTier,
    /// True when a repeated single-letter query collapsed to its single character to cycle.
    pub effective_query_collapsed: bool,
}

/// Evaluates `name` against `query` case-insensitively without per-entry heap allocation
/// on valid UTF-8 filenames.
pub fn match_name(name: &OsStr, query: &str) -> Option<MatchTier> {
    if query.is_empty() {
        return None;
    }
    let lossy = name.to_string_lossy();
    let hay_str = lossy.as_ref();
    if hay_str.is_ascii() && query.is_ascii() {
        return match_ascii(hay_str.as_bytes(), query.as_bytes());
    }
    if folded_starts_with(hay_str, query) {
        return Some(MatchTier::Prefix);
    }
    for (byte_index, _) in hay_str.char_indices().skip(1) {
        if folded_starts_with(&hay_str[byte_index..], query) {
            return Some(MatchTier::Substring);
        }
    }
    None
}

fn match_ascii(hay: &[u8], needle: &[u8]) -> Option<MatchTier> {
    if needle.len() > hay.len() {
        return None;
    }
    if hay[..needle.len()].eq_ignore_ascii_case(needle) {
        return Some(MatchTier::Prefix);
    }
    if hay[1..]
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
    {
        return Some(MatchTier::Substring);
    }
    None
}

fn folded_starts_with(haystack: &str, query: &str) -> bool {
    let mut needle = query.chars().flat_map(char::to_lowercase);
    let mut current = needle.next();
    for folded in haystack.chars().flat_map(char::to_lowercase) {
        let Some(expected) = current else {
            return true;
        };
        if folded != expected {
            return false;
        }
        current = needle.next();
    }
    current.is_none()
}

fn chars_fold_equal(left: char, right: char) -> bool {
    if left.is_ascii() && right.is_ascii() {
        left.eq_ignore_ascii_case(&right)
    } else {
        left.to_lowercase().eq(right.to_lowercase())
    }
}

/// Returns the first character slice when `query` has two or more characters and
/// every character folds to the same lowercase sequence.
pub fn collapsed_repeat_unit(query: &str) -> Option<&str> {
    let mut chars = query.chars();
    let first = chars.next()?;
    let second = chars.next()?;
    if !chars_fold_equal(first, second) {
        return None;
    }
    if chars.all(|ch| chars_fold_equal(first, ch)) {
        Some(&query[..first.len_utf8()])
    } else {
        None
    }
}

/// Returns true when at least one filename in `names` starts with `query` case-insensitively.
pub fn has_prefix_match<'a, I>(names: I, query: &str) -> bool
where
    I: IntoIterator<Item = &'a OsStr>,
{
    if query.is_empty() {
        return false;
    }
    names
        .into_iter()
        .any(|name| match_name(name, query) == Some(MatchTier::Prefix))
}

/// Counts every candidate filename in `names` that matches `query` at either
/// `MatchTier::Prefix` or `MatchTier::Substring`.
pub fn count_matches<'a, I>(names: I, query: &str) -> usize
where
    I: IntoIterator<Item = &'a OsStr>,
{
    if query.is_empty() {
        return 0;
    }
    names
        .into_iter()
        .filter(|name| match_name(name, query).is_some())
        .count()
}

/// Picks the first `Prefix` match in display order, falling back to the first `Substring` match.
pub fn pick_best_match<'a, I>(names: I, query: &str) -> Option<(usize, MatchTier)>
where
    I: IntoIterator<Item = &'a OsStr>,
{
    if query.is_empty() {
        return None;
    }
    let mut first_substring = None;
    for (index, name) in names.into_iter().enumerate() {
        match match_name(name, query) {
            Some(MatchTier::Prefix) => return Some((index, MatchTier::Prefix)),
            Some(MatchTier::Substring) if first_substring.is_none() => {
                first_substring = Some(index);
            }
            Some(MatchTier::Substring) | None => {}
        }
    }
    first_substring.map(|index| (index, MatchTier::Substring))
}

/// Cycles to the next match strictly after `after_candidate` within the highest matching tier,
/// wrapping to the first match in that tier at the end of the listing.
pub fn pick_cycled_match<'a, I>(
    names: I,
    unit_query: &str,
    after_candidate: Option<usize>,
) -> Option<(usize, MatchTier)>
where
    I: IntoIterator<Item = &'a OsStr>,
{
    if unit_query.is_empty() {
        return None;
    }
    let mut first_prefix = None;
    let mut next_prefix = None;
    let mut first_substring = None;
    let mut next_substring = None;
    for (index, name) in names.into_iter().enumerate() {
        match match_name(name, unit_query) {
            Some(MatchTier::Prefix) => {
                if first_prefix.is_none() {
                    first_prefix = Some(index);
                }
                if after_candidate.is_some_and(|cur| index > cur) && next_prefix.is_none() {
                    next_prefix = Some(index);
                    break;
                }
            }
            Some(MatchTier::Substring) if first_prefix.is_none() => {
                if first_substring.is_none() {
                    first_substring = Some(index);
                }
                if after_candidate.is_some_and(|cur| index > cur) && next_substring.is_none() {
                    next_substring = Some(index);
                }
            }
            Some(MatchTier::Substring) | None => {}
        }
    }
    if let Some(index) = next_prefix.or(first_prefix) {
        Some((index, MatchTier::Prefix))
    } else {
        next_substring
            .or(first_substring)
            .map(|index| (index, MatchTier::Substring))
    }
}

/// Selects the winning candidate for `query`, applying Option B (`Prefix` then `Substring`)
/// and monotonic same-letter cycling when `force_cycle` is set or a repeated single character
/// has zero literal `Prefix` matches.
pub fn select_jump_match<'a, I>(
    names: I,
    query: &str,
    force_cycle: bool,
    current_candidate: Option<usize>,
) -> Option<JumpPick>
where
    I: IntoIterator<Item = &'a OsStr> + Clone,
{
    if query.is_empty() {
        return None;
    }
    if force_cycle {
        let unit = collapsed_repeat_unit(query).unwrap_or(query);
        let (candidate_index, tier) = pick_cycled_match(names, unit, current_candidate)?;
        return Some(JumpPick {
            candidate_index,
            tier,
            effective_query_collapsed: true,
        });
    }
    if let Some(unit) = collapsed_repeat_unit(query) {
        if let Some(candidate_index) = names
            .clone()
            .into_iter()
            .position(|name| match_name(name, query) == Some(MatchTier::Prefix))
        {
            return Some(JumpPick {
                candidate_index,
                tier: MatchTier::Prefix,
                effective_query_collapsed: false,
            });
        }
        let (candidate_index, tier) = pick_cycled_match(names, unit, current_candidate)?;
        return Some(JumpPick {
            candidate_index,
            tier,
            effective_query_collapsed: true,
        });
    }
    let (candidate_index, tier) = pick_best_match(names, query)?;
    Some(JumpPick {
        candidate_index,
        tier,
        effective_query_collapsed: false,
    })
}
