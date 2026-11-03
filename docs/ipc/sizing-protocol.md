# Sizing protocol

This file describes how Rust sizes and places the dock window, and how the page lays the dock out
in it.

1. Rust creates the dock window hidden, then shows it at the bottom of the primary monitor's work
   area, **spanning the work area's full width**.
2. The UI measures the bounding box of its panel (`ResizeObserver`), adds the headroom for
   magnification and tooltips, and calls `set_dock_content_size(width, height)` at most once per
   animation frame, and never while the pointer is magnifying the dock.
3. Rust sets the window's height from `height`, clamped to the work area, and keeps the window as
   wide as the work area. `width` is informational: Rust logs it and uses it as the lower bound
   on hosts without a monitor list. The UI centres the panel along the window itself, so items
   can animate in and out without any window resize.
4. The dock panel is anchored to the **bottom** of the window. The rest of the window is
   transparent and, with the [hit region](hit-region.md), click-through.
