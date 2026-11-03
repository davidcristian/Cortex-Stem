# Identity and grouping

This file describes how Stem decides which app a window or a pin belongs to.

`AppId` is the grouping key (`stem_core::app_id_for`):

1. `aumid:<AppUserModelID>` when the window or its process declares an
   AppUserModelID (AUMID): Store apps, browsers with profiles and apps that
   set one.
2. Else `exe:<normalised path>` (lower-case, forward slashes).
3. Else `class:<window class>`.

Pinned entries store the same id, so a pinned app and its running windows
merge into one item. `identify_target` must return the id that the running
windows will have: it resolves a `.lnk` to its target exe, and a Store app
to its AUMID.
