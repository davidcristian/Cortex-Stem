# Where the jump-list data lives

This page is part of the [jump-list study](index.md). Windows keeps jump-list
data in three sources, and Explorer also reads them through undocumented COM
interfaces (4). Only the first source is a documented reading API. Claims
marked **A1** and so on are listed in [Assumptions](assumptions.md). Claims
marked *measured* come from the Windows 11 PC described in the
[study's introduction](index.md).

## 1. `IApplicationDocumentLists` (documented)

`CoCreateInstance(CLSID_ApplicationDocumentLists)`, then `SetAppID(aumid)`,
then `GetList(ADLT_RECENT or ADLT_FREQUENT, max, IID_IObjectArray)`. Each
element of the array is an `IShellItem` or an `IShellLink`, or a mix.

- It reads *Recent* and *Frequent* only. The docs say it cannot read
  pinned items, custom categories or tasks.
- Pinned items are still returned, but they are not marked as pinned.
- An app with an explicit AppID must call `SetAppID` before `GetList`. Without
  an explicit AppID the identity is "deduced". When these calls come from
  another process, "the system cannot generate the correct AppUserModelID and
  those calls will have no effect" (AppUserModelIDs). For Stem this means
  `aumid:` apps only. **A1**: `GetList` after `SetAppID`, called from another
  process, returns that app's list. The docs strongly suggest this but do not
  state it.

## 2. `AutomaticDestinations\<hash>.automaticDestinations-ms`

These files are in `%APPDATA%\Microsoft\Windows\Recent\AutomaticDestinations`
and are what `SHAddToRecentDocs` writes. Each file is an OLE compound file
(MS-CFB, documented). Every numbered stream holds one serialised shell link
(MS-SHLLINK, documented). The `DestList` stream holds the order, the pin
state, access counts and times. Its format is undocumented: libyal
reverse-engineered it, and its version changed from 1 (Windows 7) to 3 and 4
(Windows 10).

- It gives pinned, recent and frequent items for any AppID, implicit ones
  included.
- Win32 can read it without a new crate: read the bytes,
  `CreateILockBytesOnHGlobal` + `StgOpenStorageOnILockBytes`,
  `IStorage::OpenStream`, then `IPersistStream::Load` of each numbered stream
  into a `CLSID_ShellLink`. Only the `DestList` parser would be Stem's own
  code.

## 3. `CustomDestinations\<hash>.customDestinations-ms`

These files are in `%APPDATA%\Microsoft\Windows\Recent\CustomDestinations`
and are what `ICustomDestinationList::CommitList` writes. The container is
undocumented (libyal):

- The header holds version `2`, the category count and a reserved word.
- Each category starts with a type: `0` custom (a UTF-16 name, then
  entries), `1` known category (an id: `KDC_FREQUENT` = 1, `KDC_RECENT` = 2),
  or `2` tasks (entries). Each category ends with `0xBABFFBAB`.
- Each entry is a 16-byte CLSID followed by the object's data. For
  `{00021401-0000-0000-C000-000000000046}` (a shell link) the data is an
  MS-SHLLINK blob. The blob has no reliable size field (libyal). MS-SHLLINK
  itself is self-delimiting (sized sections and a terminal extra-data block),
  so a parser can find where the next entry starts.
- *Measured*: Windows PowerShell's file starts `2, 1, 0` (version, one
  category, reserved), then type `2` (tasks) with 3 entries. Next come the
  shell-link CLSID bytes and `4C 00 00 00` (the MS-SHLLINK header size), and
  the file ends with `0xBABFFBAB`.

It gives tasks, custom categories with their names, and which known
category (Recent or Frequent) the app added, all in the app's order. A
separator is a link whose `System.AppUserModel.IsDestListSeparator` is TRUE.
A title is the link's `System.Title`. It can be an indirect string such as
`@shell32.dll,-1324`, which `SHLoadIndirectString` resolves.

No documented API reads this file: `ICustomDestinationList` only writes.
**Never** call `BeginList`, `CommitList`, `DeleteList` or `SetAppID` on
`ICustomDestinationList`, or `IApplicationDestinations::RemoveAllDestinations`,
with another app's AppID. Each of these changes or clears that app's list.

## The file name is a hash of the AppID (*measured*)

The file name is a CRC-64 of the AppID, written as lower-case hex **without
leading zeros**. The algorithm is from Hexacorn and libyal:

```
table[i], i in 0..256: c = i; 8 times: c = (c & 1 != 0) ? (c >> 1) ^ 0x92C6_4265_D321_39A4 : c >> 1
crc = 0xFFFF_FFFF_FFFF_FFFF
for each byte b of UTF-16LE(upper-case(AppID)): crc = table[(crc ^ b) & 0xFF] ^ (crc >> 8)
no final XOR; file name = format!("{crc:x}") + ".automaticDestinations-ms" / ".customDestinations-ms"
```

Hashing these AppIDs gave names that exist on that PC:

| AppID | Hash | File found |
|-------|------|------------|
| `Microsoft.Windows.Explorer` | `f01b4d95cf55d32a` | automatic |
| `Microsoft.WindowsNotepad_8wekyb3d8bbwe!App` | `a61657a5e5dfbdc` (15 digits) | automatic |
| `Microsoft.Windows.Photos_8wekyb3d8bbwe!App` | `a52b0784bd667468` | automatic |
| `Microsoft.Windows.ControlPanel` | `7e4dca80246863e3` | automatic |
| `Microsoft.ScreenSketch_8wekyb3d8bbwe!App` (Snipping Tool) | `1c7a9be1b15a03ba` | both |
| `windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel` (Settings) | `f18460fded109990` | both |
| `308046B0AF4A39CB` (Firefox) | `6824f4a902c78fbd` | both |
| `Chrome` | `5d696d521de238c3` | custom |
| `MSEdge` | `ccba5a5986c77e43` | custom |
| `Microsoft.VisualStudioCode` | `1ced32d74a95c7bc` | custom |
| `Microsoft.WindowsTerminal_8wekyb3d8bbwe!App` | `16f2f0042ddbe0e8` | custom |
| `Microsoft.WindowsCalculator_8wekyb3d8bbwe!App` | `ff99ba2fb2e34b73` | custom |
| `{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe` (implicit) | `590aee7bdd69b59b` | custom |
| `Microsoft.Office.EXCEL.EXE.15` | `b8ab77100df80ab2` | custom, 12 bytes: a header with no categories |

These are good unit-test vectors for a pure hash function. Two more come
from Hexacorn: `MICROSOFT.INTERNETEXPLORER.DEFAULT` gives `28c8b86deab549a1`,
and `{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\notepad.exe` gives
`9b9cdc69c1c24e2b`.

- **Implicit AppIDs.** An exe's implicit AppID is its path with a known
  folder replaced by that folder's `KNOWNFOLDERID` in braces.
  `{1AC14E77-…}` is `FOLDERID_System`. This was confirmed for `System32` only.
  **A5**: which other folders the shell replaces, and in what order, is not
  known. **A4**: how the shell upper-cases characters outside ASCII is not
  known.
- **Other observations.** `*.TMP` and `*.temp` files are next to the custom
  files: writers replace a file through a temporary copy, so a reader must
  take only exact `.customDestinations-ms` names and must allow for a file
  that disappears during the read. Some custom files are 12 bytes long (no
  categories). Settings' 24-byte file holds one known-category record whose
  id is `0xFFFFFFFF`, a value libyal does not list, so libyal's description
  of the format is incomplete.
- **Packaged apps.** Terminal and Calculator have custom files. This
  suggests that the WinRT `JumpList` API writes the same file (**A3**).

## 4. Undocumented COM (rejected)

Explorer reads lists with `IAutomaticDestinationList`
(`CLSID_AutomaticDestinationList` `{f0ae1542-f497-484b-a175-a20db09144ba}`)
and with the reading side of `CLSID_DestinationList` (`IDestinationList`).
Open-Shell's `JumpLists.cpp` calls them with three different IIDs: one for
Windows 7, one for Windows 10 build 10240 and one for build 10547. A wrong
guess about a vtable causes undefined behaviour inside Stem's own process,
not a clean parse error. Rejected.

WinRT `Windows.UI.StartScreen.JumpList.LoadCurrentAsync` reads only the
calling app's own list. Not usable.

## Which apps fill which source

- Apps that open files through Explorer or the common file dialog get a
  *Recent* list without code of their own (the automatic file). Examples
  measured above: Notepad, Photos, Control Panel. A file only shows up in an
  app's list when the app is registered for that file type
  (`SHAddToRecentDocs` remarks).
- Folders appear only in Explorer's list (`SHAddToRecentDocs` remarks).
  Explorer's *Pinned* items are its Quick access pins, which are in the
  automatic file.
- Browsers, Electron apps, Terminal, Windows PowerShell and Calculator write
  custom lists: tasks plus categories of their own (custom files above).
- Snipping Tool, Settings and Firefox write both.
- Office registers AppIDs (`Microsoft.Office.*.EXE.15`), but its custom files
  were empty on that PC.
