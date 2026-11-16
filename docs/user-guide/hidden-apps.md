# Hidden apps

This page describes how to hide an app from the dock, and the _Hidden apps_
section of the settings window and the `hiddenApps` array of the settings
file that list the hidden apps.

## Hiding an app

Some apps run all the time and need no icon, such as a sync client, or a chat
app you only open from its notifications. Right-click a running app that is
not pinned and choose _Hide from Stem_. Its icon leaves every dock at once.
The icon stays off the dock when you open more of the app's windows, and
after Stem restarts. Nothing else changes for the app. Its windows still
count when Stem checks whether a window covers the dock or an app is
fullscreen. While a hidden app is in front, no icon is marked active.

To show the app again, open Settings →
[_Hidden apps_](#the-hidden-apps-section) and choose _Show_ next to it. A
pinned app cannot be hidden (unpin it first). Pinning a hidden app, for
example by dragging its `.exe` onto the dock, shows it again as a pin.

## The Hidden apps section

The section lists the apps you hid from the dock (see
[Hiding an app](#hiding-an-app)). Each row has the icon and the name the app
had when you hid it, where the app comes from (the program's path, or a
Store app's package), and a _Show_ button. _Show_ puts the app back on the
dock at once. With no app hidden, the section says _No apps are hidden._

In the file, the hidden apps are the [`hiddenApps`](../SETTINGS.md#hiddenapps)
array, empty by default. Each entry has the app's `id`, its `name`, and the
`path` of its program and its `iconKey` as Stem found them when you hid the
app. Either of the last two may be `null` or left out. Without an
`iconKey`, the section shows a plain tile in place of the icon. Without a
`path`, it shows a location read from the id.

```json
{
  "hiddenApps": [
    {
      "id": "exe:c:/program files/spotify/spotify.exe",
      "name": "Spotify",
      "path": "C:\\Program Files\\Spotify\\Spotify.exe",
      "iconKey": "exe:C:\\Program Files\\Spotify\\Spotify.exe"
    }
  ]
}
```

Stem removes an entry whose app is pinned, because a pinned app is always
on the dock. It also removes an entry with a blank id, and an entry with the
same id as an earlier entry.
