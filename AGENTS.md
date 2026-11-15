# Stem engineering rules

Every agent and every person working in this repository follows these rules. A change that breaks
one of them is **not done**, even when it works. Most checks run on Linux, where the Windows binary
cannot run ([ADR-0004](docs/adr/0004-linux-first-verification.md)), so the rules on boundaries,
checks and accurate reports are strict. If a task brief conflicts with this file, this file wins;
if you need an exception, say so in your report. The detail is in `docs/`: the design in
[docs/architecture/](docs/architecture/index.md), the Rust and frontend contract in
[docs/ipc/](docs/ipc/index.md), the decisions in [docs/adr/](docs/adr/), and the detail behind
these rules in [docs/development/](docs/development/index.md).

## 1. What we are building

**Stem** is a floating dock for Windows 11, written in Rust with a Tauri 2 shell and a Svelte 5
frontend. It runs only on Windows; macOS and Linux hosts are used only for development and checks.

- A **floating, centred, rounded panel** on one screen edge (the `position` setting: `bottom` by
  default, `left`, `right` or `top`), not an edge-to-edge bar. Icons **magnify** near the cursor,
  running apps show a **dot indicator**, and launching an app **bounces** its icon.
- It shows **pinned apps** and **running windows**, grouped per application. You can activate,
  minimise and close windows, launch apps, pin and unpin them, and reorder them by drag.
- Fixed items, each one turned on or off by a setting: at the left a launcher (Start, on by
  default), Task View and Show Desktop; at the right a Recycle Bin (on by default).
- It can **auto-hide**, follows the Windows **accent colour and light or dark theme**, supports
  **multiple monitors** and **per-monitor DPI**, and can hide the stock taskbar.
- It is **not** a notification-area (system tray) replacement; that is not a goal for now
  ([docs/research/notification-area/](docs/research/notification-area/index.md) says why).

## 2. Repository map and ownership

The [repository map](docs/development/repository-map.md) shows what each directory and crate holds.

**Ownership rule:** a task brief names the files and directories a worker owns; edit only those.
If you need a change elsewhere (a new type in `stem-core`, a feature in a `Cargo.toml` you do not
own, a new IPC command), stop and put the exact diff in your report under "Requested changes
outside my scope". The orchestrator applies it. Never work around a missing upstream change by
duplicating types or copying logic.

## 3. Operating model: orchestrator and workers

- The **orchestrator** holds the plan, owns the shared contracts (`stem-core` public types, the
  IPC contract in `docs/ipc/`, the workspace dependencies in `Cargo.toml`), writes task briefs,
  merges work and runs the final checks.
- A **worker** receives one brief, works in its own git worktree and branch when tasks run in
  parallel, and returns a report. It starts no other worker unless the brief says so. Tasks that
  run in parallel own disjoint files; the orchestrator makes cross-cutting changes while no
  parallel task runs.

What a **brief** and a **report** contain is in
[docs/development/briefs-and-reports.md](docs/development/briefs-and-reports.md). A report that
says "all tests pass" when a command was not run is the single worst failure in this project.

## 4. Hard boundaries (MUST / MUST NOT)

Code structure:

- `unsafe` is allowed **only** under `crates/stem-platform/src/windows/`, and every `unsafe` block
  has a `// SAFETY:` comment (the `clippy::undocumented_unsafe_blocks` lint fails one without).
- No Win32 or `windows` crate type appears in a public signature outside `stem-platform`; the crate
  boundary uses only `stem-core` types. `stem-core` depends on nothing platform-specific and never
  on `tauri`.
- Frontend code never calls `invoke` or `listen` directly. It goes through the `Backend` interface
  in `src/lib/ipc/`, so the mock backend stays complete.
- Behaviour decisions (what a click does, how items are ordered, what counts as a taskbar window)
  live in Rust: `stem-core` where pure, `stem-platform` where OS-specific. The frontend renders
  state and reports intent.

Process:

- Do not change the IPC contract (`docs/ipc/`) or `stem-core` public types in a worker task unless
  the brief grants it, and do not add a dependency that is not in `[workspace.dependencies]`.
  Request the change.
- Do not disable, weaken, `#[allow]` or skip a lint or a test to make the checks pass. Fix the
  cause or report it.
- Leave no `todo!()`, `unimplemented!()` or stub that returns success without doing the work in a
  code path the app runs. A stub returns `PlatformError::Unsupported("<feature>")` and is named in
  the report.
- Do not guess API names, crate features or Win32 constants; prove them with the compiler:
  `cargo check --target x86_64-pc-windows-msvc` must pass. When unsure which `windows` feature
  enables a symbol, read the crate's source in `~/.cargo/registry`.
- Do not widen scope (put anything else you notice under follow-ups), and do not rewrite files you
  do not own "because it was quicker". Never commit secrets, local paths or machine-specific
  configuration.

## 5. Verification ladder

The verification ladder is the ordered list of checks, rungs 1 to 9, from `cargo fmt` to a person
running the manual tests on Windows.
[docs/development/verification-ladder.md](docs/development/verification-ladder.md) gives each
rung's command and what it proves, the scripts that run the rungs and the Rust toolchain.

- Run the rungs in order; a task is done only when every rung its brief lists passes. Rungs 1 to 7
  are required for every task that touches the matching area.
- Rung 4 cannot prove runtime behaviour; anything it cannot prove gets a manual test (rung 9, §11).
- **A markdown file is at most 250 lines.** `scripts/linecap.mjs` checks every tracked markdown
  file in rung 5 (`pnpm lint`). Split by responsibility as you write: a file over the limit covers
  more than one topic, so give each topic its own file and link to it.
- **Look at your screenshots.** When a UI task produces screenshots, open each image (the `Read`
  tool shows images) and compare it with the brief. "It rendered" is not "it looks right".

## 6. Windows engineering rules (stem-platform/src/windows)

Code under `crates/stem-platform/src/windows/` follows the
[Windows engineering rules](docs/development/windows-rules.md) on threads, cross-process messages,
COM, handles, UIPI and elevation, DPI, foreground activation, panics, and timeouts and bounds.
Every behaviour that depends on Win32 gets a manual test on Windows (§11).

## 7. Frontend rules (src/)

Code in `src/` follows the [frontend rules](docs/development/frontend-rules.md) on Svelte 5 runes,
TypeScript and `any`, backend access, the generated IPC types, animations and idle CPU, keyboard
access, and the dev server, package manager and script names.

## 8. IPC contract change control

[docs/ipc/](docs/ipc/index.md) is the contract between `src-tauri` and `src/`. To change it:

1. Propose the change (a new command or event, a changed payload) in your report or brief. Tasks
   the orchestrator runs may edit it directly.
2. Update `docs/ipc/` **and** the Rust types in `stem-core` **and** regenerate the TS bindings
   (`cargo test -p stem-core --features ts`) **and** update both `TauriBackend` and `MockBackend`,
   in the same change.
3. Raise the contract version at the top of `docs/ipc/index.md` and log the change in
   [docs/ipc/changelog.md](docs/ipc/changelog.md).

## 9. Commits and branches

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/). No
hook checks them, so check your message against these rules before you commit.

- **The subject says what changed; the body says why it was needed.** The subject starts with a
  verb of change (`add`, `fix`, `remove`, `split`, `rename`, `document`), names what changed and
  reads correctly to someone who has not seen the diff.
- Format: `type(scope): subject`, imperative mood, lowercase subject, no trailing period, at most
  72 characters. Types: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`, `chore`,
  `revert`; a breaking change adds `!` after the type or scope and a `BREAKING CHANGE:` footer.
- Scopes are optional and used only when the change stays inside one area: `core`, `platform` and
  `service` (the crates), `app` (`src-tauri/`), `ui` (`src/`), `e2e`, `ci` (`.github/`), `scripts`
  and `docs`. Never repeat the type as its own scope (`docs(docs)`).
- **The body is at most 50 words**, wrapped at 72 columns: the problem, what the change does about
  it, and anything left for a manual test on Windows. Write no body when the subject says it all.
  A paste (a line in a code fence, or one whose first token is a bare `$` prompt) is exempt from
  the wrap, the word count and the dash rule.
- **No volatile references**, so the message still reads correctly after the docs change: no ADR
  number, commit hash, section number (`§5`), contract version or other number that points into a
  document that changes, in a paste either. Name the behaviour, the decision or the file instead.
- **No banned word and no dash as punctuation** (§15); hyphenated words and CLI flags (`--locked`)
  are fine. One logical change per commit.

Workers running in parallel commit on their own branch (`task/short-name`); the orchestrator merges
it into the feature branch. Never force-push a shared branch. Never rewrite someone else's commits.

## 10. Decisions

Architecture decisions are recorded in `docs/adr/NNNN-title.md` (context, decision, consequences).
Before deviating from an ADR, write a new ADR that supersedes it. Workers propose; the orchestrator
accepts.

## 11. Manual tests on Windows

Work that only a person on Windows can confirm gets a manual test procedure: the exact claim,
numbered steps with the expected result, and a risk rating. Adding code that touches Win32 without
adding or updating its manual test is a task failure. Tracked files state what the code does and
never say whether a manual test has run.

## 12. Dependency policy

Adding a Rust crate, a `windows` crate feature or a frontend package follows the
[dependency policy](docs/development/dependency-policy.md).

## 13. Definition of done (per task)

1. The acceptance criteria in the task's brief are met.
2. Every verification rung in the brief passes, and the report shows it.
3. No new warnings. No skipped or weakened tests.
4. Behaviour only Windows can show has a manual test (§11).
5. Docs are updated where behaviour changed (`docs/architecture/`, `docs/ipc/`, an ADR).
6. The report is delivered in the form §3 describes.

## 14. Known agent failure modes to avoid

The [failure modes list](docs/development/failure-modes.md) names the mistakes to avoid, from
invented `windows` crate paths and Svelte 4 syntax to Windows paths in tests on Linux and a
`CARGO_TARGET_DIR` shared between worktrees. Read it before you start a task.

## 15. Prose

Comments, doc comments, docs and commit messages are plain English that a new reader understands
on the first read.

- **Comments** state what the code does not show: why a workaround exists, a constraint that is
  not visible (an ordering, a Win32 rule, a measured number), or a directive a tool reads
  (`// SAFETY:`, an `eslint-disable` with its reason). Delete a comment that repeats the code,
  tells history ("was X, now Y") or explains what a better name would say. A module doc (`//!`)
  states the module's contract: its threads, bounds and fallbacks.
- **Docs** are for a reader who has never seen this repo: say what a thing is and does, then why,
  when the why is not obvious. Keep facts, numbers, commands, file paths and decisions. Cut the
  story of how the code got here, repeated explanations (link to the one place instead), hedging,
  and personal or process text such as dates of work. State a behaviour only a manual test on
  Windows can confirm as what the code does; write an open assumption as "Assumes ...".
- **Plain words.** Short sentences, one idea each, subject first, with the literal verb: "checks",
  "fails", "returns", "stores", "sends", "is set to". Code does not know, notice, want, refuse,
  believe, agree, argue, answer, promise or own anything, so say what it does. Define a technical
  term once, where it first appears. No metaphors, idioms, wordplay or invented terms.
- **No dash as punctuation**: no em dash, no en dash, no spaced `--`. Use a colon, comma,
  parentheses or a full stop, or restructure. Hyphenated words, CLI flags and ranges written as
  words ("1 to 3") are fine. An empty table cell is "none" or blank.
- **Measurements.** A figure that describes only the machine it was taken on names that machine
  ("on one Windows 11 PC at 100 % scale") or is a ratio of its own numbers. Keep a figure absolute
  when a reader compares it with their own setup.

**Banned words.** The words in the left column of the table in
[docs/development/banned-words.md](docs/development/banned-words.md) may not appear in prose, in
any capitalisation, also when a line break splits a phrase. The table is a minimum: rewrite any
other figurative word the same way. Stem's product words *pin*, *pins*, *pinned*, *pinning* and
*unpin* (keeping an app on the dock) are allowed in that sense only; for other senses write "fix"
or "set exactly". `scripts/prosecheck.mjs` reports a banned word or a dash used as punctuation in
every tracked markdown file, outside code, link targets, HTML tags and URLs. Anything in backticks
is exempt.

## 16. Working agreement

- **Vertical slices, not horizontal layers.** Each increment is a thin end-to-end path through the
  layers it needs: small, passing every check and documented. No scaffolding of empty layers.
- **Decisions are written down.** A non-obvious architectural choice becomes an ADR (§10). When a
  requirement is underspecified, write your reading of it in your report as an assumption and
  continue rather than wait; name the riskiest assumptions.
- **Names are designed, never defaulted.** A name that users or saved files depend on (a setting
  key, a theme preset, an IPC command or event) is plain, consistent with its siblings (the presets
  are `standard`, `solid` and `minimal`) and collides with no existing name. Propose one with real
  alternatives before choosing. Renaming a settings key later needs a step in `Settings::migrate`.
- **Claims come with evidence.** Never report a check as passing without having run it in this
  session; show the command and its output. Label an unchecked statement as an assumption.
- **Prove a check can fail.** A check that cannot fail is a defect. After writing or changing a
  test, lint or script, break what it checks and watch it fail; put that proof in the commit body,
  inside a code fence.
- **Read before you write.** Open the file and its call sites before editing. Never edit from
  memory and never invent an API; check the signature.
- **Report faithfully.** Report a failing test with its output and name every step you skipped. An
  inaccurate "done" costs more to undo than an accurate "not yet".
- **Stop when surprised.** When output contradicts your model of the system, work it out again
  from the evidence. Do not assume it is the nearest familiar failure and continue.
- Keep this file and every doc short, with links to the detail instead of copies of it; extra
  context is a defect.
