# Backdrop research: manual test plan (Windows 11, build 22621 or later)

This plan checks the [assumptions](assumptions.md) and the proposed
prototype of the [backdrop research](index.md) on Windows.
Step 1 tests the [CSS look](css-look.md) and runs on the current build.
Steps 2 to 15 need the prototype ([Remaining work](remaining-work.md)),
which reads `appearance.backdrop` from `settings.json` (edit the file with
Stem stopped) and logs one `info` line naming the backdrop in effect.

1. Set `"backdrop": "css"`. Fill a Notepad window with text and move it
   behind the panel. Expected (A1): the text under the panel is sharp, only
   tinted. Keep a screenshot either way; the name of the built-in choice
   depends on it.
2. Run `winver` and note the build. Stop Stem, set `"backdrop": "acrylic"`,
   start the prototype. Expected (A2, A3): the `info` line names Acrylic; the
   material shows inside the panel only, nothing beside or above it; the
   windows behind the panel look blurred.
3. Click into Notepad, wait 10 s, then drag Notepad behind the panel.
   Expected (A6): the material stays live (the blur follows Notepad) and
   never turns into a flat colour.
4. Move the pointer onto the panel and off it ten times. Expected (A4): the
   material never flickers or disappears when the dock switches between
   clickable and click-through.
5. Click beside the panel, above it, then on its four edges and corners.
   Expected: clicks beside and above reach the desktop; every click on the
   panel reaches the dock; none is lost.
6. Zoom on the corners (Magnifier, Win + Plus) at 100 % and 150 % scale,
   with *Corner radius* at 8, 22 and 30. Expected (A7, A9): no material
   outside the panel's curve, no pixel steps past the hairline, no shadow
   around the material. Keep screenshots.
7. Record the screen at 60 fps (Win + Alt + R) while the pointer enters the
   panel, moves along the row to both ends and leaves. Step through the
   frames. Expected (A8): the material's edge differs from the panel's for
   at most three frames at entry and during the settle, and not at all
   while the pointer moves in the middle of the row.
8. Add and remove a pin; drag an icon to a new place; drag a file from
   Explorer over the dock. Expected: the material follows the panel through
   each animation and settles exactly on it.
9. Set *Auto-hide* to *Always*. Leave the dock and wait for the slide, then
   touch the bottom edge. Expected: the material disappears when the slide
   out starts, nothing stays at the bottom edge while the dock is hidden,
   and the material comes back when the slide in ends. Repeat with
   *Animation effects* off (Settings → Accessibility → Visual effects): the
   same around the fade.
10. Press Ctrl+Alt+D (the toggle shortcut) twice; then play a video in
    fullscreen. Expected: no material without its panel, at any moment.
11. Lock and unlock with a maximised browser in front; open
    Task View; switch virtual desktops (Ctrl + Win + Right) and back.
    Expected (A10): the material is always directly under the panel, never
    above it, never left behind on another desktop.
12. Switch Windows between light and dark; turn *Transparency effects* off;
    turn Battery Saver on; turn a contrast theme on. Expected: the material
    follows light and dark; a solid colour with transparency off and with
    Battery Saver; no material and the opaque panel in a contrast theme.
13. With two monitors and *Monitors* at *All monitors*, repeat step 2 on
    each dock; change one monitor's scale; unplug it. Expected: each dock
    has its own material fitted to its panel, and nothing is left behind.
14. Stop Stem, set `"backdrop": "mica"`, start. Expected: a wallpaper-tinted
    panel that does not show the windows behind it; steps 3 and 4 hold.
15. Leave the dock idle for a minute with Acrylic, then with `css`.
    Expected: Stem's CPU about 0 % in Task Manager both times. Note the GPU
    use of *Desktop Window Manager* both times.

Throughout: one `info` line each time a backdrop is applied or not applied
(unsupported build, contrast theme), and no `warn` or `error` line from the
backdrop window. Record anything that differs.
