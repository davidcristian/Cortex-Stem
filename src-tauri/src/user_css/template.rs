//! The template `open_user_css` writes when there is no `dock.css` yet.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write as _};
use std::path::Path;

/// What `open_user_css` writes when there is no `dock.css` yet: every line
/// commented out, so creating it changes nothing until the user edits it.
/// Lists the stable variables (`docs/THEMING.md`, with the defaults of
/// every preset); the values shown are the `standard` preset's in dark
/// mode.
pub const TEMPLATE: &str = r#"/* dock.css: your own styles for Stem's dock.
 *
 * Stem reads this file when it starts and when you press Reload in
 * Settings > Appearance; it does not watch it. Save it as UTF-8 and keep it
 * under 64 KiB, or Stem ignores it (its log says why). The dock applies it
 * after its own styles, as CSS only: nothing in it runs as a script, and
 * url() loads nothing from your disk or the web (data: URLs work).
 *
 * The variables below are the stable way in: they keep working across
 * versions. Anything else you style here (class names, layout) may change
 * with any update. Set them on .dock-stage for every theme preset, or on
 * .dock-stage[data-theme="standard"], [data-theme="solid"] or
 * [data-theme="minimal"] for one of them; put html[data-theme="light"] or
 * html[data-theme="dark"] in front to narrow a rule to light or dark mode.
 * docs/THEMING.md in Stem's repository lists every default.
 *
 * Remove the comment marks around a line to use it.
 */

.dock-stage {
  /* The panel's background: a colour or a gradient. */
  /* --stem-panel-fill: rgb(28 28 30 / 0.72); */

  /* What the panel does to the desktop behind it (a backdrop-filter, or none). */
  /* --stem-panel-blur: blur(24px) saturate(1.4); */

  /* The 1 px line around the panel (a colour). */
  /* --stem-panel-hairline: rgb(255 255 255 / 0.12); */

  /* The panel's shadow (a box-shadow, or none). */
  /* --stem-panel-shadow: 0 12px 32px rgb(0 0 0 / 0.45); */

  /* The panel's corner radius (a length; Settings > Corner radius by default). */
  /* --stem-panel-radius: 22px; */

  /* The lines between the groups of items (a colour). */
  /* --stem-separator: rgb(255 255 255 / 0.22); */

  /* The running-app dots (a colour; the Windows accent colour by default). */
  /* --stem-dot: #0078d4; */

  /* The app-name pills: background and text (colours). */
  /* --stem-pill-fill: rgb(40 40 42 / 0.94); */
  /* --stem-pill-text: #ffffff; */
}

/* For example, a white panel for the Solid theme in light mode only:

html[data-theme="light"] .dock-stage[data-theme="solid"] {
  --stem-panel-fill: #ffffff;
}
*/
"#;

/// Write [`TEMPLATE`] to `path` unless a file is already there (it is never
/// overwritten). Returns whether it wrote one. Blocking (disk).
pub fn write_template_if_missing(path: &Path) -> std::io::Result<bool> {
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::AlreadyExists => return Ok(false),
        Err(e) => return Err(e),
    };
    file.write_all(TEMPLATE.as_bytes())?;
    file.sync_all()?;
    Ok(true)
}
