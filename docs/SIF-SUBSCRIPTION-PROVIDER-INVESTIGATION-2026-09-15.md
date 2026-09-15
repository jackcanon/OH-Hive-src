# Hive subscription-provider investigation

Date: 2026-09-15. Author: Sif your friendly Codex Agent.

Research and recommendations for Claude to review. No provider integration was installed, authenticated, implemented, or live-tested. This supplements ADR-033 and the ChatGPT integration handoff; it does not change their implementation queue. Findings are based on official documentation retrieved today. Plan eligibility and runtime compatibility must be checked again before release.

## Findings that change the shortlist

Subscription funding and authentication are separate choices. Some services require a key but debit an existing subscription allowance. Hive should distinguish **subscription sign-in**, **subscription key**, and **separately billed API**. Do not discard a subscription-key option just because its setup involves copying a key.

| Provider | Documented route | Recommendation for Hive |
|---|---|---|
| ChatGPT / Codex | Existing ADR-033 official runtime design | Continue queued implementation and its release gates. |
| GitHub Copilot | Official SDK supports subscribed users and app OAuth | Strong next adapter candidate; validate selected SDK version and account policy. |
| Gemini | Google-authenticated official CLI, ACP, cached-auth headless mode | Technical route established; clarify broader Hive deployment scope before public subscription-coordinator promise. |
| Grok | Official Grok Build subscription login and ACP orchestration | Promote to strong adapter candidate, not API-only. |
| Mistral | Plans share included usage across Vibe, Studio and API | Strong candidate; key-based connection can still satisfy subscription funding. |
| Kimi Code | Subscription key explicitly documented for third-party/self-built tools | Good candidate for personal coding workflows; verify broader production scope. |
| MiniMax Token Plan | Subscription key for compatible tools | Candidate for individual developer use; unattended production use needs separate evaluation. |
| Z.AI GLM Coding Plan | Key, but restricted supported-tool scope | Seek Hive inclusion/confirmation; compatibility alone is insufficient. |
| Qwen / Alibaba ModelStudio | Coding Plan subscription key | Investigate eligible workload/region; old free Qwen OAuth is discontinued. |
| Claude | Third-party subscription login requires prior approval | Keep BYOK today; approval is possible in principle, not secured. |
| Perplexity | API billing separate from consumer service | Lower priority for the subscription-first requirement. |

GitHub source: [SDK authentication](https://docs.github.com/en/copilot/how-tos/copilot-sdk/auth/authenticate). Distinguish GitHub Copilot from Microsoft consumer Copilot and Microsoft 365 Copilot; the earlier continuity assessment of those separate products still applies.

## Gemini: what is settled and what is not

**Authentication and unattended mechanics are documented.** Google recommends personal-account login, including the account holding AI Pro/Ultra. Credentials stay cached locally. Its headless section explicitly permits reuse of existing cached authentication; API/Vertex configuration is needed when no cached credential exists. Therefore “headless requires a separately billed API key” would be incorrect. Organizational accounts can have additional Cloud-project requirements. [Authentication](https://geminicli.com/docs/get-started/authentication/)

**Integration is also documented.** The actual Gemini CLI exposes ACP over stdio for IDEs and other developer tools, with existing editor integrations and MCP extension support. This supports a design where Hive supplies the interface and scoped fleet tools while Gemini CLI remains the agent runtime and credential owner. Verify actual method/schema compatibility against a pinned release rather than copying conceptual method names into production. [ACP documentation](https://geminicli.com/docs/cli/acp-mode/)

**Backend token reuse is prohibited.** Google's FAQ rejects third-party harvesting or piggybacking on CLI OAuth to reach backend services and directs independent coding agents to API/Vertex authentication. The terms page separately prohibits direct third-party access to the services behind the CLI. An open-source CLI license does not grant unrestricted access to its hosted service. [FAQ](https://geminicli.com/docs/resources/faq/), [service terms and privacy](https://geminicli.com/docs/resources/tos-privacy/)

My interpretation: an official ACP client driving Gemini CLI is materially different from replacing its agent loop with Hive's loop and reusing its bearer token. The former is a documented developer-tool architecture. However, the reviewed pages do not explicitly settle every aspect of a distributed, general-purpose, commercially distributed Hive coordinator or pooled community work. That narrower scope question remains; it is not a missing headless authentication feature.

Recommended design: one member-owned Gemini runtime on the coordinating computer, scoped Hive MCP tools for submission/status/results, six independently authenticated local workers. No OAuth-token extraction, fake client identity, or shared subscription gateway. A user-started personal development session is the first validation target. Community automation must be assessed separately from the member's private work.

### Exact questions to resolve with Google

Draft only; no message has been sent:

1. May a distributed desktop app drive the unmodified Gemini CLI through documented ACP, with CLI-owned Google login and AI Pro/Ultra allowance, to coordinate its user's personal development work across their computers?
2. Does permission extend to background continuation after a user starts a project, and to non-coding library curation/research?
3. May its MCP tools delegate tasks to local models and return results? Does opt-in community-project work change the allowed scope?
4. Is additional approval needed for bundling/updating the CLI or this product use? What entitlement/remaining-quota surface should Hive use without accessing private endpoints?

Answers should be recorded with source/date and exact workload boundaries. We can build transport/test doubles independently; do not market an unrestricted Gemini subscription backend meanwhile.

## Grok: a documented subscription orchestration path

The official Grok Build launch announcement identifies SuperGrok and X Premium Plus subscription access and explicitly describes ACP for bots and agent orchestration applications. That is stronger evidence than merely observing a login button. Current Build docs also describe browser sign-in and headless use. [Launch announcement](https://x.ai/news/grok-build-cli), [Build documentation](https://docs.x.ai/build/overview)

Use the official runtime over ACP with runtime-owned authentication and Hive's scoped tool bridge. This is a viable candidate, subject to a pinned-runtime pilot and current account eligibility. It is not permission to copy tokens into Hive's raw xAI API adapter. Separately billed xAI API access remains a distinct route. [Account/billing FAQ](https://docs.x.ai/console/faq/accounts)

A concrete implementation concern: Grok documents per-model credential precedence, including configured keys that can outrank session tokens. Isolate the subscription runtime configuration and reject paid-key contamination, rather than assuming that successful browser login guarantees subscription billing. [Enterprise authentication documentation](https://docs.x.ai/build/enterprise)

## Other providers worth including

**Mistral:** current docs describe global plans and included monthly usage across Vibe, Studio, and the API. Vibe CLI offers browser credential provisioning as well as keys. Paid overage depends on organization settings. For Hive, evaluate a subscription-funded API connection first, and the official runtime where useful. Explicitly check/communicate the account's overage configuration. [Subscriptions](https://docs.mistral.ai/admin/billing-usage/subscriptions), [CLI authentication and billing](https://docs.mistral.ai/vibe/code/cli/api-keys-profiles)

**Kimi Code:** documents subscription-backed keys for third-party and self-built tools, including general agent frameworks. Official clients use OAuth; a Hive-native OAuth client is not established. Use the coding endpoint, not the separately funded general platform. Preserve Hive's true client identity. Scope the first version to personal development; broad hosted-product use is not established by this guide. [Membership integration guide](https://www.kimi.com/en/help/kimi-code/membership-guide)

**MiniMax:** Token Plan explicitly supports compatible tools through a subscription key. Its FAQ describes individual interactive developer use and recommends pay-as-you-go for production. This supports a personal developer option, not a blanket claim that continuously running community coordination is covered. [Token Plan and FAQ](https://platform.minimax.io/subscribe/token-plan)

**Z.AI:** GLM Coding Plan restricts use to supported tools/environments. Although Hermes and OpenClaw appear in its list, Hive does not. Ask for inclusion or confirmation; do not spoof another application's identity. [Tool integration scope](https://docs.z.ai/devpack/tool/others)

**Qwen / Alibaba:** current Qwen Code docs say its free OAuth tier ended April 15, 2026. They describe Coding Plan subscription keys and distinguish usage-based Token Plan/standard API access. Verify region and permitted Hive use before adding a subscription option. [Authentication](https://qwenlm.github.io/qwen-code-docs/en/users/configuration/auth/)

**Claude correction for the shared log:** the earlier Loki entry says there is no partner exception. Anthropic's current primary SDK documentation starts the restriction with “Unless previously approved.” Hive has no such approval, so do not implement subscription login now. But “requires provider approval” is more accurate than “permanently impossible.” [Agent SDK overview](https://code.claude.com/docs/en/agent-sdk)

**Perplexity:** consumer subscription should not be presented as portable API funding; API billing is separately documented. [API billing](https://www.perplexity.ai/help-center/en/articles/10354847-api-payment-and-billing)

This is a prioritized survey, not a claim to have exhausted every provider or editor subscription. Existing Nous/API connections can remain explicit choices; no new subscription entitlement for them is asserted here.

## Recommended implementation sequence and acceptance gates

1. Finish the already queued Codex scaffold and existing ADR-033 gates.
2. Add GitHub Copilot as its own SDK adapter; prototype Grok Build ACP next.
3. Add a billing-aware subscription-key path, starting with Mistral and Kimi after checking account/workload scope.
4. Resolve Google's scoped questions while building reusable ACP transport. Keep Gemini's provider-specific behavior separate from Grok's.
5. Evaluate MiniMax, Alibaba and Z.AI once scope is settled; request Anthropic approval only if Jack wants that outreach.

Shared infrastructure should cover sessions, cancellation, approval UI, durable receipts, submission deduplication and quota-paused state. Each adapter must own its protocol/version/account logic. Store funding mode separately from credential type. Never silently switch to paid APIs, purchase credits, or enable account overages.

Before release, prove on macOS, Windows and Linux: legitimate login/key setup; visible funding mode; one scoped local task; exact worker placement; result return; no duplicate submission after disconnect; cancellation; expired auth; quota exhaustion without paid fallback; credential isolation; logout. Cloud context is opt-in and secrets stay out of transcripts. A provider's coding subscription is not automatically permission to pool accounts or operate a shared community inference service.

Verification this turn: primary-source review and documentation only. No live runtime or billing claims have been tested. Claude should review this report before expanding the integration queue or describing all provider options as interchangeable.

Sif your friendly Codex Agent
