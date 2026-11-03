# Icons

This file describes how the dock's icons are loaded, sized and cached.

The platform returns PNG bytes for an `IconKey` at a requested size. The UI
requests them with `get_icon` and caches them. Keys are namespaced:
`hwnd:<id>:<pid>`, `exe:`, `file:`, `aumid:` and `shell:`. The frontend
draws `shell:*` icons from built-in vector art. The Windows implementation
extracts `HICON`s (from the window, then its class, then the exe, then the
shell item image factory for large and UWP icons) and converts them to an
RGBA PNG with alpha preserved.

**Sources.** The shell item image factory is asked with
`SIIGBF_BIGGERSIZEOK`, so it returns the next larger image that an icon has
(typically 256 px) instead of resampling. Class and window icons come at
whatever size the app registered.

**Size.** The PNG's longer edge is **at most** the requested size. Anything
larger is shrunk by the area-averaging filter in `rules::icons`, which
averages premultiplied colour so alpha edges get no fringes. The platform
never upscales, so an icon whose largest image is smaller than the request
comes back at its own size.

**Window icons.** For `hwnd:` keys the largest of `WM_GETICON` (big and
small) and the class icons is used. When it is smaller than requested and
the exe is known, the exe's icon is used instead if it is larger.

**Logging and the memory cache.** Each real load logs one debug line with
the key, the requested size, the chain that produced the bitmap, and the
source and output dimensions. `exe:`, `file:` and `aumid:` results are
cached in memory per (key, size). The platform does not cache `hwnd:`
results: HWND values are recycled (which is why the key contains the
process id), and a window may change its icon. The UI's per-key cache
covers the session.
There is no disk cache.
