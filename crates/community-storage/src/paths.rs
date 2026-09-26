use std::path::{Path, PathBuf};

/// Platform data directory for this peer's storage (never a network share by default).
pub fn default_storage_root() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("community-ai")
        .join("storage")
}

pub struct StorageLayout {
    pub root: PathBuf,
    pub database_dir: PathBuf,
    pub database_path: PathBuf,
    pub objects_dir: PathBuf,
    pub temp_dir: PathBuf,
    pub data_key_path: PathBuf,
}

impl StorageLayout {
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();
        let database_dir = root.join("database");
        Self {
            database_path: database_dir.join("community.db"),
            database_dir,
            objects_dir: root.join("objects"),
            temp_dir: root.join("temp"),
            data_key_path: root.join("data.key"),
            root,
        }
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.database_dir)?;
        std::fs::create_dir_all(&self.objects_dir)?;
        std::fs::create_dir_all(&self.temp_dir)?;
        Ok(())
    }

    pub fn object_path(&self, content_hash: &str) -> PathBuf {
        let prefix = content_hash.get(..2).unwrap_or("xx");
        self.objects_dir.join(prefix).join(content_hash)
    }
}
