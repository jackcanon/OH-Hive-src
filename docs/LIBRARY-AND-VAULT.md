# Library and Vault terminology

Jack's direction, 2026-09-18: the top-level data area is **Library**. It contains collections
(such as Spark Meeting Notes), organized by the Librarian. **Vault** is reserved for a future
secrets-management area within Library, not a name implying all indexed documents are secrets.

The current change renames visible navigation and collection controls. Internal database IDs,
FFI names, paths and existing collections remain unchanged, including all imported meetings.
The ordinary Library index is not a credential store. A future Vault needs its own credential
storage and access rules, with secret payloads excluded from ordinary full-text indexing and
agent context. No new secrets storage is implemented by this terminology change.

Library now offers a native Markdown file picker and the existing folder picker/review list,
which can navigate local repository folders. It does not execute repository code. Individual
file imports use a source-path hash so two files named README.md do not overwrite each other.
Supported intake remains Markdown. Rich document formats, an in-app repository/file tree and
collection document listing remain follow-up work; the file picker is not a full file manager.

## Spark incremental sync

Spark's installed CLI exposes meeting-date filters and paginated lists but no documented
modified-time filter or change cursor. Every five minutes, list meeting metadata within the
chosen history window and fetch only IDs not previously imported successfully. Re-read existing
notes once daily, or on **Refresh existing notes**, so edits still propagate. New meetings that
arrive late with an old date are found because discovery includes the full selected date range.

Successful imports persist their IDs after intake. A failure does not mark the failed meeting
as imported. Full-review completion is stored separately from last discovery completion. Old
configurations without these fields need one baseline refresh, then normal polls skip existing
bodies. Pause, destination/history/transcript changes and manual refresh keep existing semantics;
reconfiguring restarts the baseline. No settings or database rows are migrated destructively.

This reduces a normal unchanged 672-meeting run from ~686 CLI calls (14 lists + 672 bodies)
to ~14 lightweight list calls, while daily review still rereads the bodies. A future supported
Spark change feed could replace polling. Current discovery is not claimed to be push-based.
