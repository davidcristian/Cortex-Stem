# Themes (v1.20)

This file describes the theme presets and the user's stylesheet, `dock.css`.

- `Settings.appearance.theme: ThemePreset` is one of `standard` (the default look), `solid` (an
  opaque panel without translucency and with a stronger hairline border, for readability and for
  Windows' *Transparency effects* off) or `minimal` (no panel fill and no hairline: only the
  icons, dots and separators). The dock page applies it as `data-theme` on its root element. The
  settings page offers the preset as a select.
- **The user's stylesheet.** `dock.css` is in the settings file's folder
  (`%APPDATA%\dev.davidcristian.stem\dock.css` for an installed Stem). Rust reads it at start and
  on `reload_user_css`, never on its own (there is no file watching). The result is
  `UserCss { path: string, css: string | null }`. `css` is `null` when the file does not exist,
  cannot be read, is not UTF-8 or is larger than 64 KiB, and a `warn` line in the log says which.

  `open_user_css` writes a commented template that lists the supported variables if the file
  does not exist, then opens the file with the default app for `.css`.
- The dock page puts `css` in one `<style>` element after its own styles, so the file can
  override them, and replaces it on `dock://user-css`. `docs/THEMING.md` lists the supported
  variables and their defaults per preset. That list is stable; anything else in the file may
  stop working in a later version. The file is CSS only: the page never evaluates it as script,
  and `url()` loads are subject to the app's content security policy (`docs/THEMING.md` says what
  that allows).
- The settings page shows the file's path with *Edit dock.css* (`open_user_css`) and *Reload*
  (`reload_user_css`) buttons.
- `MockBackend` takes `MockOptions.userCss`. `reload_user_css` emits it.
