use pretty_assertions::assert_eq;

use super::*;

#[test]
fn shortcut_plan_groups_by_aumid_and_launches_the_target() {
    use ShortcutSource::*;

    // VS Code / Chrome profile: target + AUMID the shell knows.
    assert_eq!(
        plan_shortcut(true, Some("Chrome.Profile1"), true),
        ShortcutPlan {
            id: AppUserModelId,
            name: AppUserModelId,
            icon: AppUserModelId,
            launch: Target,
        }
    );
    // Target + AUMID the shell does not know: grouped by AUMID, but the
    // exe supplies name, icon and launch.
    assert_eq!(
        plan_shortcut(true, Some("Vendor.App"), false),
        ShortcutPlan {
            id: AppUserModelId,
            name: Target,
            icon: Target,
            launch: Target,
        }
    );
    // Plain shortcut to an exe.
    assert_eq!(
        plan_shortcut(true, None, false),
        ShortcutPlan {
            id: Target,
            name: Target,
            icon: Target,
            launch: Target,
        }
    );
    // `apps_folder_known` is meaningless without an AUMID.
    assert_eq!(
        plan_shortcut(true, None, true),
        plan_shortcut(true, None, false)
    );
    assert_eq!(
        plan_shortcut(true, Some("  "), true),
        plan_shortcut(true, None, false)
    );
    // Store-app shortcut: no target, AUMID known to the shell.
    assert_eq!(
        plan_shortcut(
            false,
            Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"),
            true
        ),
        ShortcutPlan {
            id: AppUserModelId,
            name: AppUserModelId,
            icon: AppUserModelId,
            launch: AppUserModelId,
        }
    );
    // No target, AUMID unknown to the shell: still launched by AUMID,
    // named and drawn from the link.
    assert_eq!(
        plan_shortcut(false, Some("Gone.App"), false),
        ShortcutPlan {
            id: AppUserModelId,
            name: Link,
            icon: Link,
            launch: AppUserModelId,
        }
    );
    // Nothing to go on: the link itself.
    assert_eq!(
        plan_shortcut(false, None, false),
        ShortcutPlan {
            id: Link,
            name: Link,
            icon: Link,
            launch: Link,
        }
    );
}
