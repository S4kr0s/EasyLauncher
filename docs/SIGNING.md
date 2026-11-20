# Signing the installers

Unsigned builds work, but players see a warning on first launch. CI signs
automatically once the secrets below exist; without them it builds unsigned.

## macOS (Developer ID + notarization)

Needs a paid Apple Developer membership.

1. **Certificate.** In Xcode → Settings → Accounts → Manage Certificates, add a
   **Developer ID Application** certificate (not "Apple Development").
   Then in Keychain Access, export it with its private key as `devid.p12` and pick a password.
2. **Notarization key.** App Store Connect → Users and Access → Integrations →
   App Store Connect API → generate a key with the *Developer* role. Download
   `AuthKey_<KEYID>.p8` (it can only be downloaded once). Note the Issuer ID and the Key ID.
3. **Secrets.** Run these in the repo folder. Each one prompts for the value or
   reads it from a file, so the value never lands in shell history:

```bash
base64 -i devid.p12 | gh secret set APPLE_CERTIFICATE
gh secret set APPLE_CERTIFICATE_PASSWORD
gh secret set APPLE_SIGNING_IDENTITY   # e.g. Developer ID Application: Your Name (TEAMID)
gh secret set APPLE_API_ISSUER         # Issuer ID
gh secret set APPLE_API_KEY            # Key ID
gh secret set APPLE_API_KEY_P8 < AuthKey_KEYID.p8
```

`security find-identity -v -p codesigning` shows the exact identity string.
Delete `devid.p12` afterwards. The next CI run signs the app, notarizes it and staples the ticket to the `.dmg`.

## Windows

Without a signature, SmartScreen says "Windows protected your PC". The player
clicks **More info → Run anyway** once. For a group of friends that is usually acceptable.

Options if you want it gone:

| Option | Cost | Notes |
| --- | --- | --- |
| Unsigned | free | One extra click per install. Put a screenshot in the README. |
| Azure Trusted Signing (Artifact Signing) | ~10 $/month | Cheapest real signing; works in CI. Requires identity validation, and eligibility for individuals depends on country, so check before paying. |
| OV code-signing certificate | ~200–400 $/year | Since 2023 the key must live on a hardware token or cloud HSM, so CI needs the vendor's cloud signing. SmartScreen still warns until downloads build reputation. |

Neither paid option removes the warning on day one; SmartScreen reputation grows with downloads.
