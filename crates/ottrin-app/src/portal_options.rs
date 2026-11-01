//! Translation at the Linux portal boundary; native path bytes never pass through display text.
use ottrin_core::picker::{
    PickerChoice, PickerFilter, PickerMode, PickerPattern, PickerRequest, PickerResponse,
};
use ottrin_platform::local_file_uri as file_uri;
#[cfg(test)]
use std::os::unix::ffi::OsStrExt;
use std::{
    collections::HashMap,
    ffi::OsString,
    os::unix::ffi::OsStringExt,
    path::{Path, PathBuf},
};
use zbus::zvariant::{OwnedValue, Value};
pub(super) type Options = HashMap<String, OwnedValue>;
type Filter = (String, Vec<(u32, String)>);
type Choice = (String, String, Vec<(String, String)>, String);
#[derive(Clone, Copy)]
pub(super) enum Method {
    Open,
    Save,
    SaveFiles,
}
pub(super) struct Request {
    pub picker: PickerRequest,
    pub files: Vec<OsString>,
}
fn option<T>(options: &mut Options, key: &str) -> Result<Option<T>, String>
where
    T: TryFrom<OwnedValue>,
    T::Error: std::fmt::Display,
{
    options
        .remove(key)
        .map(|v| T::try_from(v).map_err(|e| format!("Invalid portal option {key}: {e}")))
        .transpose()
}
fn mnemonic_label(label: &str) -> String {
    let mut chars = label.chars().peekable();
    let mut result = String::new();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                chars.next();
                result.push('_');
            }
        } else {
            result.push(c);
        }
    }
    result
}
fn native(bytes: Vec<u8>) -> Result<OsString, String> {
    let bytes = bytes.strip_suffix(&[0]).unwrap_or(&bytes);
    if bytes.contains(&0) {
        return Err("Portal pathname contains an embedded NUL".into());
    }
    Ok(OsString::from_vec(bytes.to_vec()))
}
fn filter(value: Filter) -> Result<PickerFilter, String> {
    Ok(PickerFilter {
        name: value.0,
        patterns: value
            .1
            .into_iter()
            .map(|(kind, value)| match kind {
                0 => Ok(PickerPattern::Glob(value)),
                1 => Ok(PickerPattern::Mime(value)),
                _ => Err("Unknown portal filter kind".into()),
            })
            .collect::<Result<_, String>>()?,
    })
}
pub(super) fn request(
    method: Method,
    parent: String,
    title: String,
    mut options: Options,
) -> Result<Request, String> {
    // Toolkit callers retain responsibility for their own modal input state. The
    // native parent is imported by the chooser; no caller window is disabled here.
    let _modal = option::<bool>(&mut options, "modal")?.unwrap_or(true);
    let directory = option::<bool>(&mut options, "directory")?.unwrap_or(false);
    let mut picker = PickerRequest {
        parent_window: parent,
        title,
        mode: match method {
            Method::Open if directory => PickerMode::Folder,
            Method::Open => PickerMode::Open,
            Method::Save => PickerMode::Save,
            Method::SaveFiles => PickerMode::Folder,
        },
        ..Default::default()
    };
    picker.accept_label =
        mnemonic_label(&option::<String>(&mut options, "accept_label")?.unwrap_or_default());
    picker.multiple = matches!(method, Method::Open)
        && option::<bool>(&mut options, "multiple")?.unwrap_or(false);
    if let Some(folder) = option::<Vec<u8>>(&mut options, "current_folder")? {
        let folder = native(folder)?;
        if !folder.is_empty() {
            picker.initial_directory = Some(PathBuf::from(folder));
        }
    }
    if matches!(method, Method::Save) {
        picker.filename = option::<String>(&mut options, "current_name")?
            .unwrap_or_default()
            .into();
        if let Some(file) = option::<Vec<u8>>(&mut options, "current_file")? {
            let file = PathBuf::from(native(file)?);
            if !file.is_absolute() {
                return Err("Portal current_file must be absolute".into());
            }
            picker.filename = file
                .file_name()
                .ok_or("Portal current_file needs a filename")?
                .into();
            picker.initial_directory = file.parent().map(Path::to_path_buf);
        }
    }
    if !matches!(method, Method::SaveFiles) {
        picker.filters = option::<Vec<Filter>>(&mut options, "filters")?
            .unwrap_or_default()
            .into_iter()
            .map(filter)
            .collect::<Result<_, _>>()?;
        if let Some(current) = option::<Filter>(&mut options, "current_filter")? {
            let current = filter(current)?;
            // A portal may supply a current filter without listing any others.
            let index = picker
                .filters
                .iter()
                .position(|f| f == &current)
                .unwrap_or_else(|| {
                    picker.filters.push(current);
                    picker.filters.len() - 1
                });
            picker.current_filter = Some(index);
        }
    }
    picker.choices = option::<Vec<Choice>>(&mut options, "choices")?
        .unwrap_or_default()
        .into_iter()
        .map(|(id, label, options, mut selected)| {
            if selected.is_empty() {
                selected = options
                    .first()
                    .map(|(id, _)| id.clone())
                    .unwrap_or_else(|| "false".into());
            }
            PickerChoice {
                id,
                label,
                options,
                selected,
            }
        })
        .collect();
    let files = if matches!(method, Method::SaveFiles) {
        option::<Vec<Vec<u8>>>(&mut options, "files")?
            .ok_or("SaveFiles requires filenames")?
            .into_iter()
            .map(native)
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };
    if matches!(method, Method::SaveFiles)
        && (files.is_empty()
            || files
                .iter()
                .any(|f| !ottrin_core::picker::is_picker_native_filename(f)))
    {
        return Err("SaveFiles requires single-component native filenames".into());
    }
    ottrin_ui::PickerApp::validate_request(&picker)?;
    Ok(Request { picker, files })
}
fn value<T: Into<Value<'static>>>(value: T) -> Result<OwnedValue, String> {
    OwnedValue::try_from(value.into()).map_err(|e| e.to_string())
}

fn available_path(
    directory: &Path,
    name: &std::ffi::OsStr,
    reserved: &mut std::collections::HashSet<PathBuf>,
) -> Result<PathBuf, String> {
    let base = directory.join(name);
    let name = Path::new(name);
    let stem = name.file_stem().unwrap_or(name.as_os_str());
    for suffix in 0u64.. {
        let candidate = if suffix == 0 {
            base.clone()
        } else {
            let mut candidate = stem.to_os_string();
            candidate.push(format!(" ({suffix})"));
            if let Some(ext) = name.extension() {
                candidate.push(".");
                candidate.push(ext);
            }
            directory.join(candidate)
        };
        if reserved.contains(&candidate) {
            continue;
        }
        match std::fs::symlink_metadata(&candidate) {
            Ok(_) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                reserved.insert(candidate.clone());
                return Ok(candidate);
            }
            Err(e) => return Err(format!("Could not check SaveFiles destination: {e}")),
        }
    }
    Err("No available SaveFiles filename".into())
}
pub(super) fn response(
    request: &Request,
    response: PickerResponse,
) -> Result<(u32, Options), String> {
    response.validate_for(&request.picker)?;
    if !response.accepted {
        return Ok((1, HashMap::new()));
    }
    let paths = if request.files.is_empty() {
        response.paths
    } else {
        let directory = response
            .paths
            .first()
            .ok_or("SaveFiles returned no directory")?;
        let mut reserved = std::collections::HashSet::new();
        request
            .files
            .iter()
            .map(|name| available_path(directory, name, &mut reserved))
            .collect::<Result<_, _>>()?
    };
    let mut result = HashMap::new();
    result.insert(
        "uris".into(),
        value(
            paths
                .iter()
                .map(|p| file_uri(p))
                .collect::<Result<Vec<_>, _>>()?,
        )?,
    );
    result.insert(
        "choices".into(),
        value(response.choices.into_iter().collect::<Vec<_>>())?,
    );
    if let Some(index) = response.current_filter {
        let f = request
            .picker
            .filters
            .get(index)
            .ok_or("Invalid returned filter")?;
        let patterns = f
            .patterns
            .iter()
            .map(|p| match p {
                PickerPattern::Glob(s) => (0u32, s.clone()),
                PickerPattern::Mime(s) => (1, s.clone()),
            })
            .collect::<Vec<_>>();
        result.insert("current_filter".into(), value((f.name.clone(), patterns))?);
    }
    Ok((0, result))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn put<T: Into<Value<'static>>>(map: &mut Options, key: &str, v: T) {
        map.insert(key.into(), value(v).unwrap());
    }
    #[test]
    fn open_filters_choices_and_labels_follow_portal_types() {
        let mut options = Options::new();
        put(&mut options, "multiple", true);
        put(&mut options, "accept_label", "_Open__file".to_owned());
        put(
            &mut options,
            "filters",
            vec![("Text".to_owned(), vec![(0u32, "*.txt".to_owned())])],
        );
        put(
            &mut options,
            "current_filter",
            ("Images".to_owned(), vec![(1u32, "image/*".to_owned())]),
        );
        put(
            &mut options,
            "choices",
            vec![
                (
                    "readonly".to_owned(),
                    "Read only".to_owned(),
                    vec![],
                    "".to_owned(),
                ),
                (
                    "encoding".to_owned(),
                    "Encoding".to_owned(),
                    vec![("utf8".to_owned(), "UTF-8".to_owned())],
                    "".to_owned(),
                ),
            ],
        );
        let request = request(Method::Open, "".into(), "Open".into(), options).unwrap();
        assert!(request.picker.multiple);
        assert_eq!(request.picker.accept_label, "Open_file");
        assert_eq!(request.picker.current_filter, Some(1));
        assert_eq!(request.picker.choices[0].selected, "false");
        assert_eq!(request.picker.choices[1].selected, "utf8");
    }
    #[test]
    fn native_current_file_wins_over_other_save_hints() {
        let mut options = Options::new();
        put(&mut options, "current_folder", b"/other\0".to_vec());
        put(&mut options, "current_name", "different.txt".to_owned());
        put(
            &mut options,
            "current_file",
            b"/folder\x81/name\x80.txt\0".to_vec(),
        );
        let request = request(Method::Save, "".into(), "Save".into(), options).unwrap();
        assert_eq!(
            request.picker.initial_directory,
            Some(PathBuf::from(OsString::from_vec(b"/folder\x81".to_vec())))
        );
        assert_eq!(request.picker.filename.as_bytes(), b"name\x80.txt");
        let path = request
            .picker
            .initial_directory
            .as_ref()
            .unwrap()
            .join(&request.picker.filename);
        let (code, reply) = response(
            &request,
            PickerResponse {
                accepted: true,
                paths: vec![path],
                current_filter: None,
                choices: Default::default(),
            },
        )
        .unwrap();
        assert_eq!(code, 0);
        assert_eq!(
            Vec::<String>::try_from(reply["uris"].try_clone().unwrap()).unwrap(),
            ["file:///folder%81/name%80.txt"]
        );
    }
    #[test]
    fn invalid_byte_paths_filters_and_choices_are_refused_before_ui() {
        for bytes in [b"/folder\0extra\0".to_vec(), b"relative\0".to_vec()] {
            let mut options = Options::new();
            put(&mut options, "current_folder", bytes);
            assert!(request(Method::Open, "".into(), "".into(), options).is_err());
        }
        let mut options = Options::new();
        put(
            &mut options,
            "filters",
            vec![("Invalid".to_owned(), vec![(0u32, "[".to_owned())])],
        );
        assert!(request(Method::Open, "".into(), "".into(), options).is_err());
        let mut options = Options::new();
        put(
            &mut options,
            "filters",
            vec![("Invalid".to_owned(), vec![(5u32, "*.txt".to_owned())])],
        );
        assert!(request(Method::Open, "".into(), "".into(), options).is_err());
        let mut options = Options::new();
        put(&mut options, "multiple", "true".to_owned());
        assert!(request(Method::Open, "".into(), "".into(), options).is_err());
    }
    #[test]
    fn save_files_keep_order_avoid_existing_and_duplicate_names_and_create_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir
            .path()
            .join(OsString::from_vec(b"name\x80.txt".to_vec()));
        std::fs::write(&original, b"unchanged").unwrap();
        let mut options = Options::new();
        put(
            &mut options,
            "files",
            vec![
                b"name\x80.txt\0".to_vec(),
                b"name\x80.txt\0".to_vec(),
                b"other.txt\0".to_vec(),
            ],
        );
        let request = request(Method::SaveFiles, "".into(), "Save files".into(), options).unwrap();
        assert_eq!(request.picker.mode, PickerMode::Folder);
        let (code, mut reply) = response(
            &request,
            PickerResponse {
                accepted: true,
                paths: vec![dir.path().into()],
                current_filter: None,
                choices: Default::default(),
            },
        )
        .unwrap();
        assert_eq!(code, 0);
        let uris = Vec::<String>::try_from(reply.remove("uris").unwrap()).unwrap();
        let decoded: Vec<_> = uris
            .iter()
            .map(|uri| ottrin_platform::local_path_from_uri(uri).unwrap())
            .collect();
        assert_eq!(
            decoded,
            vec![
                dir.path()
                    .join(OsString::from_vec(b"name\x80 (1).txt".to_vec())),
                dir.path()
                    .join(OsString::from_vec(b"name\x80 (2).txt".to_vec())),
                dir.path().join("other.txt"),
            ]
        );
        assert_eq!(std::fs::read(original).unwrap(), b"unchanged");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn cancelled_responses_disclose_no_paths_or_choice_results() {
        let request = request(Method::Open, "".into(), "".into(), Options::new()).unwrap();
        let (code, result) = response(&request, request.picker.cancelled_response()).unwrap();
        assert_eq!(code, 1);
        assert!(result.is_empty());
    }
    #[test]
    fn file_uris_encode_raw_units_metacharacters_and_unicode_without_loss() {
        assert_eq!(
            file_uri(Path::new("/a b/#%é")).unwrap(),
            "file:///a%20b/%23%25%C3%A9"
        );
        assert!(file_uri(Path::new("relative")).is_err());
        let mut options = Options::new();
        put(&mut options, "files", vec![b"../escape\0".to_vec()]);
        assert!(request(Method::SaveFiles, "".into(), "".into(), options).is_err());
    }
}
