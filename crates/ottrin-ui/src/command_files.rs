//! Matching for the command palette's file section.
//!
//! This module only inspects the entries it is given; it never touches the
//! filesystem.

use ottrin_core::FileEntry;

/// The command palette only displays this many file results.
const MAX_RESULTS: usize = 100;

/// Return the entries whose name contains `input`, best matches first.
///
/// The query is trimmed and compared case-insensitively as a whole. Prefix
/// matches rank above matches starting a word (words are separated by
/// non-alphanumeric characters), which in turn rank above other substring
/// matches. Order within a rank is the incoming order. An empty query matches
/// nothing.
pub(crate) fn matches(input: &str, entries: &[FileEntry]) -> Vec<FileEntry> {
    let query = input.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let needle = query.to_lowercase();
    let mut prefixes = Vec::new();
    let mut word_starts = Vec::new();
    let mut substrings = Vec::new();

    for (index, entry) in entries.iter().enumerate() {
        let name = entry.name.to_lowercase();
        let Some(position) = name.find(&needle) else {
            continue;
        };
        if position == 0 {
            prefixes.push(index);
        } else if name
            .match_indices(&needle)
            .any(|(position, _)| starts_word(&name, position))
        {
            word_starts.push(index);
        } else {
            substrings.push(index);
        }
    }

    prefixes
        .into_iter()
        .chain(word_starts)
        .chain(substrings)
        .take(MAX_RESULTS)
        .map(|index| entries[index].clone())
        .collect()
}

/// Whether the match at `position` begins a word. The caller has already
/// handled position zero.
fn starts_word(name: &str, position: usize) -> bool {
    name[..position]
        .chars()
        .next_back()
        .is_some_and(|previous| !previous.is_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ottrin_core::EntryKind;
    use std::path::PathBuf;

    fn entry(name: &str) -> FileEntry {
        FileEntry {
            name: name.to_owned(),
            path: PathBuf::from(name),
            kind: EntryKind::File,
            is_executable: false,
            symlink_target_is_dir: None,
            size_bytes: None,
            modified_unix_secs: None,
            created_unix_secs: None,
        }
    }

    #[test]
    fn prefix_query_matches_through_separators() {
        let entries = vec![entry("search-results.xls"), entry("search.log")];
        let found = matches("search", &entries);
        assert_eq!(
            found
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["search-results.xls", "search.log"]
        );
    }

    #[test]
    fn ranks_prefix_then_word_start_then_substring() {
        let entries = vec![
            entry("xsearch.txt"),
            entry("great-search.txt"),
            entry("search-results.xls"),
        ];
        let found = matches("search", &entries);
        assert_eq!(
            found
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["search-results.xls", "great-search.txt", "xsearch.txt"]
        );
    }

    #[test]
    fn matches_unicode_case_insensitively_and_trims_spaces() {
        let entries = vec![
            entry("Résumé Été.txt"),
            entry("résumé final.txt"),
            entry("other.txt"),
        ];
        let found = matches("  RÉSUMÉ ÉTÉ  ", &entries);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Résumé Été.txt");

        let found = matches("ÉTÉ", &entries);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Résumé Été.txt");
    }

    #[test]
    fn empty_query_and_missing_names_match_nothing() {
        let entries = vec![entry("report.txt")];
        assert!(matches("", &entries).is_empty());
        assert!(matches("   \t ", &entries).is_empty());
        assert!(matches("missing", &entries).is_empty());
        assert!(matches("report", &[]).is_empty());
    }

    #[test]
    fn cap_applies_after_ranking() {
        let mut entries: Vec<FileEntry> = (0..120)
            .map(|index| entry(&format!("xneedle-{index:03}.txt")))
            .collect();
        entries.push(entry("needle.txt"));

        let found = matches("needle", &entries);
        assert_eq!(found.len(), MAX_RESULTS);
        assert_eq!(found[0].name, "needle.txt");
        assert_eq!(found[1].name, "xneedle-000.txt");
        assert_eq!(found[99].name, "xneedle-098.txt");
    }

    #[test]
    fn preserves_input_order_within_a_rank() {
        let entries = vec![
            entry("beta-report.txt"),
            entry("alpha-report.txt"),
            entry("gamma-report.txt"),
        ];
        let found = matches("report", &entries);
        assert_eq!(
            found
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["beta-report.txt", "alpha-report.txt", "gamma-report.txt"]
        );
    }

    #[test]
    fn later_word_start_outranks_an_earlier_substring() {
        let found = matches(
            "search",
            &[entry("xsearch.txt"), entry("research search.txt")],
        );
        assert_eq!(found[0].name, "research search.txt");
    }

    #[test]
    fn does_not_modify_the_source_entries() {
        let entries = vec![entry("search-results.xls"), entry("notes.txt")];
        let before = entries.clone();
        let _ = matches("search", &entries);
        assert_eq!(entries, before);
    }
}
