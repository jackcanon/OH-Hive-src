# Google connector C-1a–c implementation

Sif your friendly Codex Agent

Built locally, not packaged or deployed. Real Google OAuth, Drive upload, Gmail delivery and interactive multi-window acceptance remain unverified.

- `OHHiveApp.swift` owns the sole GoogleAuthManager and injects it into all four scenes. ConnectorsSettingsView consumes the shared instance. Existing ObservableObject architecture retained without unrelated migration.
- GoogleKeychain writes now update existing items instead of deleting before replacement, check OSStatus, and verify readback. Credential save errors are surfaced. Connection success requires a persisted refresh token; token and credential write failures cannot set Connected true. This is per-item persistence, not a transactional multi-key Keychain store; deletion failure handling and complete OAuth lifecycle/race testing remain follow-ups.
- `GoogleEmail.message` validates a single plain recipient and rejects CR/LF/multiple-recipient injection before token retrieval or network use. Subject content after the first newline/control is discarded; UTF-8 subject and body are encoded. The one-address restriction is intentional; display-name and internationalized address support are not claimed.
- `GoogleTextActions.swift` gives TranscribeView Save to Drive and Email Transcript. Save captures text/name at click; email captures an immutable transcript draft, shows recipient, editable subject and content, and only sends on explicit Send Email. Busy actions are disabled; errors and success use SettingsNote. Disconnected users get the Settings hint. No model-callable connector tool or expanded OAuth scope added.

Verification: Swift test builds app and tests successfully under Xcode beta; all 11 tests pass (7 existing Bots + 4 connector tests). Connector cases cover injected/multiple/malformed recipients, subject header injection, Unicode body roundtrip, base64url, and credential write status/readback validation without touching real credentials. Repo search finds one GoogleAuthManager construction at app scope. Whitespace check passes. Actual Keychain denial UI, OAuth browser callback, simultaneous windows, and Google actions were not tested; no emails/uploads were sent.

Next: C-1d saved-chat actions, then real-account acceptance for C-1a–d, followed by GitHub callback research (C-2). Existing OAuth client setup remains unchanged; shared-client-versus-user-client decision is still open. Preserve shared dirty files when integrating; no commit/push/deployment performed here.


## C-1d saved-chat export follow-up

Implemented `ChatGoogleExport.swift` and wired its observed-engine toolbar control in ChatView. The control appears only for connected Google accounts with nonempty chats and is disabled during a response. Clicking captures title and messages into an immutable preview; Drive saves the snapshot as `text/markdown` under the chat title plus `.md`. Every message uses a role heading and preserves its text and order. The preview also offers Email Chat through the existing explicit recipient/subject/Send sheet. Generalized GoogleTextActions with defaulted content label and MIME type so transcript behavior remains compatible.

Verification: app/test build passes; all 13 Swift tests pass, including a 25-message ordered role/Unicode export and immutable snapshot test plus blank-title fallback. Whitespace check passes. No Drive/Gmail reference was added to ChatEngine; the model tool surface remains unchanged. No live Google upload/email/OAuth or interactive toolbar test performed. C-1a–d implementation is present; real-account acceptance remains outstanding. Next independent work is C-2 GitHub callback investigation, not optional image export before verification.
