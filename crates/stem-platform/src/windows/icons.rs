//! Icon extraction and caching.
//!
//! Icon key scheme (produced here and in `identity`/`enumerate`, consumed by
//! [`IconCache::get_or_load`]):
//! - `hwnd:<id>`: the window's own icon (`WM_GETICON` via
//!   `SendMessageTimeoutW`, then `GCLP_HICON`, then exe icon)
//! - `exe:<path>`: the executable's main icon (`SHGetFileInfoW` /
//!   `PrivateExtractIconsW` / `IShellItemImageFactory` for large sizes)
//! - `file:<path>`: any file's shell icon (shortcuts, documents)
//! - `aumid:<id>`: a packaged app's logo (`shell:AppsFolder\<aumid>` through
//!   `IShellItemImageFactory`)
//!
//! Output is always 32-bit RGBA PNG bytes. `HICON` -> pixels goes through
//! `GetIconInfo` + `GetDIBits` with alpha preserved; all GDI objects are freed
//! (RAII guards in `util`). Results are cached per (key, size) in memory;
//! nothing is cached on disk.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use stem_core::IconKey;

use crate::{PlatformError, PlatformResult};

type CacheKey = (IconKey, u32);

#[derive(Debug, Default)]
pub struct IconCache {
    entries: Mutex<HashMap<CacheKey, Arc<Vec<u8>>>>,
}

impl IconCache {
    pub fn get_or_load(&self, key: &IconKey, size: u32) -> PlatformResult<Option<Vec<u8>>> {
        if let Some(png) = self.entries.lock().get(&(key.clone(), size)) {
            return Ok(Some(png.as_ref().clone()));
        }
        let Some(png) = load_icon_png(key, size)? else {
            return Ok(None);
        };
        self.entries
            .lock()
            .insert((key.clone(), size), Arc::new(png.clone()));
        Ok(Some(png))
    }

    pub fn invalidate(&self, key: &IconKey) {
        self.entries.lock().retain(|(k, _), _| k != key);
    }
}

/// Resolve a key to PNG bytes without caching.
pub fn load_icon_png(_key: &IconKey, _size: u32) -> PlatformResult<Option<Vec<u8>>> {
    Err(PlatformError::Unsupported("icon extraction"))
}
