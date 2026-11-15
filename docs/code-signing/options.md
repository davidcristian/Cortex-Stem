# The code-signing options

This page compares the ways Stem's Windows files can be code-signed: who can
get each option, what it costs and whether it works on GitHub-hosted CI.

| Option | List price on 2026-11-15 | Who can get it | Works on GitHub-hosted CI |
| --- | --- | --- | --- |
| [Microsoft Artifact Signing](#microsoft-artifact-signing) | Basic $9.99/month, Premium $99.99/month | Organisations in the listed countries; individuals in the USA and Canada only | Yes |
| [OV certificate](#ov-certificate-from-a-certificate-authority), key on a hardware token | Typically $150 to $300 a year | Worldwide | No (the token is a USB device) |
| [OV certificate](#ov-certificate-from-a-certificate-authority), key in the CA's cloud hardware security module (HSM) | Varies; Certum's open-source one lists at €49 | Worldwide | Depends on the vendor's tool |
| [SignPath Foundation](#signpath-foundation) | Free | Qualifying open-source projects | Yes, through SignPath |
| [EV certificate](#ev-certificates) | $400+/year | Organisations | Like OV; no SmartScreen advantage |
| [Microsoft Store](#microsoft-store) | Free for MSIX | Anyone | Not for the NSIS installer |

Microsoft's own comparison is [Code signing options for Windows app
developers][ms-options] (page dated 2026-10-29). The price ranges for OV and
EV certificates come from that page.

## Microsoft Artifact Signing

Artifact Signing is Microsoft's managed signing service. Its earlier name is
**Trusted Signing**. Some pages call it _Azure Artifact Signing_, and the
Azure price list still files it under "Trusted Signing". Microsoft names it
its recommended option outside the Store. Facts from the
[overview][ms-as-overview], the [quickstart][ms-as-quickstart] (dated
2026-05-21), the [FAQ][ms-as-faq] (dated 2026-05-14) and the
[Azure retail price list][azure-prices]:

- **Price.** Basic: $9.99 a month with 5,000 signatures and one certificate
  profile of each type. Premium: $99.99 a month with 100,000 signatures and
  ten profiles of each type. Signatures beyond the quota cost $0.005 each. The
  month in which the account is created is billed in full; it is not
  pro-rated. A release of Stem needs only a few signatures, one for each file
  listed under [Two different signatures](two-signatures.md).
- **Eligibility.** A paid Azure subscription; free, trial and sponsored
  subscriptions are not accepted. Per the quickstart, public-trust
  certificates go to organisations in the United States, Canada, the European
  Union, the United Kingdom, Australia, New Zealand, Japan, South Korea,
  Singapore, Switzerland, Norway and Israel, and to **individual developers in
  the United States or Canada only**. Microsoft's comparison page lists only
  the USA, Canada, the EU and the UK for organisations. The two pages give
  different lists; the quickstart is the more detailed one. An individual's
  validation takes the legal name and address from the Azure billing account
  (account type _Individual_) and checks them against a government ID through
  Microsoft Entra Verified ID. An organisation's validation takes 1 to 20
  business days.
- **What the certificate says.** The validated legal name, which cannot be
  customised. For an individual, that is the person's name and their city,
  state or province and country. SmartScreen and the file properties show
  that name as the publisher.
- **How it works.** The keys stay in Microsoft's FIPS 140-3 Level 3 HSMs and
  are never handed out. Certificates are renewed daily and are valid for only
  72 hours ([certificate management][ms-as-certs], dated 2026-09-30). Every
  signature must therefore include an RFC 3161 timestamp countersignature: a
  signed time from a timestamp server, which keeps the signature valid after
  its certificate expires. The service's timestamp server is
  `http://timestamp.acs.microsoft.com`. The service issues no EV certificates
  and plans none.
- **Trust.** The certificate management page says that the service's CAs are
  scheduled for removal from the Common CA Database and are no longer in
  Microsoft's Trusted Root Program. Microsoft's comparison therefore marks
  Artifact Signing as not eligible for Store submissions. The CAs stay on the
  Windows platform trust list, so Windows keeps trusting what they sign.
- **SmartScreen.** No immediate reputation; reputation builds as it does for
  any OV certificate.

## OV certificate from a certificate authority

This is a standard (OV, not EV) code-signing certificate from a CA in
Microsoft's Trusted Root Program (DigiCert, Sectigo, GlobalSign, SSL.com,
Certum and others). Since **1 June 2023**, the CA/Browser Forum's Code Signing
Baseline Requirements oblige the CA to make sure the key is generated, stored
and used in a hardware crypto module. A certificate can therefore no longer be
bought as a `.pfx` file to put in a CI secret. Since **1 March 2026**, a new
certificate is valid for at most **460 days** ([ballot CSC-31][cabf-csc31];
[requirements][cabf-csc]). Microsoft puts the usual price at $150 to $300 a
year.

The key is kept in one of two places:

- **A USB hardware token** that the CA ships. Signing needs the token plugged
  into the machine that runs the build, so a GitHub-hosted runner cannot sign.
  The choices are a self-hosted runner with the token, or building and signing
  releases on a local machine. The token's driver usually makes the
  certificate available in the Windows certificate store, which is where
  Tauri's `certificateThumbprint` looks for it; check the CA's instructions.
- **The CA's cloud HSM**, used through the CA's own signing tool, which Tauri
  runs through `signCommand`. Whether it works unattended in CI depends on the
  vendor. Two examples:
  - SSL.com's eSigner works in GitHub Actions but needs a one-time password on
    every call. The password is generated from a TOTP (time-based one-time
    password) secret stored as a repository secret ([SSL.com's CI
    guide][sslcom-ci]).
  - Certum's [Open Source Code Signing in the Cloud][certum-oss] lists at €49
    gross (the page states no period). It is issued to a natural person with
    "Open Source Developer" in the subject, keeps the key in Certum's
    SimplySign cloud, signs at most 5,000 times a month, and needs the
    SimplySign mobile and desktop apps.

## SignPath Foundation

SignPath Foundation offers free signing to open-source projects that qualify;
Microsoft's comparison page names it. The [SignPath Foundation
terms][signpath-terms] (marked as a draft) require:

- an OSI-approved licence without commercial dual licensing (Stem is
  AGPL-3.0-only);
- no proprietary components;
- a project that is actively maintained and **already released** in the form
  to be signed;
- binaries built from source in a verifiable way;
- defined author, reviewer and approver roles, with multi-factor
  authentication for all of them;
- a manual approval for every release;
- a "Code signing policy" section on the project's home page.

**The certificate is issued to SignPath Foundation**, so Windows shows
"SignPath Foundation" as the publisher, not the maintainer.

## EV certificates

Extended-validation certificates no longer get SmartScreen reputation at once.
Microsoft removed that in 2024, and EV-signed files now build reputation
exactly like OV-signed ones ([SmartScreen reputation][ms-smartscreen],
[code signing options][ms-options]). An EV certificate is still required to
register a Hardware Dev Center account to sign drivers ([driver signing
requirements][ms-driver-signing]), which Stem does not need. The EV price
($400+ a year) brings no SmartScreen benefit over an OV certificate.

## Microsoft Store

Microsoft re-signs an MSIX package published through the Store, for free, and
such a package never gets a SmartScreen warning. Microsoft does not re-sign an
MSI or EXE installer published through the Store: the publisher must sign it
with a certificate that chains to a Trusted Root Program CA ([code signing
options][ms-options]). Stem builds only an NSIS `.exe` (`bundle.targets` is
`["nsis"]`), so the Store would remove the need for a certificate only if Stem
were packaged as MSIX.

[ms-smartscreen]: https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation
[ms-options]: https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options
[ms-as-overview]: https://learn.microsoft.com/en-us/azure/artifact-signing/overview
[ms-as-quickstart]: https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart
[ms-as-faq]: https://learn.microsoft.com/en-us/azure/artifact-signing/faq
[azure-prices]: https://prices.azure.com/api/retail/prices?$filter=productName%20eq%20%27Trusted%20Signing%27
[ms-as-certs]: https://learn.microsoft.com/en-us/azure/artifact-signing/concept-certificate-management
[ms-driver-signing]: https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/code-signing-reqs
[cabf-csc]: https://cabforum.org/working-groups/code-signing/requirements/
[cabf-csc31]: https://cabforum.org/2025/11/17/ballot-csc-31-maximum-validity-reduction
[certum-oss]: https://shop.certum.eu/open-source-code-signing-on-simplysign.html
[sslcom-ci]: https://www.ssl.com/how-to/integrating-esigner-with-ci-cd-pipelines-a-complete-setup-and-configuration-guide/
[signpath-terms]: https://signpath.org/terms
