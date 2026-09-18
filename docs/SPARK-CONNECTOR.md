# Spark meeting-notes connector

Built-in macOS connector under **Settings → Connectors → Spark · Meeting notes**.
Uses Readdle's installed Spark CLI; no API key, model, or MCP server is needed for this transport.

## Setup

1. Keep Spark Desktop open. In Spark Settings → AI Agents, set up its CLI and allow Read access and meeting notes for the desired accounts.
2. In Loki’s Den Connectors, check the Spark connection.
3. Choose an existing local managed Vault or create **Spark Meeting Notes**.
4. Choose the initial history window (week, month, three months, or year) and optionally full transcripts.
5. Start automatic import. Keep both apps open on this importing Mac.

The starting date is fixed when configured: future syncs include new meetings and revisit older
meetings since that date for edits. Sync runs at launch and every five minutes, with one run at a
time; Sync now is also available. Pause stops before importing the next meeting. Imported copies
remain if removed from Spark. Existing Vault sharing settings govern access. Changing destination
leaves copies in the old Vault. This first release imports into this Mac's Vault, not a remote
primary's Vault. Only enable one importer for a given destination/source.

## Implementation

`SparkMeetingImporter.swift` invokes the fixed `spark meetings` and `spark meeting --notes`
commands, with optional `--transcript`, using an argument vector (no shell). Spark must be in
`/usr/local/bin/spark` or `/opt/homebrew/bin/spark`. It uses Spark's existing account permissions.
The parser is deliberately bounded and rejects unknown output rather than silently recording errors
as notes. Text output is a compatibility dependency; changes in Spark's formatting may require an
adapter update. Up to 100 pages of 50 meetings are supported per sync. Each command is limited to
30 seconds and 1 MiB output. No emails are retrieved or changed.

A persisted installation namespace and numeric Spark meeting ID produce stable source filenames.
Markdown retains the title and original output (including date, participants and source link).
Staging files live under `~/Library/Application Support/ohhive/spark-import/`; directory/file modes
are 0700/0600. Existing `vaultIntakeApproveFile` provides durable source generations, duplicate
suppression, edit protection and revisions. Staging alone never marks a meeting imported. Errors
leave previous successful imports intact; the next sync revisits the source. Imported documents and
staging files are not deleted by Pause. A Spark database reset that reuses message IDs is not yet
explicitly detected; disconnect/reconfiguration after such a reset needs source-identity support.

Preferences are `sparkMeetingImport.v1` in app UserDefaults, disabled initially. The app-lifetime
service runs independently of the Settings screen; it does not install a login item or separate
background daemon. A separate command process writes to a bounded temporary file; stdout/stderr
are not shown in logs. No Spark credentials are stored by Den.

## Validation and remaining work

Parser fixtures cover pagination, Unicode/truncated titles, empty results, unexpected output,
oversized notes and stable content/provenance. Configuration roundtrip retains the source namespace.
Existing Rust intake tests cover duplicate no-op, edited content replacement and invalid files.
One authorized real Spark note was parsed without displaying its content or importing it.

Remaining: installed UI verification and a user-selected initial real import; broader Spark versions,
source reset detection, per-account filtering beyond Spark's own access settings, and remote-source
publishing. This is maintained by Loki’s Den, not a claim of Readdle endorsement.

Official references:
- https://sparkmailapp.com/help/spark-cli/getting-started-with-spark-cli
- https://sparkmailapp.com/help/spark-cli/set-up-spark-cli-with-your-ai-agents
