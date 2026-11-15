# The updater key is not a code-signing certificate

Tauri's updater ([updater plugin][tauri-updater]) installs an update only if
it is signed with the key pair the app was built with; this check "cannot be
disabled". To set up the updater, the maintainer:

1. generates the pair, with a password, for example
   `pnpm tauri signer generate -w ~/.tauri/stem.key`;
2. puts the content of the **public** key (not a path) into
   `plugins.updater.pubkey` in `src-tauri/tauri.conf.json`, and sets
   `bundle.createUpdaterArtifacts: true` so that `tauri build` writes the
   `.sig` files;
3. stores the content of the **private** key and its password as the
   repository secrets `TAURI_SIGNING_PRIVATE_KEY` and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. `tauri build` reads both from the
   environment (for a local build, the key variable may also hold a path); a
   `.env` file does not work;
4. keeps a safe copy of the private key. If it is lost, no update can reach
   the installations that trust its public key.

How it differs from Authenticode, in practice:

- It costs nothing, involves no CA and no identity check, and never expires.
  Windows does not check it: an installer with a correct `.sig` and no
  Authenticode signature still gets the SmartScreen warning when downloaded by
  hand.
- It needs the most protection. A lost or leaked code-signing certificate is
  revoked and replaced, and SmartScreen reputation is built again. A lost
  updater key means that no installed copy can update again. A leaked one lets
  anyone who can also serve the update manifest publish "updates" that
  installed copies install, until a release with a new public key has reached
  every user.
- The `.sig` covers the installer's bytes as they are when `tauri build`
  finishes. Keep Authenticode signing inside `tauri build` (the configuration
  in [Adding signing to the build](build-setup.md)). Signing the installer in
  a later step changes the file and makes its `.sig` invalid.

[tauri-updater]: https://v2.tauri.app/plugin/updater/
