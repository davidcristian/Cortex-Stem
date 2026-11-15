# Code signing Stem

These pages describe how Stem's Windows files can be code-signed: the options,
what users see with and without a signature, and the build configuration each
option needs. The repository signs nothing: the installer and `stem.exe` are
unsigned (see [RELEASING.md](../RELEASING.md)), and these pages change no
behaviour.

The product names, prices and rules in these pages are as the linked
documentation stated them on 2026-11-15 (see [Sources](sources.md)). They
change often; check the links again before buying anything.

| File | What it covers |
| --- | --- |
| [two-signatures.md](two-signatures.md) | Authenticode and the updater signature, and which files the bundler signs |
| [what-users-see.md](what-users-see.md) | What SmartScreen and Smart App Control show for unsigned and signed files |
| [options.md](options.md) | The signing options compared: Artifact Signing, OV and EV certificates, SignPath Foundation, the Microsoft Store |
| [build-setup.md](build-setup.md) | Tauri's signing settings, the release workflow steps and the repository secrets |
| [updater-key.md](updater-key.md) | The updater's key pair, how to set it up and how it differs from Authenticode |
| [picking-an-option.md](picking-an-option.md) | The steps once the maintainer picks an option |
| [sources.md](sources.md) | The documentation these pages are based on |
