//! Where settings JSON is persisted. The service owns *when* to save; the
//! store owns *how*.

use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("settings store i/o error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub trait SettingsStore: Send + Sync + 'static {
    /// The stored document, or `None` if nothing was saved yet.
    fn load(&self) -> Result<Option<String>, StoreError>;
    /// Persist atomically: a reader must never observe a partial document.
    fn save(&self, json: &str) -> Result<(), StoreError>;
    /// Where the document lives, for diagnostics.
    fn location(&self) -> String;
}

/// JSON file on disk, written atomically (temp file + rename).
#[derive(Debug, Clone)]
pub struct FileSettingsStore {
    path: PathBuf,
}

impl FileSettingsStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn io(&self, source: std::io::Error) -> StoreError {
        StoreError::Io {
            path: self.path.clone(),
            source,
        }
    }
}

impl SettingsStore for FileSettingsStore {
    fn load(&self) -> Result<Option<String>, StoreError> {
        match std::fs::read_to_string(&self.path) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(self.io(e)),
        }
    }

    fn save(&self, json: &str) -> Result<(), StoreError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| self.io(e))?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| self.io(e))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| self.io(e))
    }

    fn location(&self) -> String {
        self.path.display().to_string()
    }
}

/// In-memory store for tests and for `STEM_PLATFORM=mock` runs.
#[derive(Debug, Default)]
pub struct MemorySettingsStore {
    doc: Mutex<Option<String>>,
}

impl MemorySettingsStore {
    pub fn with_document(json: &str) -> Self {
        Self {
            doc: Mutex::new(Some(json.to_owned())),
        }
    }

    pub fn document(&self) -> Option<String> {
        self.doc.lock().clone()
    }
}

impl SettingsStore for MemorySettingsStore {
    fn load(&self) -> Result<Option<String>, StoreError> {
        Ok(self.doc.lock().clone())
    }

    fn save(&self, json: &str) -> Result<(), StoreError> {
        *self.doc.lock() = Some(json.to_owned());
        Ok(())
    }

    fn location(&self) -> String {
        "<memory>".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_store_round_trips_and_creates_parent_dirs() {
        let dir = std::env::temp_dir().join(format!("dock-store-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = FileSettingsStore::new(dir.join("nested").join("settings.json"));
        assert_eq!(store.load().unwrap(), None);
        store.save("{\"a\":1}").unwrap();
        assert_eq!(store.load().unwrap().as_deref(), Some("{\"a\":1}"));
        assert!(!store.path().with_extension("json.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
