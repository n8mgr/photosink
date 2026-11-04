# Release installers and signing

Merging the `chore: prepare release` PR tags the version and runs `publish.yml`.
It publishes a universal macOS DMG, a per-user Windows x64 setup EXE, and Linux
DEB packages for x86_64 and aarch64, alongside the existing Windows ZIP and Linux
tar archives. The DEBs target Ubuntu 24.04 or compatible newer systems and declare
their runtime library dependencies. Windows setup includes a Start menu shortcut
and an uninstaller; installing a newer version updates the same installation.
The Windows release statically links the Rust bridge's C runtime.

## Required macOS secrets

Add these in the repository's **Settings → Secrets and variables → Actions →
New repository secret** before merging a release PR. Missing credentials, failed
signing or rejected notarization fail the release; local builds and pull request
builds still use ad-hoc signing.

| Secret | Value |
|---|---|
| `APPLE_CERT_B64` | Base64 of a Developer ID Application certificate exported as `.p12`, including its private key. |
| `APPLE_CERT_PASSWORD` | Password chosen when exporting that `.p12`. |
| `APPLE_CERT_ID` | Full signing identity, such as `Developer ID Application: Your Name (TEAMID)`. |
| `APPLE_API_KEY` | App Store Connect team API key ID. |
| `APPLE_API_ISSUER` | Issuer ID shown for the team API key in App Store Connect. |
| `APPLE_KEY_B64` | Base64 of the downloaded `AuthKey_<KEY_ID>.p8` private key. |

### Get the signing certificate

1. Enroll in the [Apple Developer Program](https://developer.apple.com/programs/).
   As Account Holder, create a **Developer ID Application** certificate in
   Certificates, Identifiers & Profiles. Follow Apple's
   [certificate creation instructions](https://developer.apple.com/help/account/certificates/create-developer-id-certificates/),
   including creating the certificate signing request on your Mac.
2. Download the `.cer` and open it to import into Keychain Access on that same Mac.
   In **My Certificates**, select the Developer ID Application identity with its
   private key and export it as `.p12`, choosing an export password.
3. Find the full signing identity with `security find-identity -v -p codesigning`.
   Use its quoted `Developer ID Application: ...` name for `APPLE_CERT_ID`.
4. Copy the exported file as base64, then paste into `APPLE_CERT_B64`:

   ```sh
   base64 -i DeveloperIDApplication.p12 | pbcopy
   ```

Set `APPLE_CERT_PASSWORD` to the export password. The DMG uses the same Application
certificate as the app; no Developer ID Installer certificate is needed.

### Get the notarization API key

1. In [App Store Connect](https://appstoreconnect.apple.com/), open **Users and Access
   → Integrations → App Store Connect API → Team Keys**. The Account Holder may
   need to request API access first.
2. As Account Holder or Admin, generate a team key with the **Developer** role.
   Copy its Key ID and the page's Issuer ID into `APPLE_API_KEY` and
   `APPLE_API_ISSUER`. Use a team key for this workflow, which passes an issuer ID.
3. Download the `.p8` private key (available only once), then copy its base64 into
   `APPLE_KEY_B64`:

   ```sh
   base64 -i AuthKey_YOUR_KEY_ID.p8 | pbcopy
   ```

See Apple's [API key instructions](https://developer.apple.com/help/app-store-connect/get-started/app-store-connect-api/).
Keep the `.p12`, `.p8`, and export password outside the repository.

The workflow uses a temporary keychain with a randomly generated password, signs
the app with hardened runtime and a timestamp, signs and submits the DMG to
Apple, requires an `Accepted` result, then staples and validates the notarization
ticket and checks Gatekeeper acceptance. It removes the imported keychain and key
files even if a build fails. `APPLE_KEYCHAIN_PASSWORD` is no longer needed.
See Apple's [notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow).

## Optional Windows signing

The existing Azure Key Vault signing configuration is also used for the setup EXE
and its embedded uninstaller. Without these secrets Windows artifacts are unsigned;
with a configured vault, a signing failure fails the job.

| Secret | Where to get it |
|---|---|
| `AZURE_KEY_VAULT_URI` | Vault URI from your Azure Key Vault's Overview page. |
| `AZURE_CERT_NAME` | Name of the code-signing certificate stored in that vault. |
| `AZURE_CLIENT_ID` | Application (client) ID from a Microsoft Entra app registration. |
| `AZURE_TENANT_ID` | Directory (tenant) ID from that registration. |
| `AZURE_CLIENT_SECRET` | Secret **value** created under the registration's Certificates & secrets. |

Obtain a publicly trusted code-signing certificate from a CA that supports Azure
Key Vault, provision it there according to the CA's instructions, and authorize
the app registration to sign using it. AzureSignTool's
[setup walkthrough](https://github.com/vcsjones/AzureSignTool/blob/main/WALKTHROUGH.md)
covers the registration and vault permissions (key Sign, secret Get, certificate
Get for access-policy vaults). Linux packaging needs no secrets. GitHub provides
the workflow's `GITHUB_TOKEN` automatically.

If a release fails after tagging, fix the configuration and rerun the failed jobs
from the existing Publish run.
