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
