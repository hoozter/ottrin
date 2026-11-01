//! Lossless edits to individual desktop preference keys.
//! Other backends, groups, comments and line endings remain untouched.

pub(super) fn value(data: &str, group: &str, key: &str) -> Result<Option<String>, String> {
    let wanted_key = key;
    let mut preferred = false;
    let mut found = None;
    let mut groups = 0;
    for line in data.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with(';') || line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            preferred = line == format!("[{group}]");
            if preferred {
                groups += 1;
                if groups > 1 {
                    return Err(format!("Duplicate {group} groups; configuration retained"));
                }
            }
        } else if preferred
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == wanted_key
        {
            if found.is_some() {
                return Err(format!(
                    "Duplicate {wanted_key} preferences; configuration retained"
                ));
            }
            found = Some(value.trim().to_owned());
        }
    }
    Ok(found)
}

pub(super) fn replace(
    data: &str,
    group: &str,
    key: &str,
    value: Option<&str>,
) -> Result<String, String> {
    self::value(data, group, key)?;
    let wanted_key = key;
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
                    result.push_str(&format!("{wanted_key}={value}{eol}"));
                }
                written = true;
            }
            preferred = trimmed == format!("[{group}]");
            group_seen |= preferred;
        }
        if preferred
            && let Some((key, _)) = trimmed.split_once('=')
            && key.trim() == wanted_key
        {
            if let Some(value) = value {
                result.push_str(&format!("{wanted_key}={value}{eol}"));
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
            result.push_str(&format!("[{group}]{eol}"));
        }
        result.push_str(&format!("{wanted_key}={}{eol}", value));
    }
    Ok(result)
}

/// Remove only our provider from a changed list. A sole owned provider restores
/// the previous choice; a foreign-only choice is left untouched.
pub(super) fn restore_provider(
    current: Option<&str>,
    owner: &str,
    previous: Option<&str>,
) -> (bool, Option<String>) {
    let values = current
        .unwrap_or_default()
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if !values.contains(&owner) {
        return (false, current.map(str::to_owned));
    }
    let remaining = values
        .into_iter()
        .filter(|value| *value != owner)
        .collect::<Vec<_>>();
    (
        true,
        if remaining.is_empty() {
            previous.map(str::to_owned)
        } else {
            Some(format!("{};", remaining.join(";")))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    const GROUP: &str = "preferred";
    const KEY: &str = "org.freedesktop.impl.portal.FileChooser";
    #[test]
    fn changes_only_chooser_and_restores_it_after_unrelated_edits() {
        let original = "# keep\r\n[preferred]\r\ndefault=kde;gtk\r\norg.freedesktop.impl.portal.FileChooser = kde\r\n[other]\r\nkey=value\r\n";
        let enabled = replace(original, GROUP, KEY, Some("ottrin")).unwrap();
        assert_eq!(
            value(&enabled, GROUP, KEY).unwrap().as_deref(),
            Some("ottrin")
        );
        let edited = enabled.replace("key=value", "key=changed");
        let restored = replace(&edited, GROUP, KEY, Some("kde")).unwrap();
        assert!(restored.contains("default=kde;gtk\r\n"));
        assert!(restored.contains("key=changed\r\n"));
        assert!(restored.starts_with("# keep\r\n"));
    }
    #[test]
    fn missing_key_is_added_inside_preferred_group() {
        let input = "[preferred]\ndefault=gtk\n[another]\nkey=value\n";
        let enabled = replace(input, GROUP, KEY, Some("ottrin")).unwrap();
        assert_eq!(
            value(&enabled, GROUP, KEY).unwrap().as_deref(),
            Some("ottrin")
        );
        assert_eq!(replace(&enabled, GROUP, KEY, None).unwrap(), input);
    }
    #[test]
    fn no_final_newline_or_group_is_supported_and_ambiguous_input_is_retained() {
        assert_eq!(
            value(
                &replace("# keep", GROUP, KEY, Some("ottrin")).unwrap(),
                GROUP,
                KEY
            )
            .unwrap()
            .as_deref(),
            Some("ottrin")
        );
        assert!(replace("[preferred]\n[preferred]\n", GROUP, KEY, Some("ottrin")).is_err());
        assert!(
            replace(
                &format!("[preferred]\n{KEY}=gtk\n{KEY}=kde\n"),
                GROUP,
                KEY,
                Some("ottrin")
            )
            .is_err()
        );
    }
}
