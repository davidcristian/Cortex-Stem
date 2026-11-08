# Icons

This file describes how the dock's icons are loaded, sized and cached.

The platform returns PNG bytes for an `IconKey` at a requested size. The UI
requests them with `get_icon` and caches them. Keys are namespaced:
`hwnd:<id>:<pid>`, `exe:`, `file:`, `aumid:` and `shell:`. The frontend
draws `shell:*` icons from built-in vector art. The Windows implementation
finds a source image for each key and converts it to an RGBA PNG with alpha
preserved.

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
There is no disk cache.
