# Sizing protocol

This file describes how Rust sizes and places each dock window, and how the page lays the dock out
in it.

`Settings.position` (v1.16) names the **edge**: `bottom` (the default), `top`, `left` or
`right`. A dock on `bottom` or `top` is *horizontal*; on `left` or `right` it is *vertical*. The
window's **length** runs along the edge, and its **thickness** away from it.

1. Rust creates each dock window hidden, then shows it on the edge of its monitor's work area
   (the primary monitor's for `dock`), flush with that edge and **spanning the work area's full
   length along it**: its full width on `bottom` and `top`, its full height on `left` and
   `right`.
2. The UI measures the bounding box of its panel (`ResizeObserver`), adds the headroom for
   magnification and tooltips on the side away from the edge, and calls
   `set_dock_content_size(width, height)` at most once per animation frame, and never while the
   pointer is magnifying the dock.
3. Rust sets the window's thickness from the value across the edge (`height` on a horizontal
   dock, `width` on a vertical one), clamped to the work area, and keeps the window as long as
   the work area. The other value is informational: Rust logs it and uses it as the lower bound
   on hosts without a monitor list. The UI centres the panel along the window itself, so items
   can animate in and out without any window resize.
4. The dock panel is anchored to the window's **edge side**, the side on the edge: its bottom on
   `bottom`, its top on `top`, its left side on `left`, its right side on `right`. The rest of
   the window is transparent and, with the [hit region](hit-region.md), click-through.
5. A change of `Settings.position` moves every dock window to the new edge
   (`Settings::diff_requires_window_rebuild`). Until the page's next `set_dock_content_size`, the
   window keeps its old thickness, now across the new edge. The page lays itself out for the edge
   it reads from `Settings.position` (`get_settings`, `dock://settings`); it takes the edge from
   nothing else.

## Layout on each edge (the page)

Nothing below crosses IPC except through the rects the page already reports. It is written here
so that the app layer and the page use the same positions.

- Items keep their order (`DockState.items`) along the dock's main axis: left to right on a
  horizontal dock, top to bottom on a vertical one. So the launcher is first (left or top) and
  the Recycle Bin last (right or bottom).
- Magnification follows the pointer along the main axis (its `x` on a horizontal dock, its `y`
  on a vertical one), and icons grow away from the edge. Indicator dots are between the icon and
  the edge. Title pills (tooltips), the resting place of toasts and the anchor of the native item
  menu are on the side away from the edge: above the icon on `bottom`, below it on `top`, right
  of it on `left`, left of it on `right`.
- Drag-to-reorder and the file-drag gap work along the main axis. Keyboard navigation uses the
  arrow keys of that axis (the toolbar's `aria-orientation` names it); the arrows across the axis
  move nothing.
- Auto-hide slides the panel towards its edge; the reveal strip lies along the window's edge side
  ([Hit region](hit-region.md)).
