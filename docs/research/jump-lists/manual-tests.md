# Manual test plan for jump lists (first stage)

This page is part of the [jump-list study](index.md): the steps that check
the [first stage](recommendation.md) on Windows. Claims marked **A1** and so
on are listed in [Assumptions](assumptions.md).

Run on Windows 11 with a dev build and `set STEM_LOG=debug`. Steps 1 to 12
cover recent entries, launch, budget and privacy.

1. Before starting Stem, open two `.txt` files from Explorer by double-click
   (they open in Notepad). Open one image in Photos. Browse three folders in
   File Explorer.
2. Right-click Notepad's button on the stock taskbar and note its *Recent*
   list. Then right-click Notepad in Stem. Expected: a disabled *Recent*
   heading at the top, the same names in the same order (at most 10), a
   separator, then the usual entries. This checks A1.
3. Choose the first recent entry. Expected: the file opens, the menu
   closes, Notepad's icon bounces and Notepad ends in front. The log shows
   one `open_jump_entry` line and no warning.
4. Repeat steps 2 and 3 for Photos and for File Explorer. Write down what the
   stock list shows that Stem does not (expected: Explorer's *Pinned*).
   If Explorer shows no section at all, A10 is false.
5. Right-click an app without an explicit AppID (for example
   `C:\Windows\regedit.exe` started from Run). Expected: no *Recent*
   section, no extra delay and no warning.
6. Right-click Edge or Chrome. Expected: the current menu, unchanged (tasks
   come in a later stage).
7. Right-click Start, the Recycle Bin and a separator. Expected: unchanged.
8. Settings › Personalization › Start: turn off *Show recently opened items
   in Start, Jump Lists and File Explorer*. Right-click Notepad in Stem.
   Expected: no *Recent* section. Turn it back on, open a file in Notepad,
   right-click again: the file is listed. This checks A9.
9. On Windows Pro, `gpedit.msc` › User Configuration › Administrative
   Templates › Start Menu and Taskbar: enable *Do not keep history of
   recently opened documents*, run `gpupdate`, then right-click Notepad in
   Stem. Expected: no section, and a debug line saying the policy is set.
   Disable the policy again. This checks A8.
10. Open a file from a network share in Notepad, then disconnect the network
    and right-click Notepad in Stem. Expected: the menu appears in about
    150 ms or less, with the last good list or with no section, and a debug
    line reports the timeout. The dock keeps magnifying and stays usable.
    Reconnect and right-click again.
11. Delete a file that is in Notepad's *Recent*, then choose it in Stem.
    Expected: the shell or Notepad reports the missing file. Stem logs one
    warning and keeps running.
12. Read the debug lines for the fetch time of each app in steps 2 to 4,
    cold (the first right-click after start) and warm. Record them; they
    settle A7.
13. Check the menu basics again (outside click, Esc, foreground handed
    back, 150 % scaling). Expected: unchanged. With auto-hide *Always*, open
    an app's menu and move the pointer away. Expected: the dock does not
    hide until the menu closes. This checks A11.
14. Open a file whose name contains `&` and is longer than 60 characters,
    then right-click its app in Stem. Expected: `&` shows literally and the
    name ends in `…`.
