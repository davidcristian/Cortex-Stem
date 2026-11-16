# Windows engineering rules

These rules apply to the Win32 code in `crates/stem-platform/src/windows/`
([AGENTS.md §6](../../AGENTS.md#6-windows-engineering-rules-stem-platformsrcwindows)).

- **Threads.** Win32 calls that can block or that need a message loop (`SetWinEventHook`,
  `GetMessageW`, `SHAppBarMessage`, COM with STA) run on a dedicated thread that `stem-platform`
  creates, never on the Tauri main thread and never in a Tauri command handler. A hook callback
  does the minimum (posts to a channel) and returns.
- **Cross-process messages** use `SendMessageTimeoutW` with `SMTO_ABORTIFHUNG` and a timeout of at
  most 200 ms, never `SendMessageW`.
- **COM** is initialised per thread through the `util::ComApartment` guard: `sta()` for shell
  interfaces, `mta()` for interfaces that call back on their own threads (Core Audio). Do not call
  `CoInitializeEx` anywhere else.
- **Handles are closed.** `HICON`, `HBITMAP`, `HDC` and process handles are wrapped in RAII guards;
  no GDI object leaks.
- **UIPI and elevation.** `WM_GETICON` and some queries fail against an elevated window, so every
  query has a documented fallback (exe icon, title, class). Never crash or spin on an
  access-denied window.
- **DPI.** Every rectangle from the platform is in physical pixels, tagged with its monitor's scale
  factor. The Tauri layer converts to CSS pixels, once.
- **Foreground rules.** Windows restricts `SetForegroundWindow`. Implement the fallbacks documented
  in `windows/actions.rs`, and log when activation falls back.
- **No panics.** Platform functions return `PlatformResult`; `expect` and `unwrap` are not allowed
  outside tests.
- **Timeouts and bounds.** Enumeration callbacks, icon extraction and property store reads are
  bounded in time and size, so the dock stays responsive when a target process hangs.
