# Security Policy

## Supported Versions

Hive is pre-1.0 and moves fast. Only the latest released version (see
[Releases](https://github.com/jackcanon/OH-Hive-src/releases)) is supported
with security fixes. There is no LTS branch yet.

## Reporting a Vulnerability

Please do **not** open a public GitHub issue for security vulnerabilities.

Instead, use one of these private channels:

1. **Preferred:** [GitHub Security Advisories](https://github.com/jackcanon/OH-Hive-src/security/advisories/new)
   for this repository ("Report a vulnerability" under the Security tab).
2. **Email:** jack@happyjack.media — please include "SECURITY" in the subject
   line.

We'll acknowledge your report as quickly as we can and keep you updated as a
fix is developed. Please give us reasonable time to ship a fix before any
public disclosure.

## Scope

Hive is a distributed network where community members run compute nodes
and regional servers on their own hardware, executing agent-driven tasks from
a shared queue. Reports that are especially valuable:

- Anything that lets a task/"card" escape its execution sandbox on a compute
  node (arbitrary code execution on a volunteer's machine).
- Authentication/authorization bypasses in the hub (Supabase RPCs, RLS
  policies) or in node/server pairing.
- Anything that would let one member read or spend another member's $honey
  balance, or forge ledger entries.
- Secrets or credential leakage (node keys, backup encryption keys, tunnel
  tokens) via any hub API or regional server endpoint.

Denial-of-service reports against a single volunteer node are lower priority
than anything that compromises the hub, the ledger, or another member's
machine or data.
