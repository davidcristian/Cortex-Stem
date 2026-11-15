# Clicking an app

This page describes the order of the items on the dock, what a click and a
right-click on an app's icon do, and what the dots next to an icon show.

The dock shows these items from left to right (from top to bottom on a side
edge):

1. the fixed items Start, Task View and Show Desktop;
2. the pinned apps, in your order;
3. the other running apps, in the order they appeared;
4. the fixed items Quick Settings, Notification Center, Battery, Clock and
   the Recycle Bin.

Windows of the same app share one icon.

A left click on an app icon does one of these:

- It **launches** the app when the app has no window open and Stem can start
  it (a pinned app, for example). The icon bounces while the app starts.
- It **activates** the app's window when the app is not in front. A
  minimised window is restored first.
- It **minimises** the window when the app has one window and that window is
  already in front. Turn off _Clicking the active app minimises it_ to bring
  the window to the front again instead.
- It **cycles** through the app's windows when the app has several: each
  click brings the next one to the front.

Only windows that the dock shows take part. With _Show only windows on the
current desktop_ on, windows on other virtual desktops are left out.

Windows limits which app may bring a window to the front. Stem tries the
documented ways to bring a window forward. If all of them fail, a short
notice appears on the dock, and the app may only flash. The
[FAQ](faq.md#why-does-a-click-on-an-app-sometimes-only-make-it-flash) has
more.

Dots between an icon and the screen edge show the app's open windows: one
dot per window, at most three. The dots are dimmed when all the windows are
minimised. _Running indicators_ off removes the dots. An app that requests
attention (its button would flash on the stock taskbar) gets an orange pill
in place of its dots, even with _Running indicators_ off. Its icon also
wiggles until you switch to the app. With Windows' _Animation effects_ off,
the icon does not wiggle and only the pill shows the request.

A right-click opens a menu. The menu of an app lists:

- its open windows (choose one to activate it);
- _New window_, for an app Stem can start;
- _Pin to Stem_ or _Unpin from Stem_. _Pin to Stem_ is disabled for an app
  Stem cannot start;
- _Hide from Stem_, for an app that is not pinned (see
  [Hiding an app](hidden-apps.md#hiding-an-app));
- _Close all windows_, while the app has windows on that dock.

Every menu ends with _Stem settings…_ and _Quit Stem_. A right-click on the
panel between icons opens a menu with only those two entries.

If a click fails (the window is gone, or Windows does not allow the action),
a notice appears next to the dock for a few seconds. A menu entry that fails
only writes a warning to the log.
