# Sources for the jump-list study

This page is part of the [jump-list study](index.md): the documentation and
other sources it relies on.

Microsoft documentation (APIs relied on):

| API / topic | URL |
|---|---|
| Taskbar Extensions (jump lists, destinations, tasks) | https://learn.microsoft.com/en-us/windows/win32/shell/taskbar-extensions |
| Application User Model IDs | https://learn.microsoft.com/en-us/windows/win32/shell/appids |
| `IApplicationDocumentLists` | https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-iapplicationdocumentlists |
| `IApplicationDocumentLists::GetList` | https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-iapplicationdocumentlists-getlist |
| `IApplicationDocumentLists::SetAppID` | https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-iapplicationdocumentlists-setappid |
| `IObjectArray` | https://learn.microsoft.com/en-us/windows/win32/api/objectarray/nn-objectarray-iobjectarray |
| `ICustomDestinationList` | https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-icustomdestinationlist |
| `ICustomDestinationList::AddUserTasks` | https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-icustomdestinationlist-addusertasks |
| `ICustomDestinationList::AppendCategory` | https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-icustomdestinationlist-appendcategory |
| `ICustomDestinationList::AppendKnownCategory` | https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-icustomdestinationlist-appendknowncategory |
| `KNOWNDESTCATEGORY` | https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/ne-shobjidl_core-knowndestcategory |
| `IApplicationDestinations` | https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-iapplicationdestinations |
| `SHAddToRecentDocs` | https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shaddtorecentdocs |
| `System.Title` | https://learn.microsoft.com/en-us/windows/win32/properties/props-system-title |
| `System.AppUserModel.IsDestListSeparator` | https://learn.microsoft.com/en-us/windows/win32/properties/props-system-appusermodel-isdestlistseparator |
| `System.AppUserModel.ID` | https://learn.microsoft.com/en-us/windows/win32/properties/props-system-appusermodel-id |
| `SHLoadIndirectString` | https://learn.microsoft.com/en-us/windows/win32/api/shlwapi/nf-shlwapi-shloadindirectstring |
| `IShellItem::GetDisplayName`, `SIGDN` | https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ishellitem-getdisplayname, https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/ne-shobjidl_core-sigdn |
| `SHCreateItemFromParsingName` | https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-shcreateitemfromparsingname |
| `SHGetIDListFromObject` | https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-shgetidlistfromobject |
| `IShellLinkW` (`GetPath`, `Resolve`) | https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ishelllinkw, https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ishelllinkw-getpath, https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ishelllinkw-resolve |
| `ShellExecuteExW`, `SHELLEXECUTEINFOW` | https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shellexecuteexw, https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ns-shellapi-shellexecuteinfow |
| `SHAssocEnumHandlers` | https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-shassocenumhandlers |
| `IAssocHandler::GetName`, `IAssocHandler::Invoke` | https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-iassochandler-getname, https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-iassochandler-invoke |
| `IApplicationActivationManager::ActivateForFile` | https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-iapplicationactivationmanager-activateforfile |
| `IApplicationActivationManager::ActivateApplication` | https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-iapplicationactivationmanager-activateapplication |
| `SHRestricted`, `RESTRICTIONS` | https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shrestricted, https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/ne-shlobj_core-restrictions |
| `KNOWNFOLDERID` | https://learn.microsoft.com/en-us/windows/win32/shell/knownfolderid |
| `SHCreateMemStream` | https://learn.microsoft.com/en-us/windows/win32/api/shlwapi/nf-shlwapi-shcreatememstream |
| `IPersistStream::Load` | https://learn.microsoft.com/en-us/windows/win32/api/objidl/nf-objidl-ipersiststream-load |
| `CreateILockBytesOnHGlobal`, `StgOpenStorageOnILockBytes` | https://learn.microsoft.com/en-us/windows/win32/api/coml2api/nf-coml2api-createilockbytesonhglobal, https://learn.microsoft.com/en-us/windows/win32/api/coml2api/nf-coml2api-stgopenstorageonilockbytes |
| `CoInitializeEx` | https://learn.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-coinitializeex |
| MS-CFB (compound file format) | https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-cfb/ |
| MS-SHLLINK (shell link format) | https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-shllink/ |
| WinRT `JumpList` | https://learn.microsoft.com/en-us/uwp/api/windows.ui.startscreen.jumplist |

Reverse-engineered and third-party sources (not Microsoft, not guaranteed):

- libyal dtformats, *Jump lists format*:
  https://github.com/libyal/dtformats/blob/main/documentation/Jump%20lists%20format.asciidoc
- Hexacorn, *JumpLists file names and AppID calculator*:
  https://www.hexacorn.com/blog/?p=1859
- Open-Shell, `JumpLists.cpp` (undocumented COM, launching):
  https://github.com/Open-Shell/Open-Shell-Menu/blob/master/Src/StartMenu/StartMenuDLL/JumpLists.cpp
- Files, issue 17488 (undocumented jump-list interfaces):
  https://github.com/files-community/Files/issues/17488

Repository sources: `src-tauri/src/menus.rs`, `src-tauri/src/commands.rs`
(threading notes), `crates/stem-platform/src/lib.rs`,
`crates/stem-platform/src/windows/{launch,identity,util,mod}.rs`,
`crates/stem-service/src/lib.rs`, the architecture pages
[Layers](../../architecture/layers.md), [Threads](../../architecture/threads.md),
[Identity and grouping](../../architecture/identity.md) and
[The dock window](../../architecture/dock-window.md), the IPC contract's
[Context menus](../../ipc/context-menus.md), and
[ADR-0008](../../adr/0008-auto-hide.md).
