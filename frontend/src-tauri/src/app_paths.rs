use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, Runtime, State};

pub const DATA_ROOT_NAME: &str = "Meeting Assistant";
pub const LEGACY_IDENTIFIER: &str = "com.meetily.ai";
pub const CURRENT_IDENTIFIER: &str = "com.ghassanaq.meetingassistant";

pub const ANALYTICS_STORE: &str = "analytics.json";
pub const PREFERENCES_STORE: &str = "preferences.json";
pub const API_STORE: &str = "store.json";
pub const ONBOARDING_STORE: &str = "onboarding-status.json";
pub const NOTIFICATIONS_STORE: &str = "notifications.json";
pub const RECORDING_PREFERENCES_STORE: &str = "recording_preferences.json";

pub const STORE_FILES: [&str; 6] = [
    ANALYTICS_STORE,
    PREFERENCES_STORE,
    API_STORE,
    ONBOARDING_STORE,
    NOTIFICATIONS_STORE,
    RECORDING_PREFERENCES_STORE,
];

/// Stable application paths that do not depend on Tauri's bundle identifier.
///
/// The root deliberately uses the OS local-data base. On Windows this keeps
/// downloaded models out of the roaming profile, and changing the bundle
/// identifier later will not relocate user data again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPaths {
    root: PathBuf,
    stores: PathBuf,
    models: PathBuf,
    legacy_data_root: PathBuf,
    legacy_notification_path: PathBuf,
    identifier_roaming_root: PathBuf,
    identifier_local_root: PathBuf,
}

impl AppPaths {
    pub fn resolve<R: Runtime>(app: &AppHandle<R>) -> Result<Self> {
        let local_data_base = app
            .path()
            .local_data_dir()
            .context("failed to resolve the OS local data directory")?;
        let roaming_data_base = app
            .path()
            .data_dir()
            .context("failed to resolve the OS roaming data directory")?;
        let config_base = app
            .path()
            .config_dir()
            .context("failed to resolve the OS config directory")?;

        Ok(Self::from_bases(
            local_data_base,
            roaming_data_base,
            config_base,
            CURRENT_IDENTIFIER,
        ))
    }

    pub(crate) fn from_bases(
        local_data_base: PathBuf,
        roaming_data_base: PathBuf,
        config_base: PathBuf,
        current_identifier: &str,
    ) -> Self {
        let root = local_data_base.join(DATA_ROOT_NAME);
        Self {
            stores: root.join("stores"),
            models: root.join("models"),
            legacy_data_root: roaming_data_base.join(LEGACY_IDENTIFIER),
            legacy_notification_path: config_base.join("meetily").join(NOTIFICATIONS_STORE),
            identifier_roaming_root: roaming_data_base.join(current_identifier),
            identifier_local_root: local_data_base.join(current_identifier),
            root,
        }
    }

    pub fn ensure_directories(&self) -> Result<()> {
        std::fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create data root {}", self.root.display()))?;
        std::fs::create_dir_all(&self.stores).with_context(|| {
            format!(
                "failed to create stores directory {}",
                self.stores.display()
            )
        })?;
        std::fs::create_dir_all(&self.models).with_context(|| {
            format!(
                "failed to create models directory {}",
                self.models.display()
            )
        })?;
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn database_path(&self) -> PathBuf {
        self.root.join("meeting_minutes.sqlite")
    }

    pub fn legacy_backend_database_path(&self) -> PathBuf {
        self.root.join("meeting_minutes.db")
    }

    pub fn models_dir(&self) -> &Path {
        &self.models
    }

    pub fn summary_models_dir(&self) -> PathBuf {
        self.models.join("summary")
    }

    pub fn store_path(&self, name: &str) -> Result<PathBuf> {
        if !STORE_FILES.contains(&name) {
            return Err(anyhow!("unsupported application store: {name}"));
        }
        Ok(self.stores.join(name))
    }

    pub fn migration_journal_path(&self) -> PathBuf {
        self.root.join("storage-migration-v1.json")
    }

    pub fn legacy_data_root(&self) -> &Path {
        &self.legacy_data_root
    }

    pub fn legacy_notification_path(&self) -> &Path {
        &self.legacy_notification_path
    }

    #[cfg(test)]
    pub(crate) fn identifier_roaming_root(&self) -> &Path {
        &self.identifier_roaming_root
    }

    #[cfg(test)]
    pub(crate) fn identifier_local_root(&self) -> &Path {
        &self.identifier_local_root
    }

    pub fn temporary_recording_path(&self, filename: &str) -> Result<PathBuf> {
        if filename.contains('/') || filename.contains('\\') {
            return Err(anyhow!(
                "recording filename must not contain path separators"
            ));
        }
        Ok(self.root.join(filename))
    }
}

#[tauri::command]
pub fn get_app_store_path(paths: State<'_, AppPaths>, name: String) -> Result<String, String> {
    paths
        .store_path(&name)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_stable_and_identifier_independent() {
        let paths = AppPaths::from_bases(
            PathBuf::from("local"),
            PathBuf::from("roaming"),
            PathBuf::from("config"),
            CURRENT_IDENTIFIER,
        );

        assert_eq!(paths.root(), Path::new("local").join(DATA_ROOT_NAME));
        assert_eq!(
            paths.legacy_data_root(),
            Path::new("roaming").join(LEGACY_IDENTIFIER)
        );
        assert_ne!(paths.root(), paths.identifier_local_root());
        assert_ne!(paths.root(), paths.identifier_roaming_root());
        assert_eq!(
            paths.store_path(ANALYTICS_STORE).unwrap(),
            Path::new("local")
                .join(DATA_ROOT_NAME)
                .join("stores")
                .join(ANALYTICS_STORE)
        );
    }

    #[test]
    fn store_paths_are_allowlisted() {
        let paths = AppPaths::from_bases(
            PathBuf::from("local"),
            PathBuf::from("roaming"),
            PathBuf::from("config"),
            CURRENT_IDENTIFIER,
        );

        for store in STORE_FILES {
            assert!(paths.store_path(store).is_ok());
        }
        assert!(paths.store_path("../outside.json").is_err());
        assert!(paths.store_path("unknown.json").is_err());
    }

    #[test]
    fn identifier_constant_matches_tauri_configuration() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], CURRENT_IDENTIFIER);
    }
}
