//! Display names: the AppsFolder's name for an AUMID, and the version
//! resource of an executable.

use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::UI::Shell::{IShellItem, SHCreateItemFromParsingName, SIGDN_NORMALDISPLAY};
use windows::core::{PCWSTR, w};

use crate::rules::identity::{
    VERSION_NAME_FIELDS, apps_folder_parsing_name, clean_display_name, translation_candidates,
    translations, version_string_path, version_string_value,
};
use crate::windows::util::{self, CoTaskWideString};

/// Display name of `shell:AppsFolder\<aumid>`; `None` when the shell does
/// not know the AUMID (or names it `.exe`-only, which no name may be).
pub(super) fn apps_folder_display_name(aumid: &str) -> Option<String> {
    let name = util::to_wide(&apps_folder_parsing_name(aumid));
    // SAFETY: `name` is NUL-terminated and outlives the call; no bind context.
    let item: IShellItem = unsafe { SHCreateItemFromParsingName(util::pcwstr(&name), None) }
        .inspect_err(|e| log::trace!("AppsFolder item for {aumid:?} not found: {e}"))
        .ok()?;
    // SAFETY: plain COM call; the returned string is CoTaskMem-allocated and
    // freed by the guard.
    let display = unsafe { item.GetDisplayName(SIGDN_NORMALDISPLAY) }.ok()?;
    let display = CoTaskWideString::new(display)?.to_string_lossy();
    clean_display_name(&display)
}

/// `FileDescription`, else `ProductName`, from the exe's version resource,
/// cleaned (some resources repeat the file name, `.exe` included).
pub(super) fn exe_display_name(path: &str) -> Option<String> {
    let block = version_block(path)?;
    let declared = query_bytes(&block, w!(r"\VarFileInfo\Translation"))
        .map(|b| translations(&b))
        .unwrap_or_default();
    for field in VERSION_NAME_FIELDS {
        for (lang, codepage) in translation_candidates(&declared) {
            let sub_block = util::to_wide(&version_string_path(lang, codepage, field));
            if let Some(value) =
                query_string(&block, util::pcwstr(&sub_block)).and_then(|v| clean_display_name(&v))
            {
                return Some(value);
            }
        }
    }
    None
}

/// The whole `VS_VERSIONINFO` block of a file, or `None` if it has none.
fn version_block(path: &str) -> Option<Vec<u8>> {
    let wide = util::to_wide(path);
    // SAFETY: NUL-terminated path; no handle output requested.
    let size = unsafe { GetFileVersionInfoSizeW(util::pcwstr(&wide), None) };
    if size == 0 {
        return None;
    }
    let mut block = vec![0u8; size as usize];
    // SAFETY: `block` holds exactly `size` bytes as reported by the size query.
    unsafe { GetFileVersionInfoW(util::pcwstr(&wide), None, size, block.as_mut_ptr().cast()) }
        .inspect_err(|e| log::trace!("{path}: GetFileVersionInfoW failed: {e}"))
        .ok()?;
    Some(block)
}

/// Pointer and length (unit depends on the sub-block) of a value inside a
/// version block. The pointer refers into `block`.
fn query_value(block: &[u8], sub_block: PCWSTR) -> Option<(*const core::ffi::c_void, usize)> {
    let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
    let mut len: u32 = 0;
    // SAFETY: `block` is a complete version block; `sub_block` is a
    // NUL-terminated string that outlives the call; `ptr`/`len` are valid
    // out-pointers.
    let ok = unsafe { VerQueryValueW(block.as_ptr().cast(), sub_block, &mut ptr, &mut len) };
    if !ok.as_bool() || ptr.is_null() || len == 0 {
        return None;
    }
    let offset = (ptr as usize).checked_sub(block.as_ptr() as usize)?;
    (offset < block.len()).then_some((ptr.cast_const(), len as usize))
}

/// A binary value (e.g. the translation table), copied out of the block.
fn query_bytes(block: &[u8], sub_block: PCWSTR) -> Option<Vec<u8>> {
    let (ptr, len) = query_value(block, sub_block)?;
    let offset = ptr as usize - block.as_ptr() as usize;
    let end = offset.checked_add(len)?;
    block.get(offset..end).map(<[u8]>::to_vec)
}

/// A string value (`len` is in UTF-16 units, NUL included or not).
fn query_string(block: &[u8], sub_block: PCWSTR) -> Option<String> {
    let (ptr, len) = query_value(block, sub_block)?;
    let offset = ptr as usize - block.as_ptr() as usize;
    Some(version_string_value(block.get(offset..)?, len))
}
