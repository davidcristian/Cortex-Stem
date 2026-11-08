# Keyboard shortcuts

This page describes Stem's keyboard shortcut, the toggle shortcut, and how
to write a shortcut.

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
