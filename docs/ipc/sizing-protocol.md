# Sizing protocol

This file describes how Rust sizes and places the dock window, and how the page lays the dock out
in it.

1. Rust creates the dock window at 640×96 logical px, hidden, then shows it centred at the bottom
   of the primary monitor's work area.
2. The UI measures the bounding box of its root element (`ResizeObserver`), including the
   headroom for magnification and tooltips, and calls `set_dock_content_size(width, height)` at
   most once per animation frame.
3. Rust resizes the window and centres it again on the work area's bottom edge.
4. The dock panel is anchored to the **bottom** of the window, and the headroom above it is
   transparent. That transparent area takes clicks, so the page keeps it as small as the
   magnification needs.
