# macOS signing & notarization

The release pipeline (`.github/workflows/release.yml`) produces a signed,
notarized, stapled `KuberNation.app` inside a drag-to-Applications `.dmg`.
`release-macos.sh` does the assemble → sign → notarize → staple → dmg work; it
runs in CI and locally.

> **Gotcha — two notarization submissions are required.** A ticket is keyed to the
> cdhash of the code that was *submitted*. The `.dmg` is its own separately-signed
> code object with its own cdhash, so notarizing only the `.app` leaves the `.dmg`
> unstapleable (`stapler` fails with *"Record not found"*). The script therefore
> submits the `.app` **and** the `.dmg`. Stapling both is what lets the `.dmg`
> verify offline at mount and the `.app` verify offline after being dragged to
> /Applications.

> **Gotcha — the signing keychain must be in the search list.** `codesign`
> resolves the signing identity through the keychain **search list**; passing
> `--keychain` alone is not enough and fails with *"<hash>: no identity found"*.
> CI therefore runs `security list-keychains -d user -s "$KEYCHAIN" …` after
> importing. This can only bite in CI — on a dev Mac the identity is in your
> login keychain, which is already in the search list.

> **Apple's notary service can be slow.** It's usually 1–5 minutes, but a
> submission has been observed sitting `In Progress` for ~90 minutes before being
> accepted. `NOTARY_TIMEOUT` (default `45m`) tunes the wait. If CI times out, the
> build job fails and `publish` never runs — no half-published release; just re-run
> the workflow.

## Required GitHub Actions secrets

Set these under repo **Settings ▸ Secrets and variables ▸ Actions**. If
`MACOS_CERT_P12_BASE64` is absent, the release still runs but ships the older
unsigned tarball.

| Secret | What it is |
| --- | --- |
| `MACOS_CERT_P12_BASE64` | Your **Developer ID Application** certificate **and its private key**, exported as a `.p12`, base64-encoded |
| `MACOS_CERT_PASSWORD` | The password you set when exporting the `.p12` |
| `APPLE_API_KEY_P8_BASE64` | Your App Store Connect API key `.p8`, base64-encoded |
| `APPLE_API_KEY_ID` | The API key's **Key ID** |
| `APPLE_API_ISSUER_ID` | The API key's **Issuer ID** |

### Preparing the certificate (`.p12`)

The Developer ID cert in your login keychain must be exported *with* its private
key so CI can sign with it:

1. **Keychain Access** → **login** keychain → **My Certificates**.
2. Find **Developer ID Application: <your name> (TEAMID)**, expand it to confirm
   a private key hangs beneath it.
3. Right-click the certificate → **Export…** → format **Personal Information
   Exchange (.p12)**, set a strong password (→ `MACOS_CERT_PASSWORD`).
4. Base64 it for the secret:
   ```sh
   base64 -i DeveloperID.p12 | pbcopy      # paste into MACOS_CERT_P12_BASE64
   ```

### Preparing the API key (`.p8`)

You already generated the key in App Store Connect → **Users and Access ▸
Integrations ▸ Keys** (role **Developer**). The Key ID and Issuer ID are shown
there. Base64 the downloaded `.p8`:

```sh
base64 -i AuthKey_XXXXXXXXXX.p8 | pbcopy   # paste into APPLE_API_KEY_P8_BASE64
```

The `.p8` downloads only once — keep your copy safe.

## Running a signed build locally

With the Developer ID identity already in your keychain and the API key `.p8` on
disk:

```sh
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
lipo -create -output out/kubernation \
  target/aarch64-apple-darwin/release/kubernation \
  target/x86_64-apple-darwin/release/kubernation

BIN_PATH=out/kubernation \
VERSION=1.0.0 \
ICON_PNG=crates/kubernation/assets/logo/mark.png \
IDENTITY="Developer ID Application: Your Name (TEAMID)" \
NOTARY_KEY=~/keys/AuthKey_XXXXXXXXXX.p8 \
NOTARY_KEY_ID=XXXXXXXXXX \
NOTARY_ISSUER=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx \
EXTRA_RESOURCES="LICENSE-MIT LICENSE-APACHE crates/kubernation/THIRD-PARTY-NOTICES.md" \
OUT_DMG=kubernation-v1.0.0-macos-universal.dmg \
bash packaging/macos/release-macos.sh
```

`security find-identity -v -p codesigning` lists your exact `IDENTITY` string.

## Verifying a produced `.dmg`

```sh
spctl -a -vvv -t install kubernation-*.dmg      # → "accepted, source=Notarized Developer ID"
xcrun stapler validate kubernation-*.dmg        # → "The validate action worked!"
```
