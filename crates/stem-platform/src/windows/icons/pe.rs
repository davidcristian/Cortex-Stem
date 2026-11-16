//! The `PE resources` chain: the first `RT_GROUP_ICON` of a PE image,
//! read from the file mapped as data.

use std::fmt;

use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{
    EnumResourceNamesW, FindResourceW, LoadResource, LockResource, SizeofResource,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, LR_DEFAULTCOLOR, RT_GROUP_ICON, RT_ICON,
};
use windows::core::{BOOL, PCWSTR};

use super::MAX_SIZE;
use super::bitmap::icon_to_image;
use crate::rules::icons::{self as pixels, IconImage};
use crate::windows::util::{self, LoadedLibrary, OwnedIcon};
use crate::{PlatformError, PlatformResult};

/// An `RT_GROUP_ICON` or `RT_ICON` resource larger than this is not an icon
/// (a 256 px 32-bit uncompressed image is about 270 KB).
const MAX_RESOURCE_BYTES: u32 = 4 * 1024 * 1024;
/// `dwVer` for `CreateIconFromResourceEx`: the Windows 3.x icon format, the
/// only value the function accepts.
const ICON_RESOURCE_VERSION: u32 = 0x0003_0000;

/// A resource name copied out of an `EnumResourceNamesW` callback, where
/// the string form is only valid for the duration of the call.
#[derive(Debug)]
enum ResourceName {
    /// `MAKEINTRESOURCE` id: the high word of the "pointer" is zero.
    Id(u16),
    /// A NUL-terminated string name.
    Name(Vec<u16>),
}

impl ResourceName {
    /// Copy a callback's `lpName`.
    ///
    /// # Safety
    /// `name` must be an integer resource id or a pointer to a
    /// NUL-terminated string that is readable for the duration of the call.
    unsafe fn from_callback(name: PCWSTR) -> Self {
        let raw = name.0 as usize;
        if raw >> 16 == 0 {
            return Self::Id(raw as u16);
        }
        // SAFETY: by this function's contract, `name` addresses a
        // NUL-terminated string for the duration of the callback.
        let wide = unsafe { name.as_wide() };
        Self::Name(wide.iter().copied().chain(std::iter::once(0)).collect())
    }

    /// The `lpName` to pass to `FindResourceW`; the pointer borrows `self`.
    fn as_pcwstr(&self) -> PCWSTR {
        match self {
            Self::Id(id) => PCWSTR(usize::from(*id) as *const u16),
            Self::Name(wide) => util::pcwstr(wide),
        }
    }
}

impl fmt::Display for ResourceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Id(id) => write!(f, "#{id}"),
            Self::Name(wide) => write!(f, "\"{}\"", util::from_wide(wide)),
        }
    }
}

/// `PE resources` chain: the first `RT_GROUP_ICON` of the mapped file, its
/// best entry of at most `MAX_SIZE` created at its native size. `Ok(None)`
/// when the file has no icon group; `Err` for a file that cannot be mapped
/// (missing, unreadable, not a PE image), a malformed directory, or a
/// resource that cannot be read or decoded. The icon is owned and
/// destroyed here; the mapping is freed on return.
pub(super) fn pe_resource_icon(path: &str) -> PlatformResult<Option<IconImage>> {
    let module = LoadedLibrary::open_as_data(path)?;
    let Some(group) = first_group_icon_name(&module) else {
        log::debug!("{path}: no RT_GROUP_ICON resource");
        return Ok(None);
    };
    let directory = read_resource(&module, RT_GROUP_ICON, &group)?;
    let entries = pixels::parse_group_icon_dir(&directory)
        .map_err(|e| PlatformError::Other(format!("icon group {group}: {e}")))?;
    let Some(entry) = pixels::select_group_icon_entry(&entries, MAX_SIZE) else {
        log::debug!("{path}: icon group {group} has no image of at most {MAX_SIZE} px");
        return Ok(None);
    };
    let bytes = read_resource(&module, RT_ICON, &ResourceName::Id(entry.id))?;
    // SAFETY: `bytes` is a whole RT_ICON resource (SizeofResource's length);
    // the function reads it and returns an icon we own, wrapped in a guard.
    let icon = unsafe {
        CreateIconFromResourceEx(
            &bytes,
            true,
            ICON_RESOURCE_VERSION,
            entry.width as i32,
            entry.height as i32,
            LR_DEFAULTCOLOR,
        )
    }?;
    let Some(icon) = OwnedIcon::new(icon) else {
        return Ok(None);
    };
    log::trace!(
        "{path}: icon group {group}: entry #{} {}x{} {}-bit, {} bytes",
        entry.id,
        entry.width,
        entry.height,
        entry.bit_count,
        entry.bytes_in_res
    );
    icon_to_image(icon.handle())
}

/// The name of the first `RT_GROUP_ICON` resource of a mapped module (the
/// one `ExtractIconW(.., 0)` and the shell use as the application icon), or
/// `None` when it has none.
fn first_group_icon_name(module: &LoadedLibrary) -> Option<ResourceName> {
    let mut found: Option<ResourceName> = None;
    let lparam = std::ptr::from_mut(&mut found) as isize;
    // SAFETY: `take_first_name` only writes to the Option addressed by
    // `lparam`, which outlives the call, and stops the enumeration after the
    // first name; the module handle is live for `module`'s lifetime.
    let completed = unsafe {
        EnumResourceNamesW(
            Some(module.handle()),
            RT_GROUP_ICON,
            Some(take_first_name),
            lparam,
        )
    };
    if found.is_none() && !completed.as_bool() {
        log::trace!("EnumResourceNamesW(RT_GROUP_ICON): {}", util::last_error());
    }
    found
}

/// `EnumResourceNamesW` callback: copy the first name into the `Option`
/// behind `lparam` and stop.
unsafe extern "system" fn take_first_name(
    _module: HMODULE,
    _kind: PCWSTR,
    name: PCWSTR,
    lparam: isize,
) -> BOOL {
    // SAFETY: `name` is valid for the duration of this callback, as the
    // enumeration contract guarantees.
    let copied = unsafe { ResourceName::from_callback(name) };
    let slot = lparam as *mut Option<ResourceName>;
    // SAFETY: `lparam` is the pointer `first_group_icon_name` passed, to an
    // Option that outlives the enumeration and is not aliased meanwhile.
    unsafe { *slot = Some(copied) };
    BOOL(0)
}

/// The bytes of resource `name` of type `kind`, copied out of the mapping
/// so they outlive it. Bounded by `MAX_RESOURCE_BYTES`.
fn read_resource(
    module: &LoadedLibrary,
    kind: PCWSTR,
    name: &ResourceName,
) -> PlatformResult<Vec<u8>> {
    // SAFETY: the module handle is live for `module`'s lifetime; `kind` is
    // an RT_* id and `name` an id or a NUL-terminated string owned by the
    // caller for the duration of the call.
    let info = unsafe { FindResourceW(Some(module.handle()), name.as_pcwstr(), kind) };
    if info.is_invalid() {
        return Err(util::last_error());
    }
    // SAFETY: `info` is a valid resource handle of this module.
    let size = unsafe { SizeofResource(Some(module.handle()), info) };
    if size == 0 || size > MAX_RESOURCE_BYTES {
        return Err(PlatformError::Other(format!(
            "resource {name} is {size} bytes"
        )));
    }
    // SAFETY: as above; for a module mapped as a data file the "global"
    // handle is a pointer into the mapping and needs no freeing.
    let data = unsafe { LoadResource(Some(module.handle()), info) }?;
    // SAFETY: `data` came from LoadResource on a live mapping.
    let ptr = unsafe { LockResource(data) };
    if ptr.is_null() {
        return Err(PlatformError::Other(format!(
            "resource {name} could not be locked"
        )));
    }
    // SAFETY: `ptr` addresses `size` readable bytes inside the mapping, which
    // `module` keeps alive until after this copy.
    let bytes = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), size as usize) };
    Ok(bytes.to_vec())
}
