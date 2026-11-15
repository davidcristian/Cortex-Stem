# Sources

This page lists the documentation that the code-signing pages are based on.
All were read on 2026-11-15. A date in parentheses is the date the page itself
shows.

- Microsoft, [SmartScreen reputation for Windows app developers][ms-smartscreen] (2026-05-04)
- Microsoft, [Code signing options for Windows app developers][ms-options] (2026-10-29)
- Microsoft, [What is Smart App Control?][ms-sac]
- Microsoft, [What is Artifact Signing?][ms-as-overview] (2026-01-02),
  [Quickstart: Set up Artifact Signing][ms-as-quickstart] (2026-05-21),
  [Artifact Signing FAQ][ms-as-faq] (2026-05-14),
  [Artifact Signing pricing][ms-as-pricing] (the page loads its prices from the
  [Azure retail prices API][azure-prices], where the product is still "Trusted Signing")
- Microsoft, [Artifact Signing certificate management][ms-as-certs] (2026-09-30)
- Microsoft, [Driver code signing requirements][ms-driver-signing] (2025-05-29)
- CA/Browser Forum, [Code Signing Baseline Requirements][cabf-csc] (version 3.11.0) and
  [Ballot CSC-31: maximum validity reduction][cabf-csc31]
- Tauri, [Windows code signing][tauri-sign] (updated 2026-10-27),
  [configuration reference][tauri-config], [updater plugin][tauri-updater],
  and the bundler's source: [`bundle.rs`][tauri-bundle-rs], [`nsis/mod.rs`][tauri-nsis-rs]
- Certum, [Open Source Code Signing in the Cloud][certum-oss]
- SSL.com, [Integrating eSigner with CI/CD pipelines][sslcom-ci]
- SignPath Foundation, [terms][signpath-terms]
- GitHub, [Deployments and environments][gh-environments]

[ms-smartscreen]: https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation
[ms-options]: https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options
[ms-sac]: https://support.microsoft.com/en-us/topic/what-is-smart-app-control-285ea03d-fa88-4d56-882e-6698afdb7003
[ms-as-overview]: https://learn.microsoft.com/en-us/azure/artifact-signing/overview
[ms-as-quickstart]: https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart
[ms-as-faq]: https://learn.microsoft.com/en-us/azure/artifact-signing/faq
[ms-as-pricing]: https://azure.microsoft.com/en-us/pricing/details/artifact-signing/
[azure-prices]: https://prices.azure.com/api/retail/prices?$filter=productName%20eq%20%27Trusted%20Signing%27
[ms-as-certs]: https://learn.microsoft.com/en-us/azure/artifact-signing/concept-certificate-management
[ms-driver-signing]: https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/code-signing-reqs
[cabf-csc]: https://cabforum.org/working-groups/code-signing/requirements/
[cabf-csc31]: https://cabforum.org/2025/11/17/ballot-csc-31-maximum-validity-reduction
[tauri-sign]: https://v2.tauri.app/distribute/sign/windows/
[tauri-config]: https://v2.tauri.app/reference/config/
[tauri-updater]: https://v2.tauri.app/plugin/updater/
[tauri-bundle-rs]: https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle.rs
[tauri-nsis-rs]: https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle/windows/nsis/mod.rs
[certum-oss]: https://shop.certum.eu/open-source-code-signing-on-simplysign.html
[sslcom-ci]: https://www.ssl.com/how-to/integrating-esigner-with-ci-cd-pipelines-a-complete-setup-and-configuration-guide/
[signpath-terms]: https://signpath.org/terms
[gh-environments]: https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments
