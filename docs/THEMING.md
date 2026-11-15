# Theming the dock

Stem's dock has three built-in looks, the **theme presets**, and applies an
optional stylesheet of your own, **`dock.css`**, on top of them. This page
is the reference for both: what each preset draws, the stable CSS variables
a `dock.css` can set, their defaults, and what the app's content security
policy lets a stylesheet load. The IPC contract for it is
[`docs/ipc/themes.md`](ipc/themes.md) (v1.20).

- [Theme presets](#theme-presets)
- [Your dock.css](#your-dockcss)
- [The stable variables](#the-stable-variables)
- [Selectors you can rely on](#selectors-you-can-rely-on)
- [An example](#an-example)
- [What `url()` can load: the content security policy](#what-url-can-load-the-content-security-policy)
- [For maintainers](#for-maintainers)

## Theme presets

The preset is Settings → Appearance → _Theme_, or `appearance.theme` in the
settings file ([reference](SETTINGS.md#appearance)). A change restyles the
dock at once. No icon moves, because every preset keeps the panel's size.

| Preset               | Panel                                                                                                                                                       |
| -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `standard` (default) | Translucent and blurred, with a fine hairline and a soft shadow. _Panel opacity_ (`appearance.opacity`) sets how translucent it is.                       |
| `solid`              | Opaque, in the colours Windows 11 gives a flyout with _Transparency effects_ off, with no blur, the flyout's stronger stroke as the hairline and a tighter shadow. The title pills are opaque to match. It is easy to read over any wallpaper, and the natural choice with Windows' _Transparency effects_ off. _Panel opacity_ does not apply. |
| `minimal`            | No panel fill, hairline or shadow: only the icons, the running dots and the separators. The separators have more contrast, because they are drawn on the wallpaper. _Panel opacity_ does not apply. |

Independent of the preset:

- **Light or dark** is _Colour mode_ (`theme`: `system` follows Windows,
  or `light`, `dark`). Every preset has a light and a dark version.
- **The running dots** are drawn in the Windows accent colour
  (`ThemeInfo.accent`) in every preset, and so are the Start item and the
  outline of a file drop.
- **_Transparency effects_ off** in Windows makes the `standard` panel at
  least 95 % opaque; `solid` is opaque anyway and `minimal` has nothing
  translucent.
- **A Windows contrast theme** overrides the preset: every preset draws
  the contrast panel (opaque, unblurred, lines and separators in the text
  colour, no shadow), and WebView2's forced-colours mode then repaints it in
  the contrast theme's own colours.

## Your dock.css

`dock.css` is in the same folder as the settings file:
`%APPDATA%\dev.davidcristian.stem\dock.css` for an installed Stem.

- **When it is read.** When Stem starts, and when you press _Reload_ in
  Settings → Appearance. Stem does not watch the file: save it, then press
  _Reload_. Every dock (one per monitor with _All monitors_) gets the new
  text at once.
- **Creating it.** _Edit dock.css_ writes a commented template if there is
  no file yet (never over an existing one). It then opens the file with the
  default app for `.css`. The template lists the variables below with example
  values, all commented out, so it changes nothing until you edit it. If
  Windows has no app for `.css`, the settings page shows the error: open the
  file from its folder (_About_ → _Open folder_ next to the settings file)
  with any text editor.
- **What Stem accepts.** UTF-8 text of at most 64 KiB (65,536 bytes). A
  leading byte order mark, which Notepad may write, is dropped. Stem ignores
  the whole file when it is larger, not UTF-8 (Notepad's _UTF-16_ /
  "Unicode"), or unreadable: the dock uses its own styles only, and Stem's
  log has a `warn` line saying which (`dock.css at … is ignored: …`). A
  missing file is the normal case and logs one `info` line. Under the file's
  path, the settings page shows what the last start or _Reload_ found:
  _In use_ or _Not in use_.
- **How it is applied.** The dock page puts the text in one `<style>`
  element at the end of `<head>`, after its own styles, so a rule of yours
  overrides a rule of the dock's with the same specificity. The page sets the
  element's text, never its HTML: the file is CSS only and nothing in it runs
  as a script. A mistake in one rule makes the browser skip that rule, as CSS
  always does; the rest still applies.
- **What is stable.** The variables below keep their names and meaning
  across versions. Everything else in the dock's page (class names,
  element structure, layout variables such as `--icon` or `--slot-gap`,
  the internal `--preset-*` values) may change with any update, so a rule
  that depends on them can stop working after an update.

## The stable variables

The dock's own stylesheet reads each variable with the preset's value as
the fallback: `background: var(--stem-panel-fill, <the preset's fill>)`.
Stem sets none of them, so a value you set anywhere above the element
that reads it applies, whatever the selector's specificity: `:root`,
`html`, `.dock-stage` or `.dock-panel` all work. To set one only for some
presets or one colour mode, use the selectors of the next section.

Where a cell has two values, the first is dark mode, the second light mode.
`α` is _Panel opacity_ (0.72 by default; at least 0.95 with _Transparency
effects_ off). `accent` is the Windows accent colour, and `radius` is
_Corner radius_ in px (22 by default).

| Variable                | What it styles (value type)                                                     | `standard`                                                                                                                                                                       | `solid`                                                                                   | `minimal`                                      |
| ----------------------- | ------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | ---------------------------------------------- |
| `--stem-panel-fill`     | The panel's background (a `background`: a colour or a gradient)                 | `rgb(28 28 30 / α)`; `rgb(244 244 246 / α)`                                                                                                                                      | `rgb(44 44 44)`; `rgb(249 249 249)`                                                       | `transparent`                                  |
| `--stem-panel-blur`     | What the panel does to the desktop behind it (a `backdrop-filter`, or `none`)   | `blur(24px) saturate(1.4)`                                                                                                                                                       | `none`                                                                                    | `none`                                         |
| `--stem-panel-hairline` | The 1 px line just inside the panel's edge (a colour)                           | `rgba(255, 255, 255, 0.12)`; `rgba(0, 0, 0, 0.1)`                                                                                                                                | `rgb(117 117 117 / 0.4)`                                                                  | `transparent`                                  |
| `--stem-panel-shadow`   | The panel's shadow (a `box-shadow`, or `none`)                                  | `0 12px 32px rgba(0, 0, 0, 0.45), 0 2px 6px rgba(0, 0, 0, 0.3), inset 0 1px 0 rgba(255, 255, 255, 0.06)`; `0 12px 32px rgba(0, 0, 0, 0.22), 0 2px 6px rgba(0, 0, 0, 0.12), inset 0 1px 0 rgba(255, 255, 255, 0.6)` | `0 8px 24px rgb(0 0 0 / 0.42), 0 2px 6px rgb(0 0 0 / 0.28)`; `0 8px 24px rgb(0 0 0 / 0.16), 0 2px 6px rgb(0 0 0 / 0.1)` | `none`                                         |
| `--stem-panel-radius`   | The panel's corner radius (a length)                                            | `radius`                                                                                                                                                                         | `radius`                                                                                  | `radius`                                       |
| `--stem-separator`      | The lines between the groups of items (a colour)                                | `rgba(255, 255, 255, 0.22)`; `rgba(0, 0, 0, 0.18)`                                                                                                                               | `rgb(255 255 255 / 0.28)`; `rgb(0 0 0 / 0.22)`                                            | `rgb(255 255 255 / 0.5)`; `rgb(0 0 0 / 0.4)`   |
| `--stem-dot`            | The running-app dots and the glow of the active app's dots (a colour)           | `accent`                                                                                                                                                                         | `accent`                                                                                  | `accent`                                       |
| `--stem-pill-fill`      | The app-name pills and their arrow (a colour)                                   | `rgba(40, 40, 42, 0.94)`; `rgba(250, 250, 252, 0.96)`                                                                                                                            | `rgb(44 44 44)`; `rgb(249 249 249)`                                                       | as `standard`                                  |
| `--stem-pill-text`      | The app-name pills' text (a colour)                                             | `#ffffff`; `#1b1b1f`                                                                                                                                                             | as `standard`                                                                             | as `standard`                                  |

Notes:

- The dots keep their own opacity (dimmer for an app whose windows are all
  minimised). An app that needs attention keeps its orange pill, whatever
  `--stem-dot` is set to.
- A contrast theme replaces the preset's values (see above) but not yours:
  a variable you set still applies there, where WebView2's forced-colours
  mode repaints most colours anyway.
- `--stem-panel-radius` rounds the CSS panel only. The experimental native
  backdrop (`appearance.backdrop` `acrylic` or `mica` in the settings
  file) uses _Corner radius_; neither the preset nor your stylesheet
  changes it.

## Selectors you can rely on

| Selector                                                         | Matches                                                                                    |
| ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `.dock-stage`                                                    | The dock's root element; the place to set the variables for every preset.                  |
| `.dock-stage[data-theme="standard"]` (`"solid"`, `"minimal"`)    | The dock with that preset (`appearance.theme`).                                            |
| `.dock-stage[data-edge="bottom"]` (`"left"`, `"right"`, `"top"`) | The dock on that screen edge (`position`).                                                 |
| `html[data-theme="dark"]`, `html[data-theme="light"]`            | The page in dark or light mode (_Colour mode_ and Windows). Put it in front of the others. |
| `html[data-high-contrast]`                                       | A Windows contrast theme is on.                                                            |

Two different elements have a `data-theme`: on `<html>` it is light or
dark, on the dock's root it is the preset. The values never overlap, so
`html[data-theme="light"] .dock-stage[data-theme="solid"]` matches "the
Solid theme in light mode".

## An example

```css
/* Every preset: square-ish corners and amber dots. */
.dock-stage {
  --stem-panel-radius: 10px;
  --stem-dot: #ffb900;
}

/* Standard only: a tinted glass. */
.dock-stage[data-theme="standard"] {
  --stem-panel-fill: linear-gradient(135deg, rgb(0 120 212 / 0.45), rgb(136 23 152 / 0.45));
  --stem-panel-hairline: rgb(255 255 255 / 0.3);
}

/* Solid in light mode only: a white panel without a shadow. */
html[data-theme="light"] .dock-stage[data-theme="solid"] {
  --stem-panel-fill: #ffffff;
  --stem-panel-shadow: none;
}

/* Minimal: give the separators a little more weight. */
.dock-stage[data-theme="minimal"] {
  --stem-separator: rgb(255 255 255 / 0.7);
}
```

## What `url()` can load: the content security policy

The dock's page runs under the content security policy that
`src-tauri/tauri.conf.json` sets (`app.security.csp`), which is:

```text
default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; font-src 'self' data:
```

What that means for a `dock.css`:

- **Images** (`background-image`, `border-image`, `mask-image`,
  `list-style-image`, `cursor`, `content: url(…)`) fall under `img-src`:
  `data:` URLs work (`url("data:image/png;base64,…")`,
  `url("data:image/svg+xml,…")`). `blob:` and `'self'` URLs are the app's
  own, and a stylesheet cannot reach them. A file on your disk
  (`file:///C:/…`, a relative path) or a web address (`https://…`) is
  blocked.
- **Fonts** in `@font-face` fall under `font-src`: a `data:` URL works, and
  nothing from the disk or the web loads. A font installed in Windows needs
  no load at all: name it in `font-family`, or use `src: local("…")`.
- **`@import`** falls under `style-src`: it reaches only the app's own
  files, so another stylesheet of yours cannot be imported; put
  everything in `dock.css`.
- **The `<style>` element itself** is allowed by `style-src
  'unsafe-inline'`.
- A blocked load leaves only that one value empty. The browser reports it
  in the dock page's developer console (debug builds), not in Stem's log.

## For maintainers

- The variables are also listed in `STEM_CSS_VARIABLES`
  (`src/lib/dock/user-css.ts`) and in the template (`TEMPLATE`,
  `src-tauri/src/user_css.rs`). Unit tests on both sides fail when one of
  them, this page or `src/dock/Dock.svelte` names a variable the others do
  not. A new stable variable needs all four, and an entry in
  `docs/ipc/changelog.md`.
- The user's `<style>` element works because the dock's HTML has no inline
  `<style>` of its own. Tauri adds a nonce to `style-src` for each inline
  `<style>` in a page it serves, and a policy with a nonce or a hash
  ignores `'unsafe-inline'`, which would block the element. Keep the page's
  own styles in the bundle (Vite's `<link>`). Before an inline `<style>` is
  added to the page, the user's element needs another way to be allowed.
- The preset values are in `src/dock/Dock.svelte` as `--preset-*`
  variables on `.dock-stage`; `standard` uses the colour tokens of
  `src/app.css`.
