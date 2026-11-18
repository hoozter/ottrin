use super::*;

pub(super) mod native_folder {
    use super::*;
    pub fn serialize<S: serde::Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
        native_path::NativePath::encode(path).serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<PathBuf, D::Error> {
        native_path::NativePath::deserialize(deserializer)?
            .decode()
            .map_err(serde::de::Error::custom)
    }
}

fn folder_key(path: &Path) -> String {
    if let Some(path) = path.to_str() {
        path.to_owned()
    } else {
        // NUL cannot occur in a filesystem path, so this prefix cannot collide
        // with a valid Unicode path. Keep distinct non-Unicode folders distinct.
        let mut key = String::from("\0native:");
        for byte in path.as_os_str().as_encoded_bytes() {
            use std::fmt::Write;
            write!(&mut key, "{byte:02x}").expect("writing to a String");
        }
        key
    }
}

impl AppConfig {
    pub fn has_folder_sort(&self, path: &Path) -> bool {
        self.folder_sort_configs.contains_key(&folder_key(path))
    }
    pub fn sort_for_folder(&self, path: &Path) -> SortConfig {
        self.folder_sort_configs
            .get(&folder_key(path))
            .copied()
            .unwrap_or(self.default_sort)
    }

    pub fn set_folder_sort(&mut self, path: &Path, sort: Option<SortConfig>) {
        let key = folder_key(path);
        if let Some(sort) = sort {
            self.folder_sort_configs.insert(key, sort);
        } else {
            self.folder_sort_configs.remove(&key);
        }
    }
}

impl AppState {
    /// Restore only available directories. Explicit launcher targets are applied
    /// by the UI after construction and always take precedence over this choice.
    pub fn from_config(config: AppConfig) -> (Self, Option<String>) {
        let mut warning = None;
        let mut tabs = Vec::new();
        let mut active_tab_idx = 0;
        if config.restore_tabs {
            for (index, saved) in config.saved_tabs.iter().enumerate() {
                if saved.current_dir.is_absolute() && saved.current_dir.is_dir() {
                    let mut tab = TabState::new(tabs.len() as u64 + 1, saved.current_dir.clone());
                    tab.view_mode = saved.view_mode;
                    tab.label = saved.label.clone();
                    if index == config.saved_active_tab {
                        active_tab_idx = tabs.len();
                    }
                    tabs.push(tab);
                }
            }
            if tabs.len() != config.saved_tabs.len() {
                warning = Some("Unavailable folders were skipped while restoring tabs.".to_owned());
            }
        }
        if tabs.is_empty() {
            let dir = config
                .startup_folder
                .as_ref()
                .filter(|path| path.is_absolute() && path.is_dir())
                .cloned()
                .unwrap_or_else(|| {
                    if config.startup_folder.is_some() {
                        warning = Some(
                            "The startup folder is unavailable. Opened Home instead.".to_owned(),
                        );
                    }
                    default_home_dir()
                });
            let mut tab = TabState::new(1, dir);
            tab.view_mode = config.default_view_mode;
            tabs.push(tab);
        }
        let next_tab_id = tabs.len() as u64 + 1;
        (
            Self {
                config,
                tabs,
                active_tab_idx,
                link_view: None,
                next_tab_id,
            },
            warning,
        )
    }

    pub fn remember_session(&mut self) {
        if self.config.restore_tabs {
            self.config.saved_tabs = self
                .tabs
                .iter()
                .map(|tab| {
                    let mut saved = TabState::new(tab.id, tab.current_dir.clone());
                    saved.view_mode = tab.view_mode;
                    saved.label = tab.label.clone();
                    saved
                })
                .collect();
            self.config.saved_active_tab = self.active_tab_idx;
        } else {
            self.config.saved_tabs.clear();
            self.config.saved_active_tab = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_sort_directions_roundtrip_without_changing_the_column() {
        for sort in [
            SearchSort::Name,
            SearchSort::Path,
            SearchSort::Modified,
            SearchSort::Size,
        ] {
            assert_eq!(sort.reversed().reversed(), sort);
            assert_eq!(sort.reversed().column(), sort);
            assert_ne!(sort.ascending(), sort.reversed().ascending());
        }
        assert_eq!(SearchSort::Relevance.reversed(), SearchSort::Relevance);
    }

    #[cfg(unix)]
    #[test]
    fn native_folder_preferences_roundtrip_without_path_collisions() {
        use std::os::unix::ffi::OsStringExt;
        let first = PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/ottrin-\xff".to_vec()));
        let second = PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/ottrin-\xfe".to_vec()));
        let mut config = AppConfig {
            startup_folder: Some(first.clone()),
            ..Default::default()
        };
        config.saved_tabs.push(TabState::new(1, first.clone()));
        config.set_folder_sort(
            &first,
            Some(SortConfig {
                by: SortBy::Modified,
                ..Default::default()
            }),
        );
        config.set_folder_sort(
            &second,
            Some(SortConfig {
                by: SortBy::Size,
                ..Default::default()
            }),
        );
        let loaded: AppConfig =
            serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(loaded.startup_folder, Some(first.clone()));
        assert_eq!(loaded.saved_tabs[0].current_dir, first);
        assert_eq!(
            loaded.sort_for_folder(&loaded.saved_tabs[0].current_dir).by,
            SortBy::Modified
        );
        assert_eq!(loaded.sort_for_folder(&second).by, SortBy::Size);
    }

    #[test]
    fn unavailable_restored_folders_fall_back_and_session_opt_out_clears_saved_tabs() {
        let home = default_home_dir();
        let mut config = AppConfig {
            restore_tabs: true,
            startup_folder: Some(home.clone()),
            ..Default::default()
        };
        config.saved_tabs = vec![TabState::new(99, home.join("ottrin-no-such-saved-folder"))];
        let (mut state, warning) = AppState::from_config(config);
        assert_eq!(state.tabs.len(), 1);
        assert_eq!(state.active_tab().current_dir, home);
        assert!(warning.is_some());
        state.config.restore_tabs = false;
        state.remember_session();
        assert!(state.config.saved_tabs.is_empty());
    }

    #[test]
    fn folder_sort_overrides_roundtrip_and_reset_independently() {
        let mut config = AppConfig::default();
        let home = default_home_dir();
        let downloads = home.join("Downloads");
        let newest = SortConfig {
            by: SortBy::Modified,
            ascending: false,
            folders_first: true,
        };
        config.set_folder_sort(&downloads, Some(newest));
        let mut loaded: AppConfig =
            serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(loaded.sort_for_folder(&downloads), newest);
        assert_eq!(loaded.sort_for_folder(&home), SortConfig::default());
        loaded.default_sort.by = SortBy::Size;
        assert_eq!(loaded.sort_for_folder(&downloads), newest);
        assert_eq!(loaded.sort_for_folder(&home).by, SortBy::Size);
        loaded.set_folder_sort(&downloads, None);
        assert_eq!(loaded.sort_for_folder(&downloads), loaded.default_sort);
    }

    #[test]
    fn startup_session_precedence_and_unavailable_fallback() {
        let home = default_home_dir();
        let config = AppConfig {
            startup_folder: Some(home.clone()),
            ..Default::default()
        };
        let (mut state, warning) = AppState::from_config(config);
        assert_eq!(state.active_tab().current_dir, home);
        assert!(warning.is_none());
        state.config.restore_tabs = true;
        state.new_tab(std::env::current_dir().unwrap());
        state.active_tab_idx = 1;
        let expected = state.active_tab().current_dir.clone();
        state.remember_session();
        state.config.startup_folder = Some(home.join("ottrin-nonexistent-startup-fixture"));
        let (restored, warning) = AppState::from_config(state.config.clone());
        assert_eq!(restored.active_tab().current_dir, expected);
        assert!(warning.is_none());
        state.config.restore_tabs = false;
        let (fallback, warning) = AppState::from_config(state.config);
        assert_eq!(fallback.active_tab().current_dir, home);
        assert!(warning.unwrap().contains("unavailable"));
        assert!(fallback.next_tab_id > fallback.active_tab().id);
    }
}
