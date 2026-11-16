//! Identity parsing helpers.
//!
//! `shell:AppsFolder\<aumid>` parsing, environment-variable detection for
//! shortcut targets, display-name fallbacks, version-resource lookups,
//! icon-key construction, Windows command-line splitting/joining, and the
//! packaged-executable rules (package full name from a path under
//! `WindowsApps`, which of a package's applications an executable belongs
//! to). Pure and tested here.
//!
//! Paths are handled as strings on purpose: `std::path::Path` only treats
//! `\` as a separator on Windows, and these helpers must behave the same in
//! tests on any host (`docs/development/failure-modes.md`).

mod aliases;
mod command_line;
mod icon_keys;
mod names;
mod packages;
mod paths;
mod shortcuts;

pub use aliases::{
    AppExecLink, IO_REPARSE_TAG_APPEXECLINK, alias_filter_matches, parse_app_exec_link,
    windows_dir_exe,
};
pub use command_line::{join_command_line, quote_argument, split_command_line};
pub use icon_keys::{
    WINDOW_ICON_NAMESPACE, aumid_icon_key, exe_icon_key, file_icon_key, hwnd_icon_key,
    hwnd_icon_owner_matches, is_window_icon_key, parse_hwnd_icon_value,
};
pub use names::{
    PLACEHOLDER_DISPLAY_NAME, VERSION_NAME_FIELDS, aumid_fallback_name, clean_display_name,
    display_name_fallback, first_display_name, first_non_empty, translation_candidates,
    translations, version_string_path, version_string_value,
};
pub use packages::{
    PackagedExe, WINDOWS_APPS_DIR, choose_package_app, is_package_full_name, package_app_aumid,
    packaged_exe, same_relative_path, strings_at,
};
pub use paths::{
    APPS_FOLDER_PREFIX, apps_folder_parsing_name, default_working_dir, file_name, file_stem,
    has_extension, needs_env_expansion, parse_apps_folder,
};
pub use shortcuts::{ShortcutPlan, ShortcutSource, plan_shortcut};

#[cfg(test)]
mod tests;
