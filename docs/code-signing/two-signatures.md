# Two different signatures

A signed Stem release has two unrelated signatures. Authenticode is
Microsoft's signature format for Windows executables and installers. The
updater signature is a separate signature on each update, which Tauri's
updater checks before it installs the update.

| | Authenticode (the subject of the code-signing pages) | Updater signature |
| --- | --- | --- |
| Checked by | Windows: SmartScreen, Smart App Control, the file's _Digital Signatures_ tab | Only Stem's own updater, against the public key built into the app |
| On | `stem.exe`, the uninstaller and the NSIS installer | The installer the updater downloads (a `.sig` file next to it) |
| Issued by | A public certificate authority (CA) or Microsoft's Artifact Signing service, after checking who you are | Nobody: the maintainer generates the key pair with `pnpm tauri signer generate` |
| Costs | Money, every year or month | Nothing |
| Expires | The certificate does (at most 460 days; 72 hours for Artifact Signing); a timestamped signature stays valid | Never |
| Losing it | Get a new certificate; SmartScreen reputation starts again | Existing installations can never update again |

Neither replaces the other. Authenticode makes Windows show the publisher's
name and stop treating the file as unknown. The updater signature only proves
to an installed Stem that an update was signed with the private key that
matches the public key in the app. [The updater key is not a code-signing
certificate](updater-key.md) has the details.

When signing is configured, the Tauri bundler signs these files, as read from
its source ([`bundle.rs`][tauri-bundle-rs], [`nsis/mod.rs`][tauri-nsis-rs]):

- the main binary `stem.exe`, after the bundler has written the bundle type
  into it (a change that would break an earlier signature);
- the NSIS plugin DLLs it ships;
- the uninstaller, through a sign command that the bundler passes to the NSIS
  script;
- the finished `Stem_<version>_x64-setup.exe`.

[tauri-bundle-rs]: https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle.rs
[tauri-nsis-rs]: https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle/windows/nsis/mod.rs
