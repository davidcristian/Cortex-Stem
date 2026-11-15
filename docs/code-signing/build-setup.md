# Adding signing to the build

This page describes the build configuration that code-signs Stem's Windows
files: Tauri's signing settings, the steps a release workflow would add, and
the repository secrets each option needs.

## Tauri's settings

Tauri 2 reads the signing configuration from `bundle.windows` in the Tauri
configuration ([Windows code signing][tauri-sign], [configuration
reference][tauri-config]):

- `certificateThumbprint`: the SHA-1 thumbprint of a certificate in the
  Windows certificate store. Tauri then signs with `signtool.exe`, using
  `digestAlgorithm` (`"sha256"`), `timestampUrl`, and `tsp: true` when the
  timestamp server uses RFC 3161.
- `signCommand`: any command that signs a file in place, as a string or as
  `{ "cmd": "...", "args": ["...", "%1"] }`. `%1` is replaced with the path of
  the file to sign. Tauri runs the command for every file it signs.

`tauri build --no-sign` skips signing. `tauri build --config <file or JSON>`
merges extra configuration over `tauri.conf.json`. Both options are listed in
the help of `@tauri-apps/cli` 2.12.4, the version that `package.json` sets
exactly.

**Do not put signing into `src-tauri/tauri.conf.json`**, nor into
`tauri.windows.conf.json`, which Tauri merges automatically on Windows. Every
build would then try to sign: the CI `windows` job, the debug build of
`scripts/check-all.ps1`, and any developer's build. None of them has the
credentials. Put the signing settings in a separate file that only the release
workflow passes, for example `src-tauri/tauri.signing.conf.json`:

```json
{
  "bundle": {
    "windows": {
      "signCommand": "artifact-signing-cli -e https://weu.codesigning.azure.net -a <account> -c <certificate-profile> -d Stem %1"
    }
  }
}
```

This is the Artifact Signing form from Tauri's guide. `artifact-signing-cli`
is installed with `cargo install artifact-signing-cli`. It needs `signtool`
(Windows SDK 10.0.26100.0 or later recommended), .NET (8 recommended) and the
Azure CLI, and it reads the service principal from `AZURE_CLIENT_ID`,
`AZURE_CLIENT_SECRET` and `AZURE_TENANT_ID`. Its arguments:

- `-e`: the account's regional endpoint (the quickstart lists them; `weu` is
  West Europe);
- `-a`: the account;
- `-c`: the certificate profile;
- `-d`: the description.

The endpoint, account and profile names are not secrets and can stay in the
file.

For an OV certificate whose token or cloud tool puts it into the certificate
store, the file would instead hold:

```json
{
  "bundle": {
    "windows": {
      "certificateThumbprint": "<SHA-1 thumbprint of the certificate>",
      "digestAlgorithm": "sha256",
      "timestampUrl": "<the CA's RFC 3161 timestamp URL>",
      "tsp": true
    }
  }
}
```

For a vendor tool, it would hold
`"signCommand": { "cmd": "<tool>", "args": [..., "%1"] }` with the arguments
from the vendor's documentation.

Tauri's guide also shows how to import a `.pfx` from the secrets
`WINDOWS_CERTIFICATE` and `WINDOWS_CERTIFICATE_PASSWORD`. The guide states
that this applies only to OV certificates bought before 1 June 2023, so it is
not an option for a new certificate.

## Changes to `.github/workflows/release.yml`

This is a sketch for Artifact Signing; `release.yml` does not contain it. The
secrets are set in the environment of the `tauri build` step only:

```yaml
      - name: Install the Artifact Signing CLI
        run: cargo install artifact-signing-cli --locked

      - name: tauri build
        shell: bash
        env:
          AZURE_TENANT_ID: ${{ secrets.AZURE_TENANT_ID }}
          AZURE_CLIENT_ID: ${{ secrets.AZURE_CLIENT_ID }}
          AZURE_CLIENT_SECRET: ${{ secrets.AZURE_CLIENT_SECRET }}
          # Once the updater is added:
          # TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          # TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        run: |
          # (the existing retry loop, with this build command; assumes the CLI
          # reads the --config path relative to the repository root)
          pnpm tauri build --config src-tauri/tauri.signing.conf.json

      # Fail the run, before the draft release exists, if anything came out
      # unsigned or without a timestamp.
      - name: Check the signatures
        shell: pwsh
        run: |
          foreach ($file in 'target/release/stem.exe', "target/release/bundle/nsis/Stem_$($env:VERSION)_x64-setup.exe") {
            $signature = Get-AuthenticodeSignature -FilePath $file
            if ($signature.Status -ne 'Valid' -or -not $signature.TimeStamperCertificate) {
              Write-Output "::error::$file is not signed and timestamped: $($signature.Status) $($signature.StatusMessage)"
              exit 1
            }
            Write-Output "$file signed by $($signature.SignerCertificate.Subject)"
          }
```

The release notes that `release.yml` writes ("The installer is unsigned")
change in the same commit.

Repository secrets, by option (names only):

| Secret | Option | What it holds |
| --- | --- | --- |
| `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET` | Artifact Signing | An app registration (service principal) with the _Artifact Signing Certificate Profile Signer_ role on the account |
| The vendor's names, for example SSL.com's username, password, credential id and TOTP secret | OV in a cloud HSM | The vendor tool's login |
| `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | The updater, with any option | The updater's private key and its password |

A GitHub **environment** (for example `release`) with required reviewers
withholds these secrets from every job until someone approves the run
([deployments and environments][gh-environments]). On GitHub's Free, Pro and
Team plans, required reviewers are available for public repositories only.

Each retry after a download failure (the retry loop in `release.yml`) signs
the files again. With Artifact Signing, every signature counts against the
monthly quota; a few retries use a small part of it.

[tauri-sign]: https://v2.tauri.app/distribute/sign/windows/
[tauri-config]: https://v2.tauri.app/reference/config/
[gh-environments]: https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments
