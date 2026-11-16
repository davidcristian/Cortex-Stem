//! Executables under `WindowsApps`: which application of an installed
//! package an executable belongs to.

use windows::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
use windows::Win32::Storage::Packaging::Appx::{
    GetPackagesByPackageFamily, PACKAGE_FAMILY_NAME_MAX_LENGTH, PackageFamilyNameFromFullName,
};
use windows::core::PWSTR;

use super::names::apps_folder_display_name;
use super::package_info::{manifest_executables, package_application_ids};
use crate::rules::identity::{PackagedExe, choose_package_app, package_app_aumid, strings_at};
use crate::windows::util;
use crate::{PlatformError, PlatformResult};

/// At most this many installed packages of one family are looked at (a user
/// has one version of a package; resource packages share the family).
const MAX_FAMILY_PACKAGES: usize = 16;
/// Upper bound for the buffers the package APIs ask for, in bytes.
pub(super) const MAX_PACKAGE_BUFFER_BYTES: u32 = 64 * 1024;

/// The AppUserModelID of the packaged application that the executable at
/// `exe` (a path under `WindowsApps`) belongs to, or `None` when the package
/// is not installed for this user, has no application, or the choice is not
/// certain (`rules::identity::choose_package_app`); each `None` is logged at
/// debug. The directory in the path may name a version that has since been
/// updated away, so the package's family is looked up and whatever version
/// of it is installed now answers. Local calls only: nothing here waits on
/// another process.
pub(super) fn packaged_app_aumid(exe: &PackagedExe<'_>) -> Option<String> {
    let full_name = exe.package_full_name;
    let family = package_family_name(full_name)
        .inspect_err(|e| log::debug!("{full_name}: PackageFamilyNameFromFullName failed: {e}"))
        .ok()?;
    let installed = match packages_of_family(&family) {
        Ok(names) if names.is_empty() => {
            log::debug!("{full_name}: no package of family {family} is installed for this user");
            return None;
        }
        Ok(names) => names,
        Err(e) => {
            log::debug!("{family}: GetPackagesByPackageFamily failed: {e}; trying {full_name}");
            vec![full_name.to_owned()]
        }
    };

    let mut apps: Vec<String> = Vec::new();
    for name in &installed {
        // The installed name carries the package's own spelling; the one in
        // the path may have been lower-cased along the way.
        let own_family = package_family_name(name).unwrap_or_else(|_| family.clone());
        match package_application_ids(name) {
            Ok(ids) => {
                for aumid in ids
                    .iter()
                    .filter_map(|id| package_app_aumid(&own_family, id))
                {
                    if !apps.contains(&aumid) {
                        apps.push(aumid);
                    }
                }
            }
            Err(e) => log::debug!("{name}: reading the application ids failed: {e}"),
        }
    }
    if apps.is_empty() {
        log::debug!("{full_name}: the installed package {installed:?} has no application");
        return None;
    }
    // The manifest only matters when there is a choice to make.
    let executables: Vec<(String, String)> = if apps.len() > 1 {
        installed
            .iter()
            .flat_map(|name| {
                manifest_executables(name)
                    .inspect_err(|e| log::debug!("{name}: reading AppxManifest.xml failed: {e}"))
                    .unwrap_or_default()
            })
            .collect()
    } else {
        Vec::new()
    };
    // The AppsFolder also resolves entries the Start menu does not list
    // (measured: `Claude_pzs8sxrjxfjjc!SshProxy` answers "Claude"), so this
    // tie-break rarely decides; the manifest does the real work.
    let chosen = choose_package_app(&apps, &executables, exe.relative_path, |aumid| {
        apps_folder_display_name(aumid).is_some()
    });
    if chosen.is_none() {
        log::debug!(
            "{full_name}: cannot tell which of {apps:?} runs {:?} (manifest executables {executables:?})",
            exe.relative_path
        );
    }
    chosen.map(str::to_owned)
}

/// `PackageFamilyNameFromFullName`: computed from the name alone, so it
/// works for a version that is no longer installed.
pub(super) fn package_family_name(full_name: &str) -> PlatformResult<String> {
    let wide = util::to_wide(full_name);
    let mut buffer = vec![0u16; PACKAGE_FAMILY_NAME_MAX_LENGTH as usize + 1];
    let mut len = buffer.len() as u32;
    // SAFETY: `wide` is NUL-terminated; `buffer` is writable for `len` UTF-16
    // units and `len` is a live in/out pointer.
    unsafe {
        PackageFamilyNameFromFullName(
            util::pcwstr(&wide),
            &mut len,
            Some(PWSTR(buffer.as_mut_ptr())),
        )
    }
    .ok()?;
    Ok(util::from_wide(&buffer))
}

/// Full names of the packages of `family` installed for the current user
/// (`GetPackagesByPackageFamily`); empty when there is none.
pub(super) fn packages_of_family(family: &str) -> PlatformResult<Vec<String>> {
    let wide = util::to_wide(family);
    let mut count = 0u32;
    let mut len = 0u32;
    // SAFETY: size query: no output arrays, `count` and `len` are live
    // out-pointers, `wide` is NUL-terminated.
    let err = unsafe {
        GetPackagesByPackageFamily(util::pcwstr(&wide), &mut count, None, &mut len, None)
    };
    if count == 0 && (err.is_ok() || err == ERROR_INSUFFICIENT_BUFFER) {
        return Ok(Vec::new());
    }
    if err != ERROR_INSUFFICIENT_BUFFER {
        return Err(windows::core::Error::from(err).into());
    }
    if count as usize > MAX_FAMILY_PACKAGES || len.saturating_mul(2) > MAX_PACKAGE_BUFFER_BYTES {
        return Err(PlatformError::Other(format!(
            "{family}: {count} packages in {len} UTF-16 units is more than expected"
        )));
    }
    let mut names = vec![PWSTR::null(); count as usize];
    let mut buffer = vec![0u16; len as usize];
    // SAFETY: `names` holds `count` pointers and `buffer` `len` units, the
    // sizes the query asked for; both outlive the call.
    unsafe {
        GetPackagesByPackageFamily(
            util::pcwstr(&wide),
            &mut count,
            Some(names.as_mut_ptr()),
            &mut len,
            Some(PWSTR(buffer.as_mut_ptr())),
        )
    }
    .ok()?;
    let addresses: Vec<usize> = names
        .iter()
        .take(count as usize)
        .map(|p| p.0 as usize)
        .collect();
    Ok(strings_at(&buffer, buffer.as_ptr() as usize, &addresses))
}
