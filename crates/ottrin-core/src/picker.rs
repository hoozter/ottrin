//! The shared caller contract for Ottrin's on-demand file chooser.
//!
//! A chooser selects paths; it never creates, truncates, or edits the selected file.
//! Callers retain responsibility for opening it and handling later filesystem races.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

/// External native window identity. Linux follows the desktop portal convention;
/// cooperating Windows applications pass `win32:` followed by a hexadecimal HWND.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerParentWindow {
    None,
    X11(u32),
    Wayland(String),
    Windows(usize),
}

impl PickerParentWindow {
    pub fn parse(value: &str) -> Result<Self, String> {
        if value.is_empty() {
            return Ok(Self::None);
        }
        let invalid = || "Invalid picker parent window identifier".to_owned();
        let (kind, handle) = value.split_once(':').ok_or_else(invalid)?;
        if handle.is_empty() || handle.contains('\0') || handle.len() > 1024 {
            return Err(invalid());
        }
        let handle = if kind == "x11" {
            handle
                .strip_prefix("0x")
                .or_else(|| handle.strip_prefix("0X"))
                .unwrap_or(handle)
        } else {
            handle
        };
        match kind {
            "wayland" => Ok(Self::Wayland(handle.to_owned())),
            "x11" | "win32" if handle.bytes().all(|b| b.is_ascii_hexdigit()) => {
                let id = usize::from_str_radix(handle, 16).map_err(|_| invalid())?;
                if id == 0 {
                    return Err(invalid());
                }
                if kind == "x11" {
                    Ok(Self::X11(u32::try_from(id).map_err(|_| invalid())?))
                } else {
                    Ok(Self::Windows(id))
                }
            }
            _ => Err(invalid()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PickerMode {
    #[default]
    Open,
    Save,
    Folder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PickerPattern {
    Glob(String),
    Mime(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PickerFilter {
    pub name: String,
    pub patterns: Vec<PickerPattern>,
}

/// Empty options represent a boolean choice, with selected equal to true or false.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PickerChoice {
    pub id: String,
    pub label: String,
    pub options: Vec<(String, String)>,
    pub selected: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct PickerRequest {
    pub mode: PickerMode,
    pub title: String,
    pub accept_label: String,
    pub parent_window: String,
    #[serde(with = "crate::native_path::optional")]
    pub initial_directory: Option<PathBuf>,
    #[serde(with = "crate::native_path::os_string")]
    pub filename: std::ffi::OsString,
    pub multiple: bool,
    pub filters: Vec<PickerFilter>,
    pub current_filter: Option<usize>,
    pub choices: Vec<PickerChoice>,
}

impl PickerRequest {
    pub fn validate(&self) -> Result<(), String> {
        fn text(value: &str, name: &str, limit: usize) -> Result<(), String> {
            if value.contains('\0') || value.len() > limit {
                return Err(format!(
                    "Invalid {name}: contains NUL or exceeds {limit} bytes"
                ));
            }
            Ok(())
        }
        text(&self.title, "title", 1024)?;
        text(&self.accept_label, "accept label", 256)?;
        text(&self.parent_window, "parent window", 1024)?;
        PickerParentWindow::parse(&self.parent_window)?;
        if self.filename.as_encoded_bytes().len() > 4096 {
            return Err("The suggested filename exceeds4096 native encoded bytes".into());
        }
        if let Some(path) = &self.initial_directory {
            crate::native_path::NativePath::encode(path)
                .decode()
                .map_err(|error| format!("Invalid initial directory: {error}"))?;
            if !path.is_absolute() {
                return Err("The initial directory must be an absolute path".into());
            }
        }
        if self.multiple && self.mode == PickerMode::Save {
            return Err("Save requests select one filename".into());
        }
        if !self.filename.is_empty() && !is_picker_native_filename(&self.filename) {
            return Err("The suggested filename must be a single filename".into());
        }
        if self.filters.len() > 128 || self.choices.len() > 64 {
            return Err("Too many filters or choices".into());
        }
        if self
            .current_filter
            .is_some_and(|index| index >= self.filters.len())
        {
            return Err("The selected filter does not exist".into());
        }
        for filter in &self.filters {
            text(&filter.name, "filter name", 1024)?;
            if filter.patterns.is_empty() || filter.patterns.len() > 128 {
                return Err("A filter must contain between one and128 patterns".into());
            }
            for pattern in &filter.patterns {
                let value = match pattern {
                    PickerPattern::Glob(value) | PickerPattern::Mime(value) => value,
                };
                text(value, "filter pattern", 1024)?;
                if value.is_empty() {
                    return Err("Filter patterns cannot be empty".into());
                }
                if matches!(pattern, PickerPattern::Mime(_))
                    && (!value.contains('/') || value.contains(char::is_whitespace))
                {
                    return Err(format!("Invalid MIME filter: {value}"));
                }
            }
        }
        let mut ids = HashSet::new();
        for choice in &self.choices {
            text(&choice.id, "choice identifier", 256)?;
            text(&choice.label, "choice label", 1024)?;
            if choice.id.is_empty() || !ids.insert(&choice.id) {
                return Err("Choice identifiers must be nonempty and unique".into());
            }
            if choice.options.len() > 128 {
                return Err("Too many options in a choice".into());
            }
            let mut option_ids = HashSet::new();
            for (id, label) in &choice.options {
                text(id, "option identifier", 256)?;
                text(label, "option label", 1024)?;
                if id.is_empty() || !option_ids.insert(id) {
                    return Err("Option identifiers must be nonempty and unique".into());
                }
            }
            let selected_exists = if choice.options.is_empty() {
                matches!(choice.selected.as_str(), "true" | "false")
            } else {
                option_ids.contains(&choice.selected)
            };
            if !selected_exists {
                return Err(format!("Invalid selection for choice {}", choice.id));
            }
        }
        Ok(())
    }

    pub fn window_title(&self) -> &str {
        if !self.title.is_empty() {
            &self.title
        } else {
            match self.mode {
                PickerMode::Open => "Open file",
                PickerMode::Save => "Save file",
                PickerMode::Folder => "Choose folder",
            }
        }
    }

    pub fn action_label(&self) -> &str {
        if !self.accept_label.is_empty() {
            &self.accept_label
        } else {
            match self.mode {
                PickerMode::Open => "Open",
                PickerMode::Save => "Save",
                PickerMode::Folder => "Choose folder",
            }
        }
    }

    pub fn cancelled_response(&self) -> PickerResponse {
        PickerResponse {
            accepted: false,
            paths: Vec::new(),
            current_filter: self.current_filter,
            choices: self
                .choices
                .iter()
                .map(|choice| (choice.id.clone(), choice.selected.clone()))
                .collect(),
        }
    }
}

/// A save filename cannot escape the currently displayed directory. Reject both
/// separators on both platforms so cooperating applications share one contract.
pub fn is_picker_filename(value: &str) -> bool {
    is_picker_native_filename(std::ffi::OsStr::new(value))
}

pub fn is_picker_native_filename(value: &std::ffi::OsStr) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value
            .as_encoded_bytes()
            .iter()
            .any(|b| matches!(b, b'/' | b'\\' | b':' | 0))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PickerResponse {
    pub accepted: bool,
    #[serde(with = "crate::native_path::list")]
    pub paths: Vec<PathBuf>,
    pub current_filter: Option<usize>,
    pub choices: BTreeMap<String, String>,
}

impl PickerResponse {
    /// Check the response envelope before a cooperating caller uses any paths.
    /// Filesystem state can still change after the dialog returns.
    pub fn validate_for(&self, request: &PickerRequest) -> Result<(), String> {
        request.validate()?;
        if self.accepted {
            if self.paths.is_empty() || (!request.multiple && self.paths.len() != 1) {
                return Err("The chooser returned an invalid number of paths".into());
            }
            for path in &self.paths {
                crate::native_path::NativePath::encode(path)
                    .decode()
                    .map_err(|error| format!("Invalid selected path: {error}"))?;
            }
            let mut paths = HashSet::new();
            if self
                .paths
                .iter()
                .any(|path| !path.is_absolute() || !paths.insert(path))
            {
                return Err("The chooser returned a relative or duplicate path".into());
            }
        } else if !self.paths.is_empty() {
            return Err("A cancelled chooser cannot return selected paths".into());
        }
        if self
            .current_filter
            .is_some_and(|index| index >= request.filters.len())
        {
            return Err("The chooser returned an unknown filter".into());
        }
        if self.choices.len() != request.choices.len() {
            return Err("The chooser returned a different set of choices".into());
        }
        for choice in &request.choices {
            let selected = self
                .choices
                .get(&choice.id)
                .ok_or("A caller choice is missing")?;
            let valid = if choice.options.is_empty() {
                matches!(selected.as_str(), "true" | "false")
            } else {
                choice.options.iter().any(|(id, _)| id == selected)
            };
            if !valid {
                return Err(format!("Invalid returned choice {}", choice.id));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_parent_identity_is_explicit_and_lossless() {
        assert_eq!(
            PickerParentWindow::parse("").unwrap(),
            PickerParentWindow::None
        );
        assert_eq!(
            PickerParentWindow::parse("x11:ABC123").unwrap(),
            PickerParentWindow::X11(0xabc123)
        );
        for handle in ["x11:0x400004", "x11:0X400004"] {
            assert_eq!(
                PickerParentWindow::parse(handle).unwrap(),
                PickerParentWindow::X11(0x400004)
            );
        }
        assert_eq!(
            PickerParentWindow::parse("win32:1234").unwrap(),
            PickerParentWindow::Windows(0x1234)
        );
        assert_eq!(
            PickerParentWindow::parse("wayland:opaque:handle/α").unwrap(),
            PickerParentWindow::Wayland("opaque:handle/α".into())
        );
        for bad in [
            "1234",
            "x11:",
            "x11:0",
            "x11:0x",
            "x11:0x0",
            "x11:0x100000000",
            "x11:0x0x123",
            "x11:100000000",
            "x11:-1",
            "x11: 123",
            "win32:0",
            "win32:GG",
            "wayland:",
            "wayland:bad\0",
            "other:123",
        ] {
            let request = PickerRequest {
                parent_window: bad.into(),
                ..Default::default()
            };
            assert!(request.validate().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn caller_requests_reject_ambiguous_choices_and_path_escaping_filenames() {
        let mut request = PickerRequest {
            mode: PickerMode::Save,
            filename: "theme.json".into(),
            ..Default::default()
        };
        assert!(request.validate().is_ok());
        for filename in [
            "../theme.json",
            "sub/theme.json",
            "sub\\theme.json",
            "..",
            ".",
            "bad\0name",
            "C:theme.json",
        ] {
            request.filename = filename.into();
            assert!(request.validate().is_err(), "{filename:?}");
        }
        request.filename.clear();
        request.multiple = true;
        assert!(request.validate().is_err());
        request.multiple = false;
        request.choices = vec![PickerChoice {
            id: "encoding".into(),
            label: "Encoding".into(),
            options: vec![("utf8".into(), "UTF-8".into())],
            selected: "other".into(),
        }];
        assert!(request.validate().is_err());
        request.choices[0].selected = "utf8".into();
        assert!(request.validate().is_ok());
        request.choices.push(request.choices[0].clone());
        assert!(request.validate().is_err());
    }

    #[test]
    fn cancellation_keeps_caller_choice_identities_and_returns_no_paths() {
        let request = PickerRequest {
            choices: vec![PickerChoice {
                id: "readonly".into(),
                label: "Read only".into(),
                options: Vec::new(),
                selected: "true".into(),
            }],
            ..Default::default()
        };
        assert!(request.validate().is_ok());
        let result = request.cancelled_response();
        assert!(result.validate_for(&request).is_ok());
        assert!(!result.accepted);
        assert!(result.paths.is_empty());
        assert_eq!(result.choices["readonly"], "true");
        assert_eq!(request.action_label(), "Open");
        assert_eq!(
            serde_json::from_str::<PickerResponse>(&serde_json::to_string(&result).unwrap())
                .unwrap(),
            result
        );
    }
    #[cfg(any(unix, windows))]
    #[test]
    fn picker_request_and_response_transport_preserves_native_path_units() {
        #[cfg(unix)]
        let path = {
            use std::os::unix::ffi::OsStringExt;
            PathBuf::from(std::ffi::OsString::from_vec(
                b"/folder\x81/item\x80.txt".to_vec(),
            ))
        };
        #[cfg(windows)]
        let path = {
            use std::os::windows::ffi::OsStringExt;
            PathBuf::from(std::ffi::OsString::from_wide(&[
                67, 58, 92, 102, 111, 108, 100, 101, 114, 92, 0xd800, 46, 116, 120, 116,
            ]))
        };
        let request = PickerRequest {
            initial_directory: Some(path.parent().unwrap().into()),
            filename: path.file_name().unwrap().into(),
            ..Default::default()
        };
        let response = PickerResponse {
            accepted: true,
            paths: vec![path],
            current_filter: None,
            choices: BTreeMap::new(),
        };
        request.validate().unwrap();
        let request_json =
            serde_json::to_vec(&request).expect("Native caller folder must be serializable");
        assert_eq!(
            serde_json::from_slice::<PickerRequest>(&request_json).unwrap(),
            request
        );
        let response_json = serde_json::to_vec(&response)
            .expect("Native selected path must be serializable without loss");
        assert_eq!(
            serde_json::from_slice::<PickerResponse>(&response_json).unwrap(),
            response
        );
        response.validate_for(&request).unwrap();
    }
    #[test]
    fn filename_wire_rejects_text_foreign_units_and_path_components() {
        assert!(serde_json::from_str::<PickerRequest>(r#"{"filename":"theme.json"}"#).is_err());
        for value in [
            r#"{"encoding":"unix-bytes","units_hex":"6100"}"#,
            r#"{"encoding":"windows-utf16-le","units_hex":"00610000"}"#,
        ] {
            assert!(
                serde_json::from_str::<PickerRequest>(&format!("{{\"filename\":{value}}}"))
                    .is_err()
            );
        }
        let value = crate::native_path::NativePath::encode(std::path::Path::new("../escape"));
        let input = serde_json::json!({"filename":value});
        let request: PickerRequest = serde_json::from_value(input).unwrap();
        assert!(request.validate().is_err());
    }
    #[test]
    fn path_wire_rejects_strings_foreign_encodings_unknown_fields_and_nul_units() {
        assert!(
            serde_json::from_str::<PickerRequest>(r#"{"initial_directory":"/old-string-wire"}"#)
                .is_err()
        );
        for path in [
            r#"{"encoding":"unix-bytes","units_hex":"0"}"#,
            r#"{"encoding":"unix-bytes","units_hex":"2f00"}"#,
            r#"{"encoding":"windows-utf16-le","units_hex":"0043003a005c0000"}"#,
            r#"{"encoding":"unix-bytes","units_hex":"2f61","extra":true}"#,
        ] {
            let input = format!("{{\"initial_directory\":{path}}}");
            assert!(
                serde_json::from_str::<PickerRequest>(&input).is_err(),
                "{input}"
            );
        }
    }
}
