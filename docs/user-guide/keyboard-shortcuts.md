# Keyboard shortcuts

This page describes Stem's two keyboard shortcuts, the toggle shortcut and
the focus shortcut, and how to write a shortcut.

## The toggle shortcut

The toggle shortcut shows or hides the dock from any app, as a left click on
the tray icon does. The default is `Ctrl+Alt+D`. With auto-hide on and the
dock slid away, one press brings the dock back. With more than one dock, the
shortcut acts on all of them.

A shortcut is written as modifiers first, then one key, joined by `+`. Case
does not matter.

- Modifiers: `Ctrl`, `Alt`, `Shift` and `Super` (the Windows key).
- Keys: `A` to `Z`, `0` to `9`, `F1` to `F24`, `Space`, `Enter`, `Tab`, `Up`,
  `Down`, `Left`, `Right`, `Home`, `End`, `PageUp`, `PageDown`, `Insert`,
  `Delete`, and punctuation such as `,` or `/`.
- Unless the key is `F1` to `F24`, the shortcut needs `Ctrl`, `Alt` or
  `Super`, so that typing text never triggers it. `Shift+A` is not
  accepted. `F9` and `Ctrl+Shift+F12` are.
- Leave the field empty for no shortcut.

In the settings window, the field applies its value when you press Enter or
leave the field. Esc puts back the previous value. A change takes effect at
once, without a restart. Stem keeps a shortcut it cannot register (an unknown
key, or one that another app has already registered) as you wrote it, but no
shortcut is active, and the log has a warning.

## The focus shortcut

The focus shortcut, `Ctrl+Alt+Shift+D` by default, moves the keyboard into
the dock from any app, as `Win+T` does for the Windows taskbar. The dock on
the monitor of the window you were in takes the focus, on the icon of that
window's app (or on the first icon). If that dock was hidden or slid away,
it comes back first. Then:

- the arrow keys along the dock, `Home` and `End` move between icons;
- `Enter` or `Space` opens the focused icon;
- `Shift+F10` or the menu key opens its menu;
- `Esc`, or the shortcut again, takes you back to the window you were in.

Change the shortcut under _Behaviour_ → _Focus the dock_. It has the same
syntax and rules as [the toggle shortcut](#the-toggle-shortcut). Stem does
not register a focus shortcut with the same keys as the toggle shortcut (the
toggle shortcut keeps them), and the log has a warning. An empty field turns
the focus shortcut off.
