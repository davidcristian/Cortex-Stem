use super::package::{package_family_name, packages_of_family};
use super::package_info::{PackageInfo, manifest_executables, package_application_ids};
use super::*;

const TERMINAL_FAMILY: &str = "Microsoft.WindowsTerminal_8wekyb3d8bbwe";
/// A version no machine has: the pin of a Terminal that was updated since.
const STALE_TERMINAL_EXE: &str = r"C:\Program Files\WindowsApps\Microsoft.WindowsTerminal_1.0.0.0_x64__8wekyb3d8bbwe\WindowsTerminal.exe";

fn exe(path: &str) -> LaunchTarget {
    LaunchTarget::Exe {
        path: PathBuf::from(path),
        args: Vec::new(),
        working_dir: None,
    }
}

#[test]
fn family_names_are_computed_from_the_full_name_alone() {
    assert_eq!(
        package_family_name("Microsoft.WindowsTerminal_1.0.0.0_x64__8wekyb3d8bbwe").unwrap(),
        TERMINAL_FAMILY
    );
    assert!(package_family_name("not a package").is_err());
}

#[test]
fn an_exe_of_a_package_that_is_not_installed_keeps_its_exe_identity() {
    let path =
        r"C:\Program Files\WindowsApps\Stem.Test.NotInstalled_1.0.0.0_x64__0000000000000\app.exe";
    assert_eq!(
        packages_of_family("Stem.Test.NotInstalled_0000000000000").unwrap(),
        Vec::<String>::new()
    );
    assert!(PackageInfo::open("Stem.Test.NotInstalled_1.0.0.0_x64__0000000000000").is_err());
    let identity = identify_target(&exe(path)).unwrap();
    assert_eq!(identity.id, AppId::for_exe(Path::new(path)));
    assert_eq!(identity.display_name, "app");
    assert_eq!(identity.launch, Some(exe(path)));
}

#[test]
fn a_stale_terminal_exe_is_identified_as_the_installed_terminal() {
    let installed = packages_of_family(TERMINAL_FAMILY).unwrap();
    let Some(current) = installed.first() else {
        eprintln!("Windows Terminal is not installed on this machine; nothing to check");
        return;
    };
    let aumid = format!("{TERMINAL_FAMILY}!App");
    assert_eq!(
        package_application_ids(current).unwrap(),
        vec![aumid.clone()]
    );
    // The manifest is not needed for one application, but it must read.
    let executables = {
        let _com = util::ComApartment::sta();
        manifest_executables(current).unwrap()
    };
    assert!(
        executables.contains(&(aumid.clone(), "WindowsTerminal.exe".to_owned())),
        "{executables:?}"
    );

    // The same identity a running Terminal window gets, whatever
    // the spelling of the path.
    for path in [
        STALE_TERMINAL_EXE.to_owned(),
        STALE_TERMINAL_EXE.to_lowercase(),
    ] {
        let identity = identify_target(&exe(&path)).unwrap();
        assert_eq!(identity.id, AppId::for_app_user_model_id(&aumid), "{path}");
        assert_eq!(identity.icon_key, Some(aumid_icon_key(&aumid)));
        assert_eq!(
            identity.launch,
            Some(LaunchTarget::StoreApp {
                app_user_model_id: aumid.clone()
            })
        );
        assert_ne!(identity.display_name, "Windows Terminal Host");
        assert!(!identity.display_name.to_lowercase().ends_with(".exe"));
    }
}

/// The Microsoft Store package declares three applications, one per
/// executable: the manifest picks the one that runs `WinStore.App.exe`.
#[test]
fn the_manifest_tells_the_applications_of_a_package_apart() {
    let family = "Microsoft.WindowsStore_8wekyb3d8bbwe";
    if packages_of_family(family).unwrap().is_empty() {
        eprintln!("the Microsoft Store is not installed on this machine; nothing to check");
        return;
    }
    let path = r"C:\Program Files\WindowsApps\Microsoft.WindowsStore_1.0.0.0_x64__8wekyb3d8bbwe\WinStore.App.exe";
    let identity = identify_target(&exe(path)).unwrap();
    assert_eq!(
        identity.id,
        AppId::for_app_user_model_id(&format!("{family}!App"))
    );
}
