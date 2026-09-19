# Bundle completeness gate

Item 20d30783: signing and loading successfully did not catch missing publisher configuration or optional helpers.

`apps/desktop-swift/scripts/check-bundle-completeness.py` now checks nonempty launcher/app/core/icon resources, all 21 shipped avatars in the macOS SwiftPM resource bundle, executable cloudflared and Copilot helpers, source stamp, and presence of Google client ID/secret and GitHub client ID. It never prints configuration values. This is structural verification, not credential validity, helper functionality, icon artwork identity, or app/core commit equality.

The build defaults to `publisher`, builds the Copilot helper, checks before signing and again during verification, and publishes only after verification. Supply `HIVE_GOOGLE_OAUTH_CREDENTIAL_JSON` as a protected path to the approved desktop credential file. `HIVE_CLOUDFLARED_SOURCE` may point to the existing approved helper. No helper or credentials are copied from a live installation automatically. A missing credential file fails before compilation. Release automation must provision this file before calling the publisher build; do not bypass the check to ship.

Contributors without publisher credentials can explicitly use `OHHIVE_BUNDLE_PROFILE=development`; that profile omits only the publisher credential/helper requirements, keeping app/core/icon/avatar checks. Pass the same profile when separately verifying a development bundle. Default verification always enforces publisher completeness.

Verification on September 19:
- Six fixture tests, covering every required resource and key, missing/empty configuration, executable permission, invalid plist and explicit development mode.
- Installed Midgaard publisher bundle passes structural verification and portable-dependency/engine-load checks. No workers or UI started.
- Shell syntax and diff whitespace checks pass.

No new full signed app was built or installed in this pass. Requested/reported model diagnostics remain on separate commit a0edafd pending integration. Build identity item e6edc4ed remains separate: the existing Info.plist stamp alone does not prove the embedded core came from the same commit.
