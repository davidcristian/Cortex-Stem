# Icons

This file describes how the dock's icons are loaded and cached.

The platform returns PNG bytes for an `IconKey` at a requested size. The UI
requests them with `get_icon` and caches them. Keys are namespaced:
`hwnd:`, `exe:`, `file:`, `aumid:` and `shell:`. The frontend
draws `shell:*` icons from built-in vector art. The Windows implementation
extracts `HICON`s (from the window, then its class, then the exe, then the
shell item image factory for large and UWP icons) and converts them to an
RGBA PNG with alpha preserved.

**The memory cache.** Results are cached in memory per (key, size). There
is no disk cache.
