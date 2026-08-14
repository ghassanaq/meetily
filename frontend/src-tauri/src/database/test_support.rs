use super::manager::DatabaseManager;
use sqlx::SqlitePool;
use tempfile::{tempdir, TempDir};

pub(crate) struct TestDatabase {
    manager: DatabaseManager,
    _temp_dir: TempDir,
}

impl TestDatabase {
    pub(crate) async fn new() -> Self {
        let temp_dir = tempdir().expect("failed to create temporary database directory");
        let database_path = temp_dir.path().join("meeting_minutes.sqlite");
        let missing_backend_path = temp_dir.path().join("missing-backend.db");

        let manager = DatabaseManager::new(
            database_path
                .to_str()
                .expect("temporary database path was not valid UTF-8"),
            missing_backend_path
                .to_str()
                .expect("temporary backend path was not valid UTF-8"),
        )
        .await
        .expect("failed to create migrated test database");

        Self {
            manager,
            _temp_dir: temp_dir,
        }
    }

    pub(crate) fn pool(&self) -> &SqlitePool {
        self.manager.pool()
    }
}
