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
    /// The file was read but is not text this store can decode
    /// ([`decode_document`]): the document is unusable, like invalid JSON.
    #[error("settings file {path} is neither UTF-8 nor UTF-16 text")]
    Encoding { path: PathBuf },
}

/// The text of a settings file from its bytes: UTF-8, with or without a
/// byte order mark (Windows PowerShell 5.1's `Set-Content -Encoding UTF8`
/// and Notepad's "UTF-8 with BOM" write one), or UTF-16 little- or
/// big-endian with its byte order mark (Windows PowerShell 5.1's `>` and
/// `Out-File`, Notepad's "UTF-16"). `None` for anything else. Pure.
pub fn decode_document(bytes: &[u8]) -> Option<String> {
    let utf16 = |rest: &[u8], unit: fn([u8; 2]) -> u16| -> Option<String> {
        if rest.len() % 2 != 0 {
            return None;
        }
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|pair| unit([pair[0], pair[1]]))
            .collect();
        String::from_utf16(&units).ok()
    };
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(rest.to_vec()).ok();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, u16::from_be_bytes);
    }
    String::from_utf8(bytes.to_vec()).ok()
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
    /// The file's text ([`decode_document`]); a file that is not text is a
    /// [`StoreError::Encoding`], which the app treats like invalid JSON,
    /// while a file that cannot be read at all stays a [`StoreError::Io`].
    fn load(&self) -> Result<Option<String>, StoreError> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(self.io(e)),
        };
        decode_document(&bytes)
            .map(Some)
            .ok_or_else(|| StoreError::Encoding {
                path: self.path.clone(),
            })
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

    /// `text` as UTF-16 with its byte order mark, little- or big-endian.
    fn utf16(text: &str, little_endian: bool) -> Vec<u8> {
        let mut bytes = if little_endian {
            vec![0xFF, 0xFE]
        } else {
            vec![0xFE, 0xFF]
        };
        for unit in text.encode_utf16() {
            let pair = if little_endian {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            };
            bytes.extend_from_slice(&pair);
        }
        bytes
    }

    #[test]
    fn documents_decode_from_utf8_with_or_without_a_bom_and_from_utf16() {
        let json = r#"{"iconSize": 64, "pinned": [{"label": "Café"}]}"#;
        assert_eq!(decode_document(json.as_bytes()).as_deref(), Some(json));
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice(json.as_bytes());
        assert_eq!(decode_document(&bom).as_deref(), Some(json));
        assert_eq!(decode_document(&utf16(json, true)).as_deref(), Some(json));
        assert_eq!(decode_document(&utf16(json, false)).as_deref(), Some(json));
        assert_eq!(decode_document(b"").as_deref(), Some(""));
        // Not text: a Latin-1 byte, an odd UTF-16 length, a lone surrogate.
        assert_eq!(decode_document(b"{\"label\": \"Caf\xE9\"}"), None);
        let mut odd = utf16(json, true);
        odd.push(0);
        assert_eq!(decode_document(&odd), None);
        assert_eq!(decode_document(&[0xFF, 0xFE, 0x00, 0xD8]), None);
    }

    #[test]
    fn a_file_that_is_not_text_is_an_encoding_error() {
        let dir = std::env::temp_dir().join(format!("dock-store-enc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = FileSettingsStore::new(dir.join("settings.json"));
        std::fs::write(store.path(), utf16("{\"iconSize\": 64}", true)).unwrap();
        assert_eq!(store.load().unwrap().as_deref(), Some("{\"iconSize\": 64}"));
        std::fs::write(store.path(), b"\xFF\xFF\xFF").unwrap();
        assert!(matches!(store.load(), Err(StoreError::Encoding { .. })));
        std::fs::remove_dir_all(&dir).unwrap();
    }

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
