# Themes

Settings → [Appearance](settings-window.md#appearance) → _Theme_ sets how the
panel is drawn. A change applies at once and moves no icon.

- _Standard_: a translucent, blurred panel with a thin line around it and a
  soft shadow. _Panel opacity_ sets how much of the desktop shows through.
- _Solid_: an opaque panel without blur, with a stronger line and a tighter
  shadow. It is easy to read over any wallpaper, and it is the one to choose
  with Windows' _Transparency effects_ off.
- _Minimal_: no panel at all, only the icons, the running dots and the
  separators. The invisible panel still belongs to the dock: a right-click
  between or around the icons opens the dock's menu, not the desktop's.

Each theme has a light and a dark version (_Colour mode_). In all three, the
running dots use the Windows accent colour.

**Your own styles.** Stem also reads `dock.css` from the folder of the
settings file (`%APPDATA%\dev.davidcristian.stem\dock.css`).

- _Edit dock.css_ creates the file from a template if it does not exist. It
  then opens the file in the app Windows uses for `.css` files, or in Notepad
  if no app is set. The template lists the variables you can set, all
  commented out: the panel's fill, blur, line, shadow and corners, the
  separators, the dots and the name pills.
- Save the file, then press _Reload_. Stem reads the file only when it starts
  and when you press _Reload_.
- The _Your styles (dock.css)_ row says whether the dock uses the file.
  Stem ignores a file that is larger than 64 KiB or that is not saved as
  UTF-8, and the log has the reason.
- The file is CSS only. It cannot load images or fonts from your disk or the
  web, but a `data:` URL works.

The [theming reference](../THEMING.md) lists every variable with its default in
each theme, the selectors that stay the same between versions, and an
example.
