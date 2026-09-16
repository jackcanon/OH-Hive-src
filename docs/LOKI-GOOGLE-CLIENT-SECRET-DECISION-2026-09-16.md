# Google needs a client_secret. My handoff rule was wrong. Jack's decision.

**Loki, 2026-09-16** · Raised by Sif's live test · **Needs Jack's call before the Google
connector can do anything real**

---

## What I told Sif, and why it was wrong

My split queue said, flatly:

> A Google client secret exists and **Jack holds it.** It must not enter the repo or the
> binary. Desktop-app clients authenticate with PKCE; Google's own docs say the installed-app
> secret "is obviously not treated as a secret."

The quote is real and the security *reasoning* holds. The **operational conclusion did not**.
Sif's live test reached the loopback callback after consent and then the token endpoint refused
the exchange:

```
invalid_request / client_secret is missing
```

PKCE protects the authorization code. It does **not** make `client_secret` optional for this
registration. I inferred one from the other, and that is the second time today I have reasoned
from "PKCE exists" to "no secret needed" — the first was GitHub, where Sif also caught me. The
pattern is mine, not hers, and it is worth naming: *PKCE and client authentication are separate
concerns, and every vendor combines them differently.*

Sif was right to stop and escalate rather than paste a secret in to make the error go away.

## There is no secretless path for Google. I checked all three.

| flow | needs `client_secret`? | scopes |
|---|---|---|
| Desktop app + PKCE (current registration) | **yes** — proven by live test | any |
| Web application | yes, genuinely confidential | any |
| Limited-input device ("TV") | **yes** — the docs require it at the polling step | `drive.file` ✅, **Gmail ✗** |

I went looking for the device-flow escape hatch specifically, because that is what saved us on
GitHub. It does not save us here: Google's limited-input device flow supports `drive.file` (our
exact Drive scope) but **has no Gmail scope at all**, and it requires the client secret anyway.
So it costs us `sendGmail` and buys nothing.

**Conclusion: every Google installed-app flow requires the secret.** GitHub was the exception,
not the rule, and I generalised from it.

## What Google itself says about that secret

> "Installed apps are distributed to individual devices, and it is assumed that these apps
> cannot keep secrets."

That is Google's own threat model, on the native-app page. The installed-app `client_secret` is
**not a confidential credential** in their design — it is closer to a second identifier. The
docs list it as "Optional," which is what misled me; in practice the Desktop token endpoint
rejects the exchange without it.

## The two real options

### A. Ship the installed-app credential (recommended)

Publisher provides it once at build time, alongside the client ID, in
`apps/desktop-swift/config/oauth-clients.sh` where Sif already put the public IDs. PKCE stays.
Members still just click Connect.

- This is what every desktop app that talks to Google does — `gcloud`, `rclone`, and the rest
  all ship one, because Google's flow requires it.
- It keeps the product's posture intact: the member's token still lands in *their* Keychain,
  and Hive never sees their data or their tokens.
- **The honest risk:** anyone can extract the ID+secret pair from the binary and build an app
  that shows **"Loki's Den"** on Google's consent screen. That is a phishing surface, and it is
  the real reason Google asks for a secret at all. It is an accepted, industry-wide risk for
  desktop apps, but it is not zero and I would rather write it down than wave it away.

### B. Hosted confidential client — Hive brokers the exchange

A real Web-application client, secret held server-side, desktop calls a Hive endpoint to swap
the code.

- Genuinely protects the secret, and lets us rotate and revoke it.
- **But** every member's Google token exchange then flows through Hive infrastructure. For a
  product whose stated posture is "your own machine, your own data" — the exact line ADR-023
  and ADR-026 draw between connectors and MCP — that is an architectural regression, and it
  makes Hive an availability dependency for a member connecting their own account.
- It is a different registration and a different architecture, as Sif correctly said. Not a
  one-line proxy patch.

## My recommendation

**A**, with **ADR-036 decision 1 amended rather than quietly broken.** The decision's real
intent is "the desktop binary ships nothing that grants server-side authority on its own." That
intent survives A: the installed-app secret grants nothing without a user completing consent,
and Google explicitly assumes it cannot be kept. GitHub keeps its stronger property — genuinely
no secret — because device flow allows it.

What the amendment should say, so this does not get relitigated:

- GitHub: device flow, no client secret, no private key. Unchanged.
- Google: installed-app client ID **and** secret ship in the binary as build configuration,
  because Google requires it and treats it as non-confidential. PKCE retained.
- Neither is stored in a way that implies confidentiality — no Keychain, no secret store, no
  `.env` that looks like it holds real secrets. They are build config, named as such.
- If Google's consent screen impersonation ever becomes a live problem, B is the escape hatch
  and this document is where the reasoning lives.

**This is Jack's call, not mine and not Sif's** — it changes what ships in a binary that goes to
members. I have recorded the recommendation; nobody should insert a secret until he says so.

Loki
