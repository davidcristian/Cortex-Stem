# Research: Win32 APIs behind window previews

This table lists every Win32 API that the window previews of
[ADR-0009](../../adr/0009-window-thumbnails.md) rely on, with its
documentation. The options it names are in [options.md](options.md).

| API | Used for | Documentation |
|-----|----------|---------------|
| `DwmRegisterThumbnail` | link a source window to the dock window | [reg] |
| `DwmUpdateThumbnailProperties` | place, crop, show, dim a thumbnail | [upd] |
| `DwmQueryThumbnailSourceSize` | source size for the fit | [qsz] |
| `DwmUnregisterThumbnail` | end a link | [unreg] |
| `DWM_THUMBNAIL_PROPERTIES`, `DWM_TNP_*` | the properties and their flags | [props], [tnp] |
| DWM thumbnail overview | how the link and rendering work | [ovw] |
| `DwmGetWindowAttribute` (`DWMWA_EXTENDED_FRAME_BOUNDS`) | crop the invisible borders | [dga] |
| `DWMWINDOWATTRIBUTE` (`DWMWA_CLOAK`, `DWMWA_FORCE_ICONIC_REPRESENTATION`) | cloaked and iconic behaviour | [attrs] |
| `GetWindowRect` | the window rectangle for the crop | [gwr] |
| `WS_EX_NOACTIVATE`, `WS_EX_LAYERED`, `WS_EX_TRANSPARENT`, `WS_EX_TOOLWINDOW` | the dock window's styles | [exstyles] |
| `WM_MOUSEACTIVATE` / `MA_NOACTIVATE` | clicks do not activate the dock | [mact] |
| `ShowWindow` (`SW_SHOW`, `SW_SHOWNOACTIVATE`) | why option 2b needs care | [show] |
| `SetWindowPos` (`SWP_NOACTIVATE`) | showing without activation (option 2b) | [swp] |
| `PostMessageW` | the wake-up message to the dock window | [post] |
| `TRACKMOUSEEVENT` (remarks) | the 400 ms default hover time | [tme] |
| `SetWindowDisplayAffinity` | protected windows | [wda] |
| `PrintWindow` | rejected capture (option 4) | [print] |
| `GraphicsCaptureSession.IsBorderRequired` | rejected capture (option 4) | [wgc] |

[ovw]: https://learn.microsoft.com/en-us/windows/win32/dwm/thumbnail-ovw
[reg]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmregisterthumbnail
[upd]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmupdatethumbnailproperties
[qsz]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmquerythumbnailsourcesize
[unreg]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmunregisterthumbnail
[props]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ns-dwmapi-dwm_thumbnail_properties
[tnp]: https://learn.microsoft.com/en-us/windows/win32/dwm/dwm-tnp-constants
[dga]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmgetwindowattribute
[attrs]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute
[gwr]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowrect
[exstyles]: https://learn.microsoft.com/en-us/windows/win32/winmsg/extended-window-styles
[mact]: https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mouseactivate
[show]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow
[swp]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos
[post]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-postmessagew
[tme]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-trackmouseevent
[wda]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity
[print]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-printwindow
[wgc]: https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.isborderrequired
