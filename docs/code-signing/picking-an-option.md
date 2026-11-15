# When the maintainer picks an option

These are the steps that add code signing to Stem's releases:

1. Choose one of the [options](options.md) and complete its identity
   validation (Artifact Signing: up to 20 business days for an
   organisation).
2. Add `src-tauri/tauri.signing.conf.json`, the secrets (preferably in a
   `release` environment) and the `release.yml` steps from [Adding signing
   to the build](build-setup.md).
3. Update [RELEASING.md](../RELEASING.md#the-installer-is-unsigned), the
   release notes in `release.yml` and the note in the
   [README](../../README.md#install) that the installer is not signed.
4. Check a release build on Windows:
   - _Properties_ → _Digital Signatures_ of `stem.exe` and of the installer
     show the publisher and a timestamp;
   - `Get-AuthenticodeSignature` reports `Valid`;
   - SmartScreen names the publisher on a fresh download.
