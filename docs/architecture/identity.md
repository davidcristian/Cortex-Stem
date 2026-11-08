# Identity and grouping

This file describes how Stem decides which app a window or a pin belongs to, and how that
app gets its name.

`AppId` is the grouping key (`stem_core::app_id_for`):

1. `aumid:<AppUserModelID>` when the window or its process declares an
   AppUserModelID (AUMID): the window's `System.AppUserModel.ID` property
   (UWP frames, browsers with profiles, apps that set one), else the
   process's package identity (`GetApplicationUserModelId`). Packaged Win32 apps such as the Windows 11
   Notepad, Terminal and Paint have no window property, and their exe under
   `WindowsApps` cannot be read, so the package identity is the only way to
   name, group and launch them like Store apps.
2. Else `exe:<normalised path>` (lower-case, forward slashes).
3. Else `class:<window class>`.

Pinned entries store the same id, so a pinned app and its running windows
merge into one item. `identify_target` must return the id that the running
windows will have: it resolves a `.lnk` to its target exe, and a Store app
to its AUMID. A `.lnk` with a `System.AppUserModel.ID` property is
identified by that AUMID, because its app's windows are grouped by it.

## Executables of packaged apps

An `Exe` target under `\WindowsApps\<package full name>\`
(`rules::identity::packaged_exe`) is identified as the packaged application
it belongs to. The platform takes the family from the full name
(`PackageFamilyNameFromFullName`), then the version installed now
(`GetPackagesByPackageFamily`, so a path that names a version since updated
still resolves), then that version's applications
(`GetPackageApplicationIds`, full AUMIDs). With one application, that
application is the result. With several, the manifest's `Executable`
(`IAppxManifestReader`) picks the one that runs this exe, and the
AppsFolder decides a remaining tie (`rules::identity::choose_package_app`).
Anything not certain keeps the `exe:` identity.

Settings files from earlier versions of Stem can pin such an app by its exe
(an `exe:` id and an `Exe` target), and that pin never merges with the
app's `aumid:` windows. So once per start, before it serves any request,
the identity thread passes every such pin (`exe:` id, `Exe` target under
`WindowsApps`, no arguments) through `identify_target`. It rewrites in
place each pin that comes back with a different `aumid:` id: the pin gets
that id and a `StoreApp` target, and keeps its label. A pin whose new id is
already pinned is dropped instead. The settings are saved once, and each
change is logged at info (`re-identified pin <old> as <new>`). Failures and
ambiguous results leave the pin as it is (`stem_service::reidentify`).

## Display names

A display name comes from the AppsFolder for an AUMID that the shell
resolves. Otherwise it comes from the exe's version resource
(`FileDescription`, then `ProductName`), the shortcut name, the file stem,
the window title and the class name, in that order. Every candidate is
cleaned (`rules::identity::clean_display_name`), so no name ends in `.exe`.
"Application" is the last resort.

## Elevated and protected processes

The dock runs at medium integrity. Everything the window probe reads
(visibility, class, title, styles, owner, cloak flags, frame) is win32k
state that User Interface Privilege Isolation (UIPI) does not block. So an
elevated window is listed with its title and class like any other.
`OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` succeeds for an elevated
process of the same user, so its image path and package identity are read.
It gets the same `exe:` or `aumid:` id, name, icon and launch target as
the same app running unelevated. A pin of it therefore launches
unelevated, or through UAC when the exe's manifest requires elevation.

Only for a *protected* process does the open fail. Its window keeps its
title and class, `exe_path` is `None`, the id is `class:<class>`, the name
is the window title (then the class), the icon key is `hwnd:` and the
launch target is `None`. The failure is logged once per process at debug
(`rules::process_access`). The open is still attempted on each query,
because process ids are reused. The service resolves identity once per app
id, and the UI keeps a missing icon's placeholder for the session, so
nothing about such a window is retried on every event.

Actions: UIPI blocks posted messages to a higher-integrity window, so
`Minimize`, `Restore` and `Close` return `PlatformError::AccessDenied`. The
command layer logs the error and shows it once. `Activate` skips the
restore that would be blocked and calls `SetForegroundWindow`, which is not
a message and normally succeeds (an elevated Task Manager activates). Every
line the platform logs about this is at `debug`.
