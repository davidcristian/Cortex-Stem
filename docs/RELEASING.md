# Releasing Stem

A release is a Git tag `v<version>` on `master`. Pushing the tag runs
[`.github/workflows/release.yml`](../.github/workflows/release.yml). The
workflow builds the NSIS installer on `windows-latest` and attaches it to a
**draft** GitHub release. It never publishes the release: the maintainer
checks the draft and publishes it by hand. Only the maintainer decides when
to push a release tag.

## Steps

1. **Set the same version in three places** (here `0.2.0`):
   - `src-tauri/tauri.conf.json`: `"version"`. It is part of the installer's
     name and is the version Windows shows under Installed apps.
   - `Cargo.toml`: `version` under `[workspace.package]`. Every crate
     inherits it, and Settings → About shows the app crate's version.
   - `package.json`: `"version"`.

   Run any cargo command (for example `cargo check -p stem-core`) so that
   Cargo writes the new workspace version into `Cargo.lock`. Commit the four
   files as `repo: release 0.2.0`. Wait until both CI jobs (`linux` and
   `windows`) pass on `master`. CI does not run for tags, so the release run
   is the only build of the tag.
2. **Tag that commit and push the tag:**

   ```sh
   git tag -a v0.2.0 -m "Stem 0.2.0"
   git push origin v0.2.0
   ```

   The workflow's first step after the checkout compares the tag with the
   three versions, before anything is built. The run fails when the tag is
   not of the form `v<version>`, or when the tag without its `v` differs from
   any of the three versions.
3. **Check the draft.** When the _Release_ run passes, GitHub → Releases
   shows a draft _Stem 0.2.0_ with `Stem_0.2.0_x64-setup.exe` attached. Its
   notes name the commit and the tag, say that the installer is unsigned and
   name the asset. Download the installer, then install, start and uninstall
   Stem with it. Edit the notes to add what changed.
4. **Publish** the draft (_Edit_ → _Publish release_). Only then is the tag's
   release visible to everyone.

To build the same tag again (after a failed run or a broken draft), first
delete the draft release if there is one. Deleting it keeps the tag. Then
start the workflow by hand: Actions → _Release_ → _Run workflow_, with the tag
(`v0.2.0`) as input. A manual run builds the tag, not the branch it is started
from, and fails for a tag that does not exist. A run for a tag that already
has a release, draft or published, fails after the build and leaves that
release unchanged.

## The installer is unsigned

Neither the installer nor `stem.exe` is code-signed. SmartScreen may stop the
installer with "Windows protected your PC" until the file has a reputation;
the user then chooses _More info_ → _Run anyway_. The release notes say so.

The installer is per-user (`installMode: currentUser`). It installs to
`%LOCALAPPDATA%\Stem` and never asks for administrator rights.

## What the installer and uninstaller do

Both come from the NSIS template of the Tauri CLI (`@tauri-apps/cli` 2.12.4).
A release build writes the generated script to
`target/release/nsis/x64/installer.nsi`. That script does the following:

- **Pages.** The wizard is English only and shows no language dialog. Its
  pages are _Welcome_, the _License Agreement_ page with `LICENSE`, a
  reinstall page when an earlier installation is found, the install folder,
  progress, and _Finish_ (with _Create desktop shortcut_ and _Run Stem_).
- **Registration.** The installer registers Stem under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Stem` with the
  publisher `davidcristian`, so Installed apps lists _Stem_, its version and
  `davidcristian`.
- **A running Stem.** Before they copy or delete Stem's files, both the installer
  and the uninstaller use the Restart Manager to check whether `stem.exe` in
  the install folder is running. If it is, they show "Stem is running! Click
  OK to kill it". _OK_ closes Stem with a forced shutdown, so Stem may not
  restore the taskbar's auto-hide setting. _Cancel_ stops the setup. Quit Stem
  first.
- **Application data.** The uninstaller's confirm page has a _Delete the
  application data_ box, cleared by default. When it is ticked, the
  uninstaller deletes `%APPDATA%\dev.davidcristian.stem` (settings) and
  `%LOCALAPPDATA%\dev.davidcristian.stem`
  (logs, the taskbar and crash markers, and WebView2's data).
  When it is clear, both folders stay, and a Stem installed later reads the
  same settings. An update run (`/UPDATE`, the switch Tauri's updater passes)
  never deletes them.
- **Always removed:** `stem.exe`, the uninstaller, the install folder once it
  is empty, and the Uninstall key.
- **Removed except on an update:** the Start menu and desktop shortcuts, and
  the `Stem` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`,
  which is the _Start Stem when you sign in_ entry.

## Automatic updates and code signing

Stem has neither. Each needs the following.

- **Automatic updates.** `tauri-plugin-updater` in the app;
  `bundle.createUpdaterArtifacts` and the updater's public key
  (`plugins.updater.pubkey`) in `src-tauri/tauri.conf.json`; the private key
  and its password as the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, set for the `tauri build` step of
  `release.yml`; and a `latest.json` manifest uploaded with the installer.
  The maintainer generates the key pair (`pnpm tauri signer generate`) and
  keeps the private key safe: without it, existing installations can no
  longer update.
- **Code signing.** An Authenticode code-signing certificate, used by the
  bundler (`bundle.windows.signCommand` or `certificateThumbprint`) in the
  same `tauri build` step, so that `stem.exe` and the installer are signed
  before the workflow attaches them to the draft.
