# ADR-0001: Rust + Tauri 2 for the application shell

Date: 2026-11-03 · Status: accepted

## Context

The product is a dock for Windows 11 with rich animation (magnification,
bounce), a settings UI, and deep Win32 integration (window enumeration,
hooks, icons, shell APIs). It must idle at about 0 % CPU and start fast.
Rust and Tauri are the chosen stack.

Alternatives considered: WinUI 3 with C# (the best native integration, but
no Rust), Electron (heavy: more than 100 MB of resident memory at idle),
egui or iced (native Rust UI, but CSS-quality animation and blur are costly
to reproduce), raw Win32 with Direct2D (the most control, the slowest to
build).

## Decision

Tauri **2.12** (stable; 3.0 is alpha) with the `windows` crate 0.62 for
Win32 and COM. The webview (WebView2) renders the dock and the settings
window; Rust decides all behaviour. Tauri 3 is considered again once it is
stable.

## Consequences

- WebView2 sets the lowest memory use Stem can have (about 60 to 100 MB);
  acceptable for a shell app.
- Tauri 2 supports transparent, undecorated, always-on-top windows
  directly.
- The dock window is `WS_EX_NOACTIVATE`, and WebView2 has child windows of
  its own. So that a click on the web content does not take the foreground
  either, the platform subclasses the dock window (its window procedure
  returns `MA_NOACTIVATE` for `WM_MOUSEACTIVATE`) and sets
  `WS_EX_NOACTIVATE` on the WebView2 child windows
  (`crates/stem-platform/src/windows/dock_window/subclass.rs` and
  `children.rs`).
- Everything OS-specific is behind a trait (ADR-0003), so most code is
  testable without Windows (ADR-0004).
