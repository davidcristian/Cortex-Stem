# Assumptions of the jump-list study

This page is part of the [jump-list study](index.md): the claims that the
other pages mark **A1**, **A3** and so on.

- **A1**: `IApplicationDocumentLists::GetList` after `SetAppID(aumid)`,
  called from another process, returns that app's Recent list.
- **A3**: Packaged apps' WinRT `JumpList` writes `customDestinations-ms`
  under their AUMID's hash. Inferred from Terminal's and Calculator's files.
- **A4**: The hash upper-cases non-ASCII characters the way Windows'
  invariant upper-casing does. Only ASCII was checked.
- **A5**: Implicit AppIDs replace other known folders (`ProgramFiles`,
  `Windows`, …) the way they replace `System32`. Only `FOLDERID_System` was
  checked.
- **A6**: `ActivateForFile` opens files in packaged desktop apps (Notepad,
  Photos), not only in UWP apps.
- **A7**: 150 ms is enough for a local list. A cold shell (first COM
  activation after start) may be slower.
- **A8**: The policy *Do not keep history of recently opened documents*
  shows up as `SHRestricted(REST_NORECENTDOCSHISTORY)`.
- **A9**: Turning off *Show recently opened items in Start, Jump Lists and
  File Explorer* empties the lists, so Stem needs no check of its own.
  Otherwise Stem must read that setting (registry value `Start_TrackDocs`,
  undocumented).
- **A10**: File Explorer windows have `Microsoft.Windows.Explorer` as their
  window AppID, so Stem gives them an `aumid:` id. If they do not, Explorer is
  an `exe:` app in Stem and gets no section in the first stage.
- **A11**: `popup_at` inside `run_on_main_thread` blocks that closure until
  the menu closes, as it blocks the current synchronous command, so the
  command's promise still covers the menu.
- **A12**: `SEE_MASK_INVOKEIDLIST` with a NULL verb opens an item the way a
  double-click in Explorer does.
- **A13**: `CLSID_ApplicationDocumentLists` works in an STA. Its threading
  model is not documented.
- **A14**: For later stages: the `DestList` pin field at offset 108 means
  the same thing in versions 3 and 4 (libyal marks its later fields
  uncertain).
