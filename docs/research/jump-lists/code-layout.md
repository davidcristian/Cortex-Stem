# Where the jump-list code would live

This page is part of the [jump-list study](index.md): the files, types and
threads that the [first stage](recommendation.md) would add, and how they
meet the Windows engineering rules. Claims marked **A7** and so on are listed
in [Assumptions](assumptions.md).

- `crates/stem-core/src/jump_list.rs` (new; public types).
  Data-only types, no OS types, no TS export
  (nothing crosses IPC):

  ```rust
  pub struct JumpList { pub categories: Vec<JumpCategory> }
  pub struct JumpCategory { pub kind: JumpCategoryKind, pub title: Option<String>, pub entries: Vec<JumpEntry> }
  pub enum JumpCategoryKind { Pinned, Recent, Frequent, Custom, Tasks }
  pub enum JumpEntry { Item { title: String, target: JumpTarget }, Separator }
  pub enum JumpTarget { Open { parsing_name: String }, Launch(LaunchTarget) }
  ```

  The module also gets a pure `for_menu(list, limits)`. It caps the counts
  (10 destinations, 10 tasks), drops entries with empty titles, removes
  duplicate targets and collapses separators. All of this is behaviour, so
  it lives in Rust and is unit-tested. `JumpTarget::Open` stores the
  `SIGDN_DESKTOPABSOLUTEPARSING` name, so the platform can rebuild the item
  on any thread (`SHCreateItemFromParsingName`), and the name can be logged
  and tested.
- `crates/stem-platform/src/lib.rs`: two trait methods whose default
  returns `Unsupported("jump lists")`:

  ```rust
  /// The jump list of `app`, or Timeout after `budget`. Never blocks longer.
  fn jump_list(&self, app: &AppId, budget: Duration) -> PlatformResult<JumpList>;
  /// Open one entry; `owner` is the app's launch target, for open-with-owner.
  fn open_jump_entry(&self, owner: Option<&LaunchTarget>, target: &JumpTarget) -> PlatformResult<()>;
  ```

- `crates/stem-platform/src/windows/jump_list.rs` (new) plus one line in the
  module table in `windows/mod.rs`. It runs a long-lived `stem-jump-list`
  thread, started on the first request (as the hit-test thread starts on the
  first `set_dock_hit_region`) and stopped in `stop`.
  - The thread holds `ComApartment::sta()` for its whole life (**A13**). It
    takes requests from a bounded channel (capacity 2) and sends each result
    back through that request's own one-shot channel.
  - `jump_list` uses `try_send`. A full queue means the thread is stuck, so
    the call returns `Timeout` at once and requests do not accumulate. Then
    `recv_timeout(budget)`. A late result is dropped.
  - Per request: `SetAppID`, `GetList` with `cItemsDesired` at the cap,
    `GetDisplayName(SIGDN_NORMALDISPLAY)` for the title,
    `SIGDN_DESKTOPABSOLUTEPARSING` for the target, and for link elements
    `GetPath` / `GetArguments` / `GetWorkingDirectory`. No `Resolve`, no
    existence checks.
  - `open_jump_entry` rebuilds the item, takes its ID list
    (`SHGetIDListFromObject`; freed by a new RAII guard appended to
    `util.rs`) and calls `ShellExecuteExW(SEE_MASK_INVOKEIDLIST |
    SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI)`. `Launch` targets go to the
    existing `launch::launch`.
  - `crates/stem-platform/Cargo.toml` needs the `Win32_UI_Shell_Common`
    feature: in `windows` 0.62.2, `IObjectArray` and `SHGetIDListFromObject`
    are compiled only with it (the crate's source in the Cargo registry
    shows this). The other symbols (`ApplicationDocumentLists`,
    `ADLT_RECENT`, `SHRestricted`, `REST_NORECENTDOCSHISTORY`,
    `SHCreateMemStream`, `PKEY_Title`, …) are available with the features
    already enabled; the compiler names any other missing feature.
  - Later stages add `crates/stem-platform/src/rules/jump_list.rs` (pure):
    the AppID hash, the custom-file framing, then `DestList`.
- `crates/stem-platform/src/mock.rs`: scripted lists per app and a configurable
  delay, so service tests cover the budget and the caps.
- `crates/stem-service/src/lib.rs`:
  - `jump_list(&AppId) -> ServiceResult<JumpList>`. It returns an empty list
    unless the id is `aumid:`. It calls the platform with a 150 ms budget
    (**A7**) and applies `for_menu`. It keeps the last good list per app (at
    most 32 apps) and returns that list when the platform times out.
  - `open_jump_entry(&AppId, &JumpEntry)`. It looks up the owner with
    `launch_target_for` and emits `Bounce`.
- `src-tauri/src/menus.rs`:
  - `show_item_menu` becomes `async`, so its body runs off the main thread.
    It fetches the jump list first, then builds and shows the menu inside
    `app.run_on_main_thread`, and awaits a one-shot channel that receives a
    message after `popup` returns. The promise still resolves when the menu
    has closed, which the auto-hide hold of ADR-0008 relies on (**A11**).
  - A new `MenuAction::Jump(u32)` with the id `ctx:jump:<n>`. The menu's
    entries are kept in app state as `(AppId, Vec<JumpEntry>)` and replaced
    by every item menu. The event handler dispatches through
    `spawn_blocking`, like the other actions. Titles go through `menu_text`.
  - The rejected alternative is to keep the command synchronous and wait up
    to the budget on the main thread: that stops the dock's message loop and
    rendering for that time.
- Docs:
  - [Context menus](../../ipc/context-menus.md) in the IPC contract: the
    menu contents, and a line in its [changelog](../../ipc/changelog.md).
    There is no payload change and no new command.
  - [Threads](../../architecture/threads.md): a new thread row.
    [The dock window](../../architecture/dock-window.md): a short "Jump
    lists" bullet.

## The Windows engineering rules, point by point

How the design meets the
[Windows engineering rules](../../development/windows-rules.md):

- **Threads.** Shell COM runs on the platform's own thread, never on the
  main thread or in a command handler. The command waits on the async
  runtime, not on the main thread.
- **COM.** One STA per thread through `util::ComApartment`. No other init
  calls.
- **Timeouts and bounds.** 150 ms per menu open. A queue of 2; a full queue
  fails the call at once. `cItemsDesired` capped. No `Resolve`. Later file
  reads are capped at 1 MiB per file (the files on the PC described in the
  [study's introduction](index.md) are under 16 KB), and categories and
  entries are capped too.
- **Handles.** COM objects drop through `windows-rs`. Strings go through
  `CoTaskWideString`. ID lists get a new guard.
- **No panics.** Everything returns `PlatformResult`. A failure shows no
  entries and logs one debug line.
- **Manual tests.** Every Win32 behaviour gets a manual test on Windows, and
  a stage is complete only when its tests pass. The first stage's tests are
  the [manual test plan](manual-tests.md).
