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

## Display names

A display name comes from the AppsFolder for an AUMID that the shell
resolves. Otherwise it comes from the exe's version resource
(`FileDescription`, then `ProductName`), the shortcut name, the file stem,
the window title and the class name, in that order. Every candidate is
cleaned (`rules::identity::clean_display_name`), so no name ends in `.exe`.
"Application" is the last resort.
