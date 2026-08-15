use crate::app_paths::{AppPaths, NOTIFICATIONS_STORE, STORE_FILES};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqliteConnectOptions, Connection, SqliteConnection};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const DATABASE_ITEM: &str = "database";
const MODELS_ITEM: &str = "models";

#[derive(Debug, Default, Deserialize, Serialize)]
struct MigrationJournal {
    version: u8,
    legacy_data_detected: bool,
    completed_items: BTreeSet<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct MigrationReport {
    pub database_migrated: bool,
    pub stores_migrated: usize,
    pub model_files_migrated: usize,
    pub incomplete_items: Vec<String>,
}

pub async fn migrate_legacy_storage(paths: &AppPaths) -> Result<MigrationReport> {
    paths.ensure_directories()?;
    let mut journal = load_journal(paths)?;
    journal.version = 1;
    journal.legacy_data_detected |= legacy_data_exists(paths);
    let mut report = MigrationReport::default();

    if !journal.completed_items.contains(DATABASE_ITEM) {
        report.database_migrated = migrate_database(paths).await?;
        journal.completed_items.insert(DATABASE_ITEM.to_string());
        save_journal(paths, &journal)?;
    }

    for name in STORE_FILES {
        let item = format!("store:{name}");
        if journal.completed_items.contains(&item) {
            continue;
        }

        match migrate_store(paths, name) {
            Ok(true) => report.stores_migrated += 1,
            Ok(false) => {}
            Err(error) => {
                log::error!("Could not migrate application store {name}: {error:#}");
                report.incomplete_items.push(item);
                continue;
            }
        }
        journal.completed_items.insert(item);
        save_journal(paths, &journal)?;
    }

    if !journal.completed_items.contains(MODELS_ITEM) {
        match migrate_models(paths) {
            Ok(count) => {
                report.model_files_migrated = count;
                journal.completed_items.insert(MODELS_ITEM.to_string());
                save_journal(paths, &journal)?;
            }
            Err(error) => {
                log::error!("Could not migrate legacy models: {error:#}");
                report.incomplete_items.push(MODELS_ITEM.to_string());
            }
        }
    }

    Ok(report)
}

fn legacy_data_exists(paths: &AppPaths) -> bool {
    paths.legacy_data_root().exists() || paths.legacy_notification_path().exists()
}

fn load_journal(paths: &AppPaths) -> Result<MigrationJournal> {
    let path = paths.migration_journal_path();
    if !path.exists() {
        return Ok(MigrationJournal::default());
    }

    let contents = fs::read_to_string(&path)
        .with_context(|| format!("failed to read migration journal {}", path.display()))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse migration journal {}", path.display()))
}

fn save_journal(paths: &AppPaths, journal: &MigrationJournal) -> Result<()> {
    let path = paths.migration_journal_path();
    let temporary = path.with_extension("json.new");
    let contents = serde_json::to_vec_pretty(journal)?;
    fs::write(&temporary, contents)
        .with_context(|| format!("failed to write migration journal {}", temporary.display()))?;

    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("failed to replace migration journal {}", path.display()))?;
    }
    fs::rename(&temporary, &path)
        .with_context(|| format!("failed to publish migration journal {}", path.display()))?;
    Ok(())
}

async fn migrate_database(paths: &AppPaths) -> Result<bool> {
    let destination = paths.database_path();
    if destination.exists() {
        return Ok(false);
    }

    let sqlite_source = paths.legacy_data_root().join("meeting_minutes.sqlite");
    let backend_source = paths.legacy_data_root().join("meeting_minutes.db");
    let source = if sqlite_source.is_file() {
        sqlite_source
    } else if backend_source.is_file() {
        backend_source
    } else {
        return Ok(false);
    };

    let temporary = destination.with_extension("sqlite.migrating");
    if temporary.exists() {
        fs::remove_file(&temporary).with_context(|| {
            format!(
                "failed to remove stale database snapshot {}",
                temporary.display()
            )
        })?;
    }

    let source_options = SqliteConnectOptions::new()
        .filename(&source)
        .create_if_missing(false);
    let mut source_connection = SqliteConnection::connect_with(&source_options)
        .await
        .with_context(|| format!("failed to open legacy database {}", source.display()))?;
    let source_counts = table_row_counts(&mut source_connection).await?;

    let escaped_destination = temporary.to_string_lossy().replace('\'', "''");
    sqlx::query(&format!("VACUUM INTO '{escaped_destination}'"))
        .execute(&mut source_connection)
        .await
        .with_context(|| {
            format!(
                "failed to create a WAL-safe database snapshot from {}",
                source.display()
            )
        })?;

    let destination_options = SqliteConnectOptions::new()
        .filename(&temporary)
        .create_if_missing(false);
    let mut destination_connection = SqliteConnection::connect_with(&destination_options)
        .await
        .with_context(|| format!("failed to open database snapshot {}", temporary.display()))?;

    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut destination_connection)
        .await
        .context("failed to run integrity_check on the database snapshot")?;
    if integrity.to_lowercase() != "ok" {
        return Err(anyhow!(
            "database snapshot failed integrity_check: {integrity}"
        ));
    }

    let destination_counts = table_row_counts(&mut destination_connection).await?;
    if source_counts != destination_counts {
        return Err(anyhow!(
            "database snapshot row counts differ from the legacy database"
        ));
    }

    source_connection.close().await?;
    destination_connection.close().await?;
    fs::rename(&temporary, &destination).with_context(|| {
        format!(
            "failed to publish database snapshot {}",
            destination.display()
        )
    })?;

    log::info!(
        "Migrated legacy database into stable local storage; source retained at {}",
        source.display()
    );
    Ok(true)
}

async fn table_row_counts(connection: &mut SqliteConnection) -> Result<BTreeMap<String, i64>> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master \
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(&mut *connection)
    .await?;

    let mut counts = BTreeMap::new();
    for table in tables {
        let quoted = table.replace('"', "\"\"");
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM \"{quoted}\""))
            .fetch_one(&mut *connection)
            .await?;
        counts.insert(table, count);
    }
    Ok(counts)
}

fn migrate_store(paths: &AppPaths, name: &str) -> Result<bool> {
    let destination = paths.store_path(name)?;
    if destination.exists() {
        return Ok(false);
    }

    let source = if name == NOTIFICATIONS_STORE && paths.legacy_notification_path().is_file() {
        paths.legacy_notification_path().to_path_buf()
    } else {
        paths.legacy_data_root().join(name)
    };

    if !source.is_file() {
        return Ok(false);
    }

    copy_file_atomically(&source, &destination)?;
    log::info!(
        "Migrated application store {}; legacy source retained",
        name
    );
    Ok(true)
}

fn migrate_models(paths: &AppPaths) -> Result<usize> {
    let source = paths.legacy_data_root().join("models");
    if !source.is_dir() {
        return Ok(0);
    }

    let migrated = link_or_copy_tree(&source, paths.models_dir())?;
    log::info!(
        "Migrated {} model files into stable local storage; legacy models retained",
        migrated
    );
    Ok(migrated)
}

fn link_or_copy_tree(source: &Path, destination: &Path) -> Result<usize> {
    link_or_copy_tree_with(source, destination, &|source: &Path, destination: &Path| {
        fs::hard_link(source, destination)
    })
}

fn link_or_copy_tree_with<F>(source: &Path, destination: &Path, link_file: &F) -> Result<usize>
where
    F: Fn(&Path, &Path) -> std::io::Result<()>,
{
    fs::create_dir_all(destination)?;
    let mut migrated = 0;

    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;

        if file_type.is_symlink() {
            log::warn!(
                "Skipping symlink in legacy model directory: {}",
                source_path.display()
            );
            continue;
        }
        if file_type.is_dir() {
            migrated += link_or_copy_tree_with(&source_path, &destination_path, link_file)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }

        if destination_path.exists() {
            let source_size = fs::metadata(&source_path)?.len();
            let destination_size = fs::metadata(&destination_path)?.len();
            if source_size != destination_size {
                return Err(anyhow!(
                    "model migration conflict at {}",
                    destination_path.display()
                ));
            }
            continue;
        }

        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if link_file(&source_path, &destination_path).is_err() {
            copy_file_atomically(&source_path, &destination_path)?;
        }
        migrated += 1;
    }

    Ok(migrated)
}

fn copy_file_atomically(source: &Path, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    let temporary = temporary_copy_path(destination);
    if temporary.exists() {
        fs::remove_file(&temporary)?;
    }
    fs::copy(source, &temporary).with_context(|| {
        format!(
            "failed to copy {} to {}",
            source.display(),
            temporary.display()
        )
    })?;
    fs::rename(&temporary, destination)
        .with_context(|| format!("failed to publish migrated file {}", destination.display()))?;
    Ok(())
}

fn temporary_copy_path(destination: &Path) -> PathBuf {
    let filename = destination
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    destination.with_file_name(format!("{filename}.migrating"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_paths::{CURRENT_IDENTIFIER, DATA_ROOT_NAME};
    use sqlx::sqlite::SqlitePoolOptions;
    use tempfile::TempDir;

    fn test_paths(temp: &TempDir) -> AppPaths {
        AppPaths::from_bases(
            temp.path().join("local"),
            temp.path().join("roaming"),
            temp.path().join("config"),
            CURRENT_IDENTIFIER,
        )
    }

    #[tokio::test]
    async fn database_snapshot_includes_wal_rows_and_preserves_the_source() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(&temp);
        fs::create_dir_all(paths.legacy_data_root()).unwrap();
        let source = paths.legacy_data_root().join("meeting_minutes.sqlite");
        let source_options = SqliteConnectOptions::new()
            .filename(&source)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(source_options)
            .await
            .unwrap();
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("PRAGMA wal_autocheckpoint=0")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE meetings (id TEXT PRIMARY KEY, title TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO meetings (id, title) VALUES ('1', 'Retained meeting')")
            .execute(&pool)
            .await
            .unwrap();

        let first = migrate_legacy_storage(&paths).await.unwrap();
        assert!(first.database_migrated);
        assert!(source.exists());
        assert!(paths.database_path().exists());

        let destination_options = SqliteConnectOptions::new()
            .filename(paths.database_path())
            .create_if_missing(false);
        let destination_pool = SqlitePoolOptions::new()
            .connect_with(destination_options)
            .await
            .unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings")
            .fetch_one(&destination_pool)
            .await
            .unwrap();
        assert_eq!(count, 1);

        let source_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meetings")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(source_count, 1);
    }

    #[tokio::test]
    async fn completed_migration_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(&temp);
        fs::create_dir_all(paths.legacy_data_root()).unwrap();
        fs::write(paths.legacy_data_root().join("analytics.json"), b"{}").unwrap();

        let first = migrate_legacy_storage(&paths).await.unwrap();
        assert_eq!(first.stores_migrated, 1);

        let second = migrate_legacy_storage(&paths).await.unwrap();
        assert_eq!(second, MigrationReport::default());
    }

    #[tokio::test]
    async fn legacy_detection_is_journaled_without_using_identifier_directories() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(&temp);
        fs::create_dir_all(paths.legacy_data_root()).unwrap();

        migrate_legacy_storage(&paths).await.unwrap();

        let journal = load_journal(&paths).unwrap();
        assert!(journal.legacy_data_detected);
        assert!(!paths.identifier_local_root().exists());
        assert!(!paths.identifier_roaming_root().exists());
        assert_eq!(paths.root(), temp.path().join("local").join(DATA_ROOT_NAME));
    }

    #[tokio::test]
    async fn all_stores_migrate_without_removing_legacy_files() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(&temp);
        fs::create_dir_all(paths.legacy_data_root()).unwrap();
        fs::create_dir_all(paths.legacy_notification_path().parent().unwrap()).unwrap();

        for name in STORE_FILES {
            let source = if name == NOTIFICATIONS_STORE {
                paths.legacy_notification_path().to_path_buf()
            } else {
                paths.legacy_data_root().join(name)
            };
            fs::write(&source, format!("{{\"source\":\"{name}\"}}")).unwrap();
        }

        let report = migrate_legacy_storage(&paths).await.unwrap();
        assert_eq!(report.stores_migrated, STORE_FILES.len());
        for name in STORE_FILES {
            let destination = paths.store_path(name).unwrap();
            assert!(destination.is_file());
            let source = if name == NOTIFICATIONS_STORE {
                paths.legacy_notification_path().to_path_buf()
            } else {
                paths.legacy_data_root().join(name)
            };
            assert!(source.is_file());
            assert_eq!(fs::read(&destination).unwrap(), fs::read(&source).unwrap());
        }
    }

    #[tokio::test]
    async fn model_files_are_hard_linked_and_legacy_models_remain() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(&temp);
        let legacy_models = paths.legacy_data_root().join("models").join("summary");
        fs::create_dir_all(&legacy_models).unwrap();
        let source = legacy_models.join("model.gguf");
        fs::write(&source, b"immutable model bytes").unwrap();

        let report = migrate_legacy_storage(&paths).await.unwrap();
        assert_eq!(report.model_files_migrated, 1);
        let destination = paths.summary_models_dir().join("model.gguf");
        assert_eq!(fs::read(&destination).unwrap(), b"immutable model bytes");
        assert_eq!(fs::read(&source).unwrap(), b"immutable model bytes");

        fs::write(&source, b"updated through legacy link").unwrap();
        assert_eq!(
            fs::read(&destination).unwrap(),
            b"updated through legacy link"
        );
    }

    #[test]
    fn model_copy_fallback_preserves_an_independent_legacy_source() {
        let temp = tempfile::tempdir().unwrap();
        let source_dir = temp.path().join("legacy-models");
        let destination_dir = temp.path().join("stable-models");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("model.bin");
        let destination = destination_dir.join("model.bin");
        fs::write(&source, b"copied model").unwrap();

        let force_link_failure = |_: &Path, _: &Path| {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "forced cross-volume fallback",
            ))
        };
        let migrated =
            link_or_copy_tree_with(&source_dir, &destination_dir, &force_link_failure).unwrap();

        assert_eq!(migrated, 1);
        assert_eq!(fs::read(&destination).unwrap(), b"copied model");
        fs::write(&source, b"legacy source changed").unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"copied model");
    }

    #[tokio::test]
    async fn model_conflicts_are_reported_and_retried_without_touching_the_source() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(&temp);
        let source = paths.legacy_data_root().join("models").join("model.bin");
        let destination = paths.models_dir().join("model.bin");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, b"legacy model").unwrap();
        fs::create_dir_all(paths.models_dir()).unwrap();
        fs::write(&destination, b"different").unwrap();

        let first = migrate_legacy_storage(&paths).await.unwrap();
        assert_eq!(first.incomplete_items, vec![MODELS_ITEM]);
        assert_eq!(fs::read(&source).unwrap(), b"legacy model");
        assert_eq!(fs::read(&destination).unwrap(), b"different");

        fs::remove_file(&destination).unwrap();
        let retry = migrate_legacy_storage(&paths).await.unwrap();
        assert_eq!(retry.model_files_migrated, 1);
        assert!(retry.incomplete_items.is_empty());
        assert_eq!(fs::read(&source).unwrap(), b"legacy model");
        assert_eq!(fs::read(&destination).unwrap(), b"legacy model");

        let completed = migrate_legacy_storage(&paths).await.unwrap();
        assert_eq!(completed, MigrationReport::default());
    }
}
