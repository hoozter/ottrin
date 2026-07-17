use ottrin_core::{SavedTheme, ThemeCustomization, ThemeMode, ThemePreset};
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
    default_preset: ThemePreset,
    default_mode: ThemeMode,
) -> Result<SavedTheme, String> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|err| format!("invalid JSON: {}", err))?;

    if let Some(theme_value) = value.get("theme") {
        let mut saved: SavedTheme = serde_json::from_value(theme_value.clone())
            .map_err(|err| format!("invalid theme payload: {}", err))?;
        if saved.name.trim().is_empty() {
            saved.name = fallback_name.to_string();
        }
        saved.customization.enabled = true;
        return Ok(saved);
    }

    if let Ok(mut saved) = serde_json::from_value::<SavedTheme>(value.clone()) {
        if saved.name.trim().is_empty() {
            saved.name = fallback_name.to_string();
        }
        saved.customization.enabled = true;
        return Ok(saved);
    }

    let mut custom: ThemeCustomization = serde_json::from_value(value)
        .map_err(|err| format!("expected Ottrin theme JSON: {}", err))?;
    custom.enabled = true;
    Ok(SavedTheme {
        name: fallback_name.to_string(),
        theme_mode: default_mode,
        base_preset: default_preset,
        customization: custom,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_theme_import_accepts_wrapped_payload() {
        let wrapped = serde_json::json!({
            "format": "ottrin-theme",
            "version": 1,
            "theme": {
                "name": "Wrapped",
                "theme_mode": "Dark",
                "base_preset": "Nord",
                "customization": ThemeCustomization::default(),
            }
        });
        let bytes = serde_json::to_vec(&wrapped).unwrap();
        let imported =
            decode_theme_import_json(&bytes, "Fallback", ThemePreset::Ottrin, ThemeMode::System)
                .unwrap();
        assert_eq!(imported.name, "Wrapped");
        assert_eq!(imported.base_preset, ThemePreset::Nord);
        assert_eq!(imported.theme_mode, ThemeMode::Dark);
        assert!(imported.customization.enabled);
    }

    #[test]
    fn decode_theme_import_accepts_saved_theme_payload() {
        let saved = SavedTheme {
            name: "Saved".to_string(),
            theme_mode: ThemeMode::Light,
            base_preset: ThemePreset::Solarized,
            customization: ThemeCustomization::default(),
        };
        let bytes = serde_json::to_vec(&saved).unwrap();
        let imported =
            decode_theme_import_json(&bytes, "Fallback", ThemePreset::Ottrin, ThemeMode::System)
                .unwrap();
        assert_eq!(imported.name, "Saved");
        assert_eq!(imported.base_preset, ThemePreset::Solarized);
        assert_eq!(imported.theme_mode, ThemeMode::Light);
        assert!(imported.customization.enabled);
    }

    #[test]
    fn decode_theme_import_accepts_customization_payload() {
        let custom = ThemeCustomization {
            accent: [1, 2, 3, 255],
            ..ThemeCustomization::default()
        };
        let bytes = serde_json::to_vec(&custom).unwrap();
        let imported = decode_theme_import_json(
            &bytes,
            "Fallback Name",
            ThemePreset::Breeze,
            ThemeMode::Dark,
        )
        .unwrap();
        assert_eq!(imported.name, "Fallback Name");
        assert_eq!(imported.base_preset, ThemePreset::Breeze);
        assert_eq!(imported.theme_mode, ThemeMode::Dark);
        assert_eq!(imported.customization.accent, [1, 2, 3, 255]);
        assert!(imported.customization.enabled);
    }
}
