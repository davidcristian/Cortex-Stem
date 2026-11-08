# Research: window previews

This folder holds the detail behind
[ADR-0009](../../adr/0009-window-thumbnails.md), which proposes drawing
live DWM thumbnails of an app's windows into the dock window when the
pointer rests on the app's icon. The ADR states the decision, its context,
its consequences and the assumptions A1 to A11. The files here compare the
options and describe the design of the recommended one.

## Contents

- [options.md](options.md): options 1 to 4, with the case for and against
  each.
- [strip-behaviour.md](strip-behaviour.md): when the preview strip opens
  and closes, its hit region, its cards for several, minimised and cloaked
  windows, and its auto-hide hold.
- [per-monitor-dpi.md](per-monitor-dpi.md): how the slots and images are
  converted, fitted and cropped at each scale.
- [cost.md](cost.md): what previews cost in CPU, GPU and memory.
- [manual-test-plan.md](manual-test-plan.md): the manual test on Windows
  for the design and for the assumptions the probe does not settle.
- [work-items.md](work-items.md): the pieces of work that build window
  previews.
- [win32-apis.md](win32-apis.md): every Win32 API the design relies on,
  with its documentation.
