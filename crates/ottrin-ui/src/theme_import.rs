use ottrin_core::SavedTheme;
use std::path::Path;

pub(crate) fn sanitize_theme_file_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        "ottrin-theme.json".to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn theme_name_from_path(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Imported theme");
    stem.to_string()
}

pub(crate) fn decode_theme_import_json(
    bytes: &[u8],
    fallback_name: &str,
) -> Result<SavedTheme, String> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|err| format!("invalid JSON: {}", err))?;
    let envelope = value
        .as_object()
        .ok_or_else(|| "expected ottrin-theme v2 export envelope".to_string())?;
    if envelope.get("format").and_then(serde_json::Value::as_str) != Some("ottrin-theme")
        || envelope.get("version").and_then(serde_json::Value::as_u64) != Some(2)
    {
        return Err("expected ottrin-theme v2 export envelope".to_string());
    }
    let theme = envelope
        .get("theme")
        .cloned()
        .ok_or_else(|| "expected ottrin-theme v2 export envelope".to_string())?;
    let mut saved: SavedTheme = serde_json::from_value(theme)
        .map_err(|err| format!("invalid ottrin-theme v2 payload: {err}"))?;
    if saved.name.trim().is_empty() {
        saved.name = fallback_name.to_string();
    }
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ottrin_core::{ThemeCustomization, ThemePreset};

    #[test]
    fn decode_theme_import_accepts_wrapped_payload() {
        let wrapped = serde_json::json!({
            "format": "ottrin-theme",
            "version": 2,
            "theme": {
                "name": "Wrapped",
                "base_preset": "Nord",
                "light": ThemeCustomization::default(),
                "dark": ThemeCustomization::default(),
            }
        });
        let bytes = serde_json::to_vec(&wrapped).unwrap();
        let imported = decode_theme_import_json(&bytes, "Fallback").unwrap();
        assert_eq!(imported.name, "Wrapped");
        assert_eq!(imported.base_preset, ThemePreset::Nord);
        assert_eq!(imported.dark, ThemeCustomization::default());
    }

    #[test]
    fn decode_theme_import_rejects_unwrapped_saved_theme_payload() {
        let saved = SavedTheme {
            name: "Saved".to_string(),
            base_preset: ThemePreset::Solarized,
            dark: (ThemeCustomization::default()).clone(),
            light: ThemeCustomization::default(),
        };
        let bytes = serde_json::to_vec(&saved).unwrap();
        let error = decode_theme_import_json(&bytes, "Fallback")
            .expect_err("unwrapped SavedTheme is not an Ottrin theme export");

        assert!(error.contains("ottrin-theme"), "unexpected error: {error}");
    }

    #[test]
    fn decode_theme_import_rejects_unwrapped_customization_payload() {
        let custom = ThemeCustomization {
            accent: [1, 2, 3, 255],
            ..ThemeCustomization::default()
        };
        let bytes = serde_json::to_vec(&custom).unwrap();
        let error = decode_theme_import_json(&bytes, "Fallback Name")
            .expect_err("unwrapped customization is not an Ottrin theme export");

        assert!(error.contains("ottrin-theme"), "unexpected error: {error}");
    }

    #[test]
    fn decode_theme_import_rejects_non_v2_envelopes() {
        let theme = SavedTheme {
            name: "Strict".to_string(),
            base_preset: ThemePreset::Nord,
            dark: (ThemeCustomization::default()).clone(),
            light: ThemeCustomization::default(),
        };
        for payload in [
            serde_json::json!({"version": 2, "theme": theme}),
            serde_json::json!({"format": "other-theme", "version": 2, "theme": theme}),
            serde_json::json!({"format": "ottrin-theme", "version": 1, "theme": theme}),
        ] {
            let bytes = serde_json::to_vec(&payload).unwrap();
            let error = decode_theme_import_json(&bytes, "Fallback")
                .expect_err("only versioned Ottrin exports are accepted");
            assert!(error.contains("ottrin-theme"), "unexpected error: {error}");
        }
    }
}
