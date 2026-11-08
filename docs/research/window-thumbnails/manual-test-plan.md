# Research: manual test plan for window previews

This is the manual test on Windows for the window previews of
[ADR-0009](../../adr/0009-window-thumbnails.md). A1 to A11 name the
assumptions listed in the ADR's
[Assumptions](../../adr/0009-window-thumbnails.md#assumptions).

Setup: a build with window previews. Defaults: click-through on, previews on,
auto-hide off, 100 % scale. Debug log. Open two Notepad windows with
different text, Edge playing a video, Paint (then minimise it), Calculator,
Task Manager run as administrator, and a second virtual desktop with one
more Notepad.

1. Click into the first Notepad and type a few letters. Rest the pointer on
   Notepad's dock icon without clicking. Expected: after about 0.4 s, a
   strip of two cards appears above the icon. Each card shows its window's
   title and current picture, filling the image area with no offset, gap or
   empty border. Notepad's title bar stays active and its caret keeps
   blinking: the dock took no focus. The log shows one thumbnails line with
   two registrations.
2. Move the pointer from the icon onto the right-hand card, once slowly and
   once fast on a diagonal. Expected: the strip stays where it is, the icons
   relax, the card under the pointer is highlighted, and nothing flickers.
3. Rest on Edge's icon, then move to Notepad's. Expected: Edge's card shows
   the video playing. The switch to Notepad's strip happens at once,
   without the 0.4 s wait.
4. Click the second Notepad's card. Expected: that window comes to the
   front and takes the keyboard, and the strip closes. The log shows the
   activation path and no foreground change to Stem.
5. Open Notepad's strip. Click one card's close button, then middle-click
   the other card. Expected: each window is asked to close (choose *Don't
   save*). The strip drops to one card, then closes when none is left.
6. Open a strip, then move the pointer straight up onto the desktop. Do it
   once slowly and once in a fast flick. Expected: the strip closes within
   about 0.25 s and no image stays on screen. A click where the strip was
   selects desktop icons. In the log, the thumbnails are unregistered before
   the `click-through` transition.
7. Rest on Paint's icon (Paint is minimised). Expected: the card shows
   Paint's icon and title and no picture, and nothing is registered for
   Paint. A click restores Paint to the front.
8. Turn *Show only windows on this desktop* off. Rest on Notepad's icon.
   Expected: the card of the Notepad on desktop 2 shows the icon. A click
   switches to desktop 2 with that Notepad in front.
9. Rest on Task Manager's icon. Expected: a live picture of Task Manager. If
   the card stays empty, copy the log line (A8).
10. Open 15 Notepad windows and rest on the icon. Expected: the cards shrink
    to fit the screen's width; past the minimum, a "+N" tile ends the strip.
    Clicking the tile opens the native menu with every window listed.
11. Rest on the leftmost and the rightmost app items. Expected: the strip
    stays fully on screen, shifted inwards, and never covers the hovered
    icon.
12. Set auto-hide to *Always* (250 ms). Open a strip, move onto it, wait 2
    s, then leave. Expected: the dock stays up while the strip is open and
    hides about 250 ms after the strip has closed with the pointer away.
13. With a strip open, right-click the item. Then start a drag of a pinned
    icon. Then drag a file from Explorer over an app item. Expected: the
    strip closes at once each time, and no strip opens during the menu, the
    drag or the file drag.
14. Set the display to 150 %. If a second monitor is available, give it
    another scale and set *Monitors* to *All*. Repeat steps 1 and 3 on each
    dock, including a window that sits on the other monitor. Expected:
    crisp titles, and pictures that fill their slots exactly at each scale.
15. Turn previews off in the settings. Expected: no strip, and the log shows
    a smaller content size (the band is gone). Turn them on again. Then turn
    click-through off. Expected: no strip, and the settings row says
    previews need click-through.
16. Leave the strip closed for one minute. Expected: `stem.exe` uses about
    0 % CPU (Task Manager, Details). Then hold a strip open over the playing
    video for 30 s, and record the CPU of `stem.exe` and `dwm.exe`, closed
    and open.
17. Read the log. Expected: no `DwmRegisterThumbnail` or
    `DwmUpdateThumbnailProperties` failures except for windows named above,
    and no thumbnail pass slower than 16 ms (A6).

Record anything that differs. Above all: Stem taking the foreground, a
picture left on screen after its strip closed, a picture hidden under the
card (only the frame visible, A1), or a picture offset from its slot (A2,
A7).
