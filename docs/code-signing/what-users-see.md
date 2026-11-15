# What users see

This page describes what Windows shows a user who downloads and runs Stem's
files, with and without a code signature. It is based on Microsoft's
[SmartScreen reputation for Windows app developers][ms-smartscreen] (page dated
2026-05-04) and [What is Smart App Control?][ms-sac]:

- **Unsigned (Stem's current state).** SmartScreen shows _Windows protected
  your PC_. The user has to choose _More info_, then _Run anyway_. Enterprise
  policy can remove that choice. An unsigned file starts with no reputation,
  and every new version starts again from zero. Reputation passes from an
  earlier version to a new one only when both are signed with the same
  publisher identity. A self-signed certificate behaves exactly like no
  signature.
- **Newly signed**, whether with an organisation-validated (OV) certificate,
  an extended-validation (EV) certificate or Artifact Signing. SmartScreen
  still warns on the first downloads that the app is unrecognised, but now
  shows the verified publisher name. Reputation then grows with download
  volume: the prompt stops once the file has enough clean download history.
  Microsoft gives no threshold; it "can take several weeks and hundreds of
  clean installs from a wide audience". A developer cannot submit a file for
  consumer reputation review. Enterprise administrators can submit files to
  the [Microsoft Security Intelligence portal][ms-wdsi].
- **Later signed releases.** Files signed with the same certificate can
  inherit the certificate's reputation, so new versions may skip the warning.
  Changing the signing certificate affects that signal. Modifying a file after
  signing breaks the signature. Signing anything that behaves like unwanted
  software can give the certificate a negative reputation.
- **Smart App Control** (Windows 11). Where it is on, it may take precedence
  over SmartScreen. It blocks an unsigned file, or one with an invalid
  signature, unless Microsoft's service predicts that the file is safe, and it
  checks every executable, not only downloads. There is no per-app exception:
  a user can only turn Smart App Control off altogether. Microsoft's advice to
  blocked users is to ask the developer to sign the app. On such a machine an
  unsigned Stem is therefore likely to be blocked, the installer and
  `stem.exe` alike, with no _Run anyway_.

The installer installs per user and never asks for administrator rights
([RELEASING.md](../RELEASING.md#the-installer-is-unsigned)), so there is no
UAC (User Account Control) prompt that would show a publisher name.

[ms-smartscreen]: https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation
[ms-sac]: https://support.microsoft.com/en-us/topic/what-is-smart-app-control-285ea03d-fa88-4d56-882e-6698afdb7003
[ms-wdsi]: https://www.microsoft.com/en-us/wdsi/filesubmission
