# Pinned apps

This page describes the _Pinned apps_ section of the settings window, the
`pinned` array of the settings file behind it, and how to pin, rearrange and
unpin apps by dragging.

## The Pinned apps section

The section lists the pinned apps in dock order, each with its name and icon
and three buttons: move it up, move it down, and unpin it. To pin an app,
drag its `.exe` or shortcut onto the dock, or right-click its running icon
and choose _Pin to Stem_ (see [Drag and drop](#drag-and-drop)).

In the file, the pinned apps are the [`pinned`](../SETTINGS.md#pinned) array,
from left to right, empty by default. Each entry has three keys:

- `id`: the same id the app's windows get, so a pin and its running windows
  share one icon.
- `label`: an optional name that replaces the app's name.
- `launch`: how Stem starts the app. Its `kind` is `exe` (a path, with
  optional `args` and `workingDir`), `shortcut` (a `.lnk` path), `storeApp`
  (an AppUserModelID) or `uri` (anything Windows can open, such as
  `ms-settings:`).

For example, this is the `pinned` part of a file with Firefox and Calculator
pinned, and Calculator renamed to "Calc":

```json
{
  "pinned": [
    {
      "id": "exe:c:/program files/mozilla firefox/firefox.exe",
      "label": null,
      "launch": {
        "kind": "exe",
        "path": "C:\\Program Files\\Mozilla Firefox\\firefox.exe",
        "args": [],
        "workingDir": null
      }
    },
    {
      "id": "aumid:Microsoft.WindowsCalculator_8wekyb3d8bbwe!App",
      "label": "Calc",
      "launch": {
        "kind": "storeApp",
        "appUserModelId": "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"
      }
    }
  ]
}
```

Pinning from the dock writes these entries for you, so you rarely need to
edit them by hand. Stem removes an entry with a blank id, and an entry with
the same id as an earlier entry.

## Drag and drop

To rearrange the pinned apps:

- Press a pinned icon and move it a few pixels to lift it. A press that does
  not move is a click, however long you hold it. The other icons make room
  as you move the icon.
- Release it among the pinned apps to drop it there, or near them: up to
  about three icon sizes towards or away from the screen edge, or about one
  icon beyond the first or the last pinned app. The new order is saved.
- Release it farther away, or press Esc, to put it back. Dragging an icon off
  the dock does not unpin it.
- Fixed items, and running apps that are not pinned, cannot be dragged.

To pin by dropping files from File Explorer or the desktop:

- Drop an `.exe` or a shortcut (`.lnk`) on the panel to pin it. While you
  drag, a gap opens where the new icon will go. Several files are pinned in
  the order they were dragged.
- A shortcut is pinned as the app it starts, so the app's running windows
  share its icon.
- An app that is already pinned moves to the drop position.
- Other files (documents, folders, `.url` links) are not pinned. A drag with
  nothing that can be pinned opens no gap.

To unpin, right-click the icon and choose _Unpin from Stem_, or use the
[_Pinned apps_ section](#the-pinned-apps-section) of the settings window.
