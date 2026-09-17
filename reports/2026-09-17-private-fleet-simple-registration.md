# Private Fleet registration and onboarding simplification

The first computer now starts with one action: **Sign in to register this Mac**. First-run Setup encourages registration before model setup, with an explicit option to do it later. Settings and the Projects registration sheet reuse the same short view. Existing-fleet connection is separate and initially selects joining a primary; sharing controls are collapsed after registration.

## Automatic browser handoff

The app obtains the existing native enrollment challenge, starts a loopback-only listener, and opens the enrollment website with a state-bound request in its URL fragment. The website preserves the short-lived request in session storage across Google/Apple sign-in, removes the fragment from browser history, loads the user's fleets, and requests explicit approval. A first fleet is created when none exists; a failed fleet lookup cannot silently create a new one. The browser returns the signed assertion to the app. Rust still validates signature, configured trust, owner/node/authority, challenge and expiry before registration succeeds. This does not invite anyone into a community Hive.

Listener callbacks reject wrong state/path/method, duplicate parameters and oversized requests. Cancelled attempts cannot satisfy a later attempt. The callback destination is fixed to loopback with a bounded numeric port. The website does not claim enrollment succeeded; the app confirms after signature verification. Additional-computer joining still uses the existing manual pairing flow; this change simplifies first-computer registration, not fleet discovery or migration.

## Verification

- Standalone Swift callback test: valid round trip and nine invalid cases pass.
- TypeScript bridge test: invalid payload/ports/state, expiry, size bound, UTF-8 round trip pass.
- Web TypeScript check and Next production build pass.
- Native release build, generated bindings, signing and isolated engine-load probe pass. Initial sandboxed attempt could not write compiler cache; standard approved build then passed.
- Vercel production deployment `dpl_33jJS3rCSXtgjAs8R9UsUxFZB1KD` is Ready at ohghive.com. A concurrent session committed and pushed the shared implementation in `6fe8922`; this session did not issue the production deployment.
- Live browser with synthetic unsigned challenge displays **Register your computer**, **Continue with Google**, and **Continue with Apple**. No OAuth sign-in, fleet creation or registration approval was submitted during this check.
- Actual user Google/Apple enrollment through the rebuilt app remains to be verified. Native UI screenshot check remains pending; no claim of a completed live enrollment.

Logs: `/private/tmp/hive-enrollment-build.log`, `/private/tmp/hive-enrollment-web-build.log`, `/private/tmp/hive-enrollment-deployment.log`.

Reproduction: compile `FleetSignInCallback.swift` with `scripts/test-fleet-signin.swift` using swiftc; compile `apps/web/app/private-fleet/enroll/desktop-bridge.ts` as CommonJS and pass its output path to `node apps/web/scripts/test-desktop-bridge.cjs`.

Next: relaunch the rebuilt `apps/desktop-swift/Hive.app`, use Settings → Private Fleet → Sign in to register this Mac, finish browser consent and approval, confirm the registered state, then return to Private Fleet Projects. Browser may require a local-network permission for loopback navigation. Capture any real failure before declaring the entire onboarding flow verified.
