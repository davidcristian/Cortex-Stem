//! What an installed package contains: its application ids, its path and
//! the executables its manifest names.

use windows::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
use windows::Win32::Storage::Packaging::Appx::{
    _PACKAGE_INFO_REFERENCE, AppxFactory, ClosePackageInfo, GetPackageApplicationIds,
    GetPackagePathByFullName, IAppxFactory, OpenPackageInfoByFullName,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, STGM_READ, STGM_SHARE_DENY_NONE,
};
use windows::Win32::UI::Shell::SHCreateStreamOnFileEx;
use windows::core::{PWSTR, w};

use super::package::MAX_PACKAGE_BUFFER_BYTES;
use crate::rules::identity::strings_at;
use crate::windows::util::{self, CoTaskWideString};
use crate::{PlatformError, PlatformResult};

/// At most this many applications are read from one manifest.
const MAX_MANIFEST_APPLICATIONS: usize = 64;

/// A package-information reference from `OpenPackageInfoByFullName`,
/// closed on drop.
pub(super) struct PackageInfo(*mut _PACKAGE_INFO_REFERENCE);

impl PackageInfo {
    pub(super) fn open(full_name: &str) -> PlatformResult<Self> {
        let wide = util::to_wide(full_name);
        let mut reference: *mut _PACKAGE_INFO_REFERENCE = std::ptr::null_mut();
        // SAFETY: `wide` is NUL-terminated, `reference` is a live out-pointer
        // and the reserved argument is 0.
        unsafe { OpenPackageInfoByFullName(util::pcwstr(&wide), None, &mut reference) }.ok()?;
        if reference.is_null() {
            return Err(PlatformError::Other(format!(
                "{full_name}: OpenPackageInfoByFullName returned no reference"
            )));
        }
        Ok(Self(reference))
    }
}

impl Drop for PackageInfo {
    fn drop(&mut self) {
        // SAFETY: the reference came from a successful OpenPackageInfoByFullName
        // and is closed exactly once, here.
        let err = unsafe { ClosePackageInfo(self.0) };
        if err.is_err() {
            log::debug!("ClosePackageInfo failed: {err:?}");
        }
    }
}

/// The application ids of an installed package (`GetPackageApplicationIds`:
/// full AUMIDs, see `rules::identity::package_app_aumid`).
pub(super) fn package_application_ids(full_name: &str) -> PlatformResult<Vec<String>> {
    let info = PackageInfo::open(full_name)?;
    let mut len = 0u32;
    let mut count = 0u32;
    // SAFETY: size query with no buffer; `len` and `count` are live
    // out-pointers; `info` is open.
    let err = unsafe { GetPackageApplicationIds(info.0, &mut len, None, Some(&mut count)) };
    if err.is_ok() {
        // Nothing to copy: a package without applications.
        return Ok(Vec::new());
    }
    if err != ERROR_INSUFFICIENT_BUFFER {
        return Err(windows::core::Error::from(err).into());
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    if len > MAX_PACKAGE_BUFFER_BYTES {
        return Err(PlatformError::Other(format!(
            "{full_name}: {len} bytes of application ids is more than expected"
        )));
    }
    // The API writes a table of `count` pointers at the start of the buffer
    // and the strings after it: allocate pointer-sized units so the table is
    // aligned.
    let unit = std::mem::size_of::<usize>();
    let mut buffer = vec![0usize; (len as usize).div_ceil(unit)];
    let mut len = (buffer.len() * unit) as u32;
    // SAFETY: `buffer` is writable for `len` bytes and outlives the call;
    // `info` is open.
    unsafe {
        GetPackageApplicationIds(
            info.0,
            &mut len,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut count),
        )
    }
    .ok()?;
    let addresses: Vec<usize> = buffer.iter().take(count as usize).copied().collect();
    // The same bytes as UTF-16 units, read by offset (never through the
    // pointers themselves).
    let bytes: Vec<u8> = buffer.iter().flat_map(|w| w.to_ne_bytes()).collect();
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_ne_bytes([c[0], c[1]]))
        .collect();
    Ok(strings_at(&units, buffer.as_ptr() as usize, &addresses))
}

/// Where an installed package lives (`GetPackagePathByFullName`).
fn package_path(full_name: &str) -> PlatformResult<String> {
    let wide = util::to_wide(full_name);
    let mut len = 0u32;
    // SAFETY: size query with no buffer; `len` is a live out-pointer.
    let err = unsafe { GetPackagePathByFullName(util::pcwstr(&wide), &mut len, None) };
    if err != ERROR_INSUFFICIENT_BUFFER {
        return Err(windows::core::Error::from(err).into());
    }
    if len == 0 || len.saturating_mul(2) > MAX_PACKAGE_BUFFER_BYTES {
        return Err(PlatformError::Other(format!(
            "{full_name}: a package path of {len} UTF-16 units"
        )));
    }
    let mut buffer = vec![0u16; len as usize];
    // SAFETY: `buffer` is writable for `len` UTF-16 units, the size the
    // query asked for.
    unsafe {
        GetPackagePathByFullName(
            util::pcwstr(&wide),
            &mut len,
            Some(PWSTR(buffer.as_mut_ptr())),
        )
    }
    .ok()?;
    Ok(util::from_wide(&buffer))
}

/// `(AUMID, Executable)` of every application in an installed package's
/// `AppxManifest.xml`, read with the OS's own manifest reader. Applications
/// without an `Executable` are left out. The caller holds a COM apartment
/// (`identify_target` does).
pub(super) fn manifest_executables(full_name: &str) -> PlatformResult<Vec<(String, String)>> {
    let root = package_path(full_name)?;
    let manifest = format!(r"{}\AppxManifest.xml", root.trim_end_matches(['\\', '/']));
    let wide = util::to_wide(&manifest);
    // SAFETY: `wide` is NUL-terminated; read-only, nothing is created, no
    // template stream.
    let stream = unsafe {
        SHCreateStreamOnFileEx(
            util::pcwstr(&wide),
            (STGM_READ | STGM_SHARE_DENY_NONE).0,
            0,
            false,
            None,
        )
    }?;
    // SAFETY: standard in-process activation of the Appx factory.
    let factory: IAppxFactory =
        unsafe { CoCreateInstance(&AppxFactory, None, CLSCTX_INPROC_SERVER) }?;
    // SAFETY: plain COM calls on live interfaces; every returned string is
    // CoTaskMem-allocated and freed by its guard.
    let reader = unsafe { factory.CreateManifestReader(&stream) }?;
    // SAFETY: as above.
    let applications = unsafe { reader.GetApplications() }?;
    let mut out = Vec::new();
    // SAFETY: as above.
    let mut has_current = unsafe { applications.GetHasCurrent() }?.as_bool();
    while has_current && out.len() < MAX_MANIFEST_APPLICATIONS {
        // SAFETY: as above.
        let application = unsafe { applications.GetCurrent() }?;
        // SAFETY: as above.
        let aumid = unsafe { application.GetAppUserModelId() }
            .ok()
            .and_then(CoTaskWideString::new)
            .map(|s| s.to_string_lossy());
        // SAFETY: as above.
        let executable = unsafe { application.GetStringValue(w!("Executable")) }
            .ok()
            .and_then(CoTaskWideString::new)
            .map(|s| s.to_string_lossy());
        if let (Some(aumid), Some(executable)) = (aumid, executable)
            && !executable.trim().is_empty()
        {
            out.push((aumid, executable));
        }
        // SAFETY: as above.
        has_current = unsafe { applications.MoveNext() }?.as_bool();
    }
    Ok(out)
}
