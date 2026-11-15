# Backdrop research: recommendation

The [backdrop research](index.md) recommends
[option 6](backdrop-window.md): one unowned backdrop window per dock window,
with `DWMSBT_TRANSIENTWINDOW` for *Acrylic* and `DWMSBT_MAINWINDOW` for
*Mica*, shaped by a region at the panel's radius. Ship it only after the
prototype passes steps 2 to 8 of the [manual test plan](manual-test-plan.md)
on Windows. If A6 ([Assumptions](assumptions.md)) fails (the material goes
flat on a window that is never active), stop there. The result would be a
flat panel, which the CSS look already draws. The option is then dropped,
and the two choices are removed from the settings page.

Acrylic is the material to build for. Microsoft recommends it for
transient surfaces that "draw outside the frame of the main app window"
([Acrylic][acrylic]), and it blurs the windows behind it. Mica "only
samples the desktop wallpaper once" ([Mica][mica]), so under a dock it is a
wallpaper-tinted panel. It comes with the same window for one attribute
value.

Either way:

- The CSS look stays the default, and the fallback for builds before 22621,
  contrast themes and any failure of the window. The rule that picks the
  effective backdrop (setting, build, contrast theme) belongs in
  `stem-core` as pure Rust, like other behaviour. In the prototype,
  `effective` in `src-tauri/src/backdrop.rs` applies the setting and the
  contrast theme, and `stem-platform` checks the build.
- Do not use `window-vibrancy`; remove it from `[workspace.dependencies]`.
  Tauri keeps its own copy.
- If test step 1 confirms A1, rename the built-in choice from *Blur (built
  in)* to what it is, a translucent tint, in the settings page, the user
  guide and the README.

[acrylic]: https://learn.microsoft.com/en-us/windows/apps/design/style/acrylic
[mica]: https://learn.microsoft.com/en-us/windows/apps/design/style/mica
