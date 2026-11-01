//! Native-unit file URI conversion shared by desktop integrations.
use std::path::{Path, PathBuf};
pub fn local_path_from_uri(uri: &str) -> Result<PathBuf, String> {
    if uri.contains('\0') {
        return Err("A file URI cannot contain NUL".into());
    }
    let (path, host) = gio::glib::filename_from_uri(uri).map_err(|e| e.to_string())?;
    if host
        .as_ref()
        .is_some_and(|host| !host.is_empty() && !host.eq_ignore_ascii_case("localhost"))
    {
        return Err("Only local file URIs are supported".into());
    }
    if !path.is_absolute() {
        return Err("A file URI must have an absolute path".into());
    }
    Ok(path)
}
pub fn local_file_uri(path: &Path) -> Result<String, String> {
    if !path.is_absolute() {
        return Err("A file URI needs an absolute path".into());
    }
    use std::os::unix::ffi::OsStrExt;
    if path.as_os_str().as_bytes().contains(&0) {
        return Err("A path cannot contain NUL".into());
    }
    gio::glib::filename_to_uri(path, None)
        .map(|uri| uri.to_string())
        .map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_file_uri_roundtrip_and_remote_rejection() {
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_vec(
            b"/tmp/native\x80 space %?#".to_vec(),
        ));
        assert_eq!(
            local_path_from_uri(&local_file_uri(&path).unwrap()).unwrap(),
            path
        );
        assert_eq!(
            local_path_from_uri("file://localhost/tmp/a").unwrap(),
            Path::new("/tmp/a")
        );
        for value in [
            "file://remote/tmp/a",
            "https://example.com/a",
            "file:///tmp/%00",
            "file:///tmp/a\0b",
            "relative",
        ] {
            assert!(local_path_from_uri(value).is_err(), "{value:?}");
        }
    }
}
