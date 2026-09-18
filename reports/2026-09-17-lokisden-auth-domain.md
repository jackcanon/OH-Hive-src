# Loki’s Den registration domain release

User requested moving personal registration to lokisden.app while community Hives remain invite-only.

Implemented in existing website checkout `/Volumes/10TB JBOD/AI-Workflow-Storage/projects/lokisden`: standalone branded `/private-fleet/enroll`, `/auth/callback`, safe relative auth-return helper, signed desktop bridge, and public Supabase browser client. Registration uses the Den layout, with no Hive member/server sidebar. The same backend identity and existing issuer/key are retained; no account migration or automatic Hive membership. Existing Den marketing/download routes remain intact.

Native `PrivateFleetSignIn` and `PrivatePrimaryView` now open lokisden.app for first-computer registration and secondary approval. Rebuilt `apps/desktop-swift/Hive.app` successfully, including bindings/signing/isolated engine-load probe. This updates the local app, not downloadable release artifacts or other fleet computers.

Production: added four exact allowed callback URL forms (apex/www with/without enrollment next query) to shared Supabase auth, preserving prior URLs. Added apex/www Den origins to the enrollment service, retaining Hive origins for compatibility. Only one declared auth configuration field changed. Full before/after pulled TOML comparison confirms all other settings unchanged. Automatic approval review initially rejected partial config based on reset concerns; read-only CLI contract/diff checks proved undeclared fields are preserved, then review accepted the update. No DNS, signing-key rotation or shared default site URL change.

Deployment `dpl_4rMAMnnhN94tLJvwG7pmE9ReRwKe` Ready, production alias lokisden.app. Native build and Den Next production build pass. Copied bridge parser/expiry/UTF-8/destination tests pass. Live browser synthetic challenge displays Register your computer / Continue with Google / Continue with Apple in Den branding. No authenticated approval submitted. Live service OPTIONS allows both Den origins (204), rejects untrusted origin (403), unauthenticated create request rejected with sign_in_required (401). auth.users has no non-internal database signup triggers; this limited check is not a complete authorization audit of the shared backend.

Remaining: real Google/Apple callback → owner fleet lookup/approval → native signature acceptance, nonmember account acceptance, additional-computer manual pairing verification. User must relaunch rebuilt app and begin a fresh registration request (previous challenges expire). Existing OAuth provider consent branding is inherited from the shared authentication project; separate provider branding is outside this domain change.

Logs: /private/tmp/lokisden-auth-{build,deploy,native-build,config-diff,config-push,config-verify}.log. Shared backend pxfbnuxcnerulbvbmowz. Dedicated public native trust was installed in prior release; private key remains only protected recovery file and Supabase secrets, never in site source.
