# Backdrop research: what the CSS look gives today

In the default `standard` theme preset, the dock's panel has
`background: rgb(var(--panel-tint) / var(--panel-opacity))`
(`appearance.opacity`, 0.72 by default) and
`backdrop-filter: blur(24px) saturate(1.4)` (`--preset-panel-fill` and
`--preset-panel-blur` in `Dock.svelte`). It has the exact shape: any radius,
an anti-aliased edge, the hairline border. It is drawn in the same frame as
the icons, so magnification, item animations, the drop gap and the slide
never lag. It follows reduced motion, *Transparency effects* and contrast
themes. It works on every build and in the mock. It costs nothing at idle.

The CSS look most likely does not blur the desktop. A `backdrop-filter`
samples what the page itself has drawn behind the element. Behind the
panel the page is transparent, and WebView2 has no access to the pixels
DWM composes under the window. So the windows behind the panel stay sharp,
seen through a 72 % tint. This is assumption A1
([Assumptions](assumptions.md)); step 1 of the
[manual test plan](manual-test-plan.md) checks it.
