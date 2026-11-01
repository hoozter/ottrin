//! Lossless edits to the one portal preference owned by Ottrin.
//! Other backends, groups, comments and line endings remain untouched.
const KEY: &str = "org.freedesktop.impl.portal.FileChooser";

pub(super) fn chooser(data: &str) -> Result<Option<String>, String> {
    let mut preferred = false;
    let mut found = None;
    let mut groups = 0;
    for line in data.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with(';') || line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            preferred = line == "[preferred]";
            if preferred {
                groups += 1;
                if groups > 1 {
                    return Err("Duplicate portal preference groups; configuration retained".into());
                }
            }
        } else if preferred
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == KEY
        {
            if found.is_some() {
                return Err("Duplicate FileChooser preferences; configuration retained".into());
            }
            found = Some(value.trim().to_owned());
        }
    }
    Ok(found)
}

pub(super) fn replace(data: &str, value: Option<&str>) -> Result<String, String> {
    chooser(data)?;
    let eol = if data.contains("\r\n") { "\r\n" } else { "\n" };
    let mut result = String::new();
    let mut preferred = false;
    let mut group_seen = false;
    let mut written = false;
    for line in data.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if preferred && !written {
                if let Some(value) = value {
                    result.push_str(&format!("{KEY}={value}{eol}"));
                }
                written = true;
            }
            preferred = trimmed == "[preferred]";
            group_seen |= preferred;
        }
        if preferred
            && let Some((key, _)) = trimmed.split_once('=')
            && key.trim() == KEY
        {
            if let Some(value) = value {
                result.push_str(&format!("{KEY}={value}{eol}"));
            }
            written = true;
        } else {
            result.push_str(line);
        }
    }
    if !written && let Some(value) = value {
        if !result.is_empty() && !result.ends_with('\n') {
            result.push_str(eol);
        }
        if !group_seen {
            result.push_str(&format!("[preferred]{eol}"));
        }
        result.push_str(&format!("{KEY}={}{eol}", value));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changes_only_chooser_and_restores_it_after_unrelated_edits() {
        let original = "# keep\r\n[preferred]\r\ndefault=kde;gtk\r\norg.freedesktop.impl.portal.FileChooser = kde\r\n[other]\r\nkey=value\r\n";
        let enabled = replace(original, Some("ottrin")).unwrap();
        assert_eq!(chooser(&enabled).unwrap().as_deref(), Some("ottrin"));
        let edited = enabled.replace("key=value", "key=changed");
        let restored = replace(&edited, Some("kde")).unwrap();
        assert!(restored.contains("default=kde;gtk\r\n"));
        assert!(restored.contains("key=changed\r\n"));
        assert!(restored.starts_with("# keep\r\n"));
    }
    #[test]
    fn missing_key_is_added_inside_preferred_group() {
        let input = "[preferred]\ndefault=gtk\n[another]\nkey=value\n";
        let enabled = replace(input, Some("ottrin")).unwrap();
        assert_eq!(chooser(&enabled).unwrap().as_deref(), Some("ottrin"));
        assert_eq!(replace(&enabled, None).unwrap(), input);
    }
    #[test]
    fn no_final_newline_or_group_is_supported_and_ambiguous_input_is_retained() {
        assert_eq!(
            chooser(&replace("# keep", Some("ottrin")).unwrap())
                .unwrap()
                .as_deref(),
            Some("ottrin")
        );
        assert!(replace("[preferred]\n[preferred]\n", Some("ottrin")).is_err());
        assert!(
            replace(
                &format!("[preferred]\n{KEY}=gtk\n{KEY}=kde\n"),
                Some("ottrin")
            )
            .is_err()
        );
    }
}
