# Security

This file describes what the webview may load and call, and what Stem's commands accept:

- No remote content. The CSP in `tauri.conf.json` allows only `'self'`,
  `data:` and `blob:` images, inline styles (for transforms), and `data:`
  fonts.
- Capabilities (`src-tauri/capabilities/default.json`, for the windows
  `dock`, `dock-*` and `settings`): `core:default`, `log:default` and
  Stem's own commands. No shell or fs plugin is exposed to the webview.
- Launch targets come from the user's own settings file or from paths the
  user dropped onto the dock. They are passed to `ShellExecuteEx` unchanged,
  which is the same trust level as a desktop shortcut.
- `delete_to_recycle_bin` is the one command that changes the user's files.
  Only dock windows may call it. Rust accepts only plain absolute paths (no
  device or verbatim prefixes, no `..`, wildcards or streams, never a root),
  always recycles rather than deletes (`FOFX_RECYCLEONDELETE`), and keeps
  the shell's confirmation and its warning before a permanent delete
  (`FOF_WANTNUKEWARNING`, no `FOF_NOCONFIRMATION`).
