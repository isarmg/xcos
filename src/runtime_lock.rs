//! Product state ownership composed from the common instance/maintenance protocol.
use anyhow::{ensure, Context};
use std::os::unix::fs::MetadataExt;
use std::{fs, path::PathBuf};
use xcss::state_file::{InstanceLock, PrivateStateDirectory};

pub struct ApplicationLock {
    _instance: InstanceLock,
    database: PathBuf,
    database_identity: Option<(u64, u64)>,
}
impl ApplicationLock {
    pub fn acquire(database_url: &str) -> anyhow::Result<Self> {
        let database = crate::sqlite::database_path(database_url)?;
        let root = database.parent().context("database parent is required")?;
        let directory = PrivateStateDirectory::open(root)?;
        let instance = directory.try_instance_lock()?;
        let database_identity = match fs::symlink_metadata(&database) {
            Ok(_) => {
                let descriptor = directory.open_existing(
                    database
                        .file_name()
                        .context("database filename is required")?,
                )?;
                let identity = descriptor.identity();
                Some((identity.device(), identity.inode()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            _instance: instance,
            database,
            database_identity,
        })
    }
    pub fn validate_open_database(&self) -> anyhow::Result<()> {
        let metadata = fs::symlink_metadata(&self.database)?;
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink() && metadata.nlink() == 1,
            "database must remain a regular single-link file"
        );
        if let Some(identity) = self.database_identity {
            ensure!(
                identity == (metadata.dev(), metadata.ino()),
                "database identity changed during startup"
            );
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    fn state() -> (tempfile::TempDir, PathBuf, String) {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let database = root.path().join("app.sqlite3");
        crate::sqlite::initialize_current_database(&database).unwrap();
        let url = format!("sqlite://{}", database.display());
        (root, database, url)
    }
    #[test]
    fn state_instance_lock_excludes_duplicates_without_runtime_files() {
        let (root, _database, url) = state();
        let first = ApplicationLock::acquire(&url).unwrap();
        first.validate_open_database().unwrap();
        assert!(ApplicationLock::acquire(&url).is_err());
        assert!(PrivateStateDirectory::open(root.path())
            .unwrap()
            .try_maintenance_lock()
            .is_err());
        PrivateStateDirectory::open(root.path())
            .unwrap()
            .try_shared_maintenance_lock()
            .unwrap();
        assert!(!root.path().join("runtime").exists());
        assert!(!root.path().join("app.pid").exists());
        drop(first);
        let maintenance = PrivateStateDirectory::open(root.path())
            .unwrap()
            .try_maintenance_lock()
            .unwrap();
        assert!(ApplicationLock::acquire(&url).is_err());
        drop(maintenance);
        ApplicationLock::acquire(&url).unwrap();
    }
    #[test]
    fn aliased_database_and_pending_maintenance_are_rejected() {
        let (root, database, url) = state();
        let alias = root.path().join("alias");
        fs::hard_link(&database, &alias).unwrap();
        assert!(ApplicationLock::acquire(&url).is_err());
        fs::remove_file(alias).unwrap();
        let link = root.path().join("link.sqlite3");
        symlink(&database, &link).unwrap();
        assert!(ApplicationLock::acquire(&format!("sqlite://{}", link.display())).is_err());
        fs::write(
            root.path().join(xcss::state_file::MAINTENANCE_PENDING_FILE),
            b"{}",
        )
        .unwrap();
        assert!(ApplicationLock::acquire(&url).is_err());
    }
}
