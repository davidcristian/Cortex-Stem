# Icons

This file describes how the dock's icons are loaded and sized, and the disk cache that keeps
them across sessions.

The platform returns PNG bytes for an `IconKey` at a requested size. The UI
requests them with `get_icon` and caches them. Keys are namespaced:
`hwnd:<id>:<pid>`, `exe:`, `file:`, `aumid:` and `shell:`. The frontend
draws `shell:*` icons from built-in vector art (the clock and battery
faces from live data). The Windows implementation finds a source
image for each key and converts it to an RGBA PNG with alpha preserved.

**PE images.** An `exe:` or `file:` key that names a PE image (`.exe`,
`.dll`, `.scr`) is read from the file's own resources (the `PE resources`
chain). The file is mapped as a data file, and its first `RT_GROUP_ICON` is
parsed in `rules::icons` (pure, tested). The best entry (largest edge up to
256 px, then bit depth, then resource size) is created at its native size
with `CreateIconFromResourceEx`. So the source is the icon's own largest
image, and no OS resampling happens.

**Other sources.** Every other source (`aumid:` logos, shortcuts and
documents, PE files whose resources cannot be read) comes from
`IShellItemImageFactory`, which is always asked for 256 px. The shell
resamples to exactly the size requested, whatever `SIIGBF_BIGGERSIZEOK`
suggests, and its filter leaves a light halo. Requesting the maximum leaves
the downscale to Stem's own filter. An icon whose largest image is smaller
than 256 px is upscaled by the shell and used as returned. `E_PENDING` from
the shell is retried once. `PrivateExtractIconsW` and `SHGetFileInfoW` are
the last fallbacks. Class and window icons come at whatever size the app
registered.

**Size.** The PNG's longer edge is **at most** the requested size. Anything
larger is shrunk by the area-averaging filter in `rules::icons`, which
averages premultiplied colour so alpha edges get no fringes. The platform
never upscales, so an icon whose largest image is smaller than the request
comes back at its own size.

**Window icons.** For `hwnd:` keys the largest of `WM_GETICON` (big and
small) and the class icons is used. When it is smaller than requested and
the exe is known, the exe's icon (through the same `PE resources` chain) is
used instead if it is larger. UIPI blocks `WM_GETICON` to a window of
higher integrity at once, so no timeout is spent, and the trace line says
access denied rather than timed out. So an `hwnd:` key of an elevated or
protected window resolves to the class icon, then to the exe's icon when
the process could be opened. It resolves to `None` when the process could
not be opened and the class has no icon. The UI then shows its neutral
placeholder tile, and keeps it for the session.

**Logging and the memory cache.** Each real load logs one debug line with
the key, the requested size, the chain that produced the bitmap, and the
source and output dimensions. `exe:`, `file:` and `aumid:` results are
cached in memory per (key, size). The platform does not cache `hwnd:`
results: HWND values are recycled (which is why the key contains the
process id), and a window may change its icon. The UI's per-key cache
covers the session.

## Disk cache (`stem_service::icon_cache`)

In front of the platform, `DockService::icon_png` keeps PNGs across sessions
in `<local app data>/dev.davidcristian.stem/icons/`. The app layer passes
the directory. There is none under the mock platform, and without one every
icon comes from the platform.

The cache stores only keys whose bitmap is a pure function of one file:
`exe:` and `file:` keys of an absolute, local path to a PE image. These
always go to the platform:

- `hwnd:` keys (a live window);
- `shell:` keys (the Recycle Bin's states change on their own);
- `aumid:` keys (the key has no version, and the service cannot see the
  package's version);
- `file:` keys of shortcuts and documents (their icon follows a target or a
  file association);
- network paths (`\\server\…`), because reading the length and
  modification time of a file on an offline share blocks for the
  redirector's timeout.

One file per (key, size), named `<FNV-1a 64 hex>.icon`, holds the key, the
size, the id of the Stem build that wrote it, the source file's length and
modification time (read before the load and checked after it), and the
PNG, with a checksum. A read compares all of them with the running build
and with the file as it is now. So a changed exe, an updated package (whose
exe path contains its version) or another build of Stem misses. A missing,
unreadable, corrupt or foreign entry is a miss, never an error; a corrupt
one is deleted.

The build id is the crate version plus the length and modification time of
the running `stem.exe` (`icon_cache::build_id`). The crate version alone
does not change when the icon pipeline in `stem-platform` changes. With the
executable's length and time in the id, every new `stem.exe` (rebuild,
install, update) misses every entry once, instead of serving the bitmaps
of an older pipeline. A development build and the installed one share the
folder and replace each other's entries, which costs one start without
cache hits per switch. A `stem.exe` whose length and modification time
cannot be read runs without the cache.

Each (key, size) goes to the disk at most once per session. Later requests
(a second monitor's dock, a reload) touch no file. They are served from
the bytes of the session's disk hit, or by the platform, whose memory cache
holds what it loaded. Like that memory cache, the disk cache does not see a
file that changes while Stem runs. Its entry keeps the old length and time,
and the next start loads the new icon. Only a session's first platform load
of a (key, size) is written, and only when the file did not change during
the load.

Writes go through the `stem-icon-cache` thread
([Threads](threads.md#the-icon-cache-thread)). They are atomic: a temp file,
then a rename, with no `fsync`. A partly written entry fails its checksum.
Bounds: 4096 files or 64 MiB, least recently used evicted first.
A disk hit refreshes its file's modification time when that time is more
than an hour old, so recency is kept across restarts. PNGs over 1 MiB and
keys over 4 KiB are not cached. At shutdown one `info` line counts the
session's hits, repeats, misses, bypassed requests, writes, failures,
drops, writes abandoned at quit and evictions. The uninstaller's *Delete
the application data* option removes the folder with the rest of
`%LOCALAPPDATA%\dev.davidcristian.stem`.

## Known limitation of the disk cache

The platform does not report which chain produced a bitmap. So for a PE
image whose resources it cannot read (the mapping failed, no icon group, a
malformed directory), the shell fallback's bitmap is cached like a
`PE resources` one: the generic exe icon, the shell's resampling, or the
result of the next fallback after an `E_PENDING`. It stays cached until the
exe changes, another build runs or the entry is evicted. The platform's
debug line per load names the chain, so the log of a start without cache
hits shows which entries these are. Writing only `PE resources` results
needs that information from the platform.
