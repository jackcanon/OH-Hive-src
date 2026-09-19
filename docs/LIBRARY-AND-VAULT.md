# Library and Vault terminology

Jack's direction, 2026-09-18: the top-level data area is **Library**. It contains collections
(such as Spark Meeting Notes), organized by the Librarian. **Vault** is reserved for a future
secrets-management area within Library, not a name implying all indexed documents are secrets.

The current change renames visible navigation and collection controls. Internal database IDs,
FFI names, paths and existing collections remain unchanged, including all imported meetings.
The ordinary Library index is not a credential store. A future Vault needs its own credential
storage and access rules, with secret payloads excluded from ordinary full-text indexing and
agent context. No new secrets storage is implemented by this terminology change.

## Local directory catalog (2026-09-18 clarification)

Jack clarified that Library identifies where items live; it must not copy directory items.
The Library screen's old file/folder copy controls are replaced by **Directories and assets**
and **Scan a directory**. Scanning records absolute path, category, size and modification time
in a local metadata catalog. The user reviews results and chooses **Save locations to Library**.
No source files are read for content, copied, moved, executed or granted to agents by discovery.
Existing imported Spark notes and manually authored notes are preserved.

Git working repositories (including worktrees with a .git file), Markdown, images, audio,
video, documents, design assets and archives are recognized. Names and paths are searchable
across saved scans; Finder reveals the original. Hidden paths, symlinks, packages and common
build/dependency directories are skipped. Maximum 50,000 visited entries per scan; unreadable
paths and truncated scans are reported. Partial rescans retain earlier entries; complete
rescans replace that root's catalog. Remove from catalog never deletes files. Scans are manual,
cancellable and run away from the main UI thread. Metadata lives only on this Mac in
`~/Library/Application Support/ohhive/library-directories.json` (0600).

Limitations: directory metadata is separate from the existing collection full-text index.
Asset contents, external Markdown text, bare Git repositories and arbitrary source folders
without Git metadata are not indexed by this scanner. No automatic watching, fleet-wide path
resolution, security-scoped bookmarks, or move tracking yet. Rescan refreshes locations.
Jack explicitly confirmed Spark meetings are the exception: keep importing and updating
searchable managed meeting notes. Location-only discovery does not change Spark sync.

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
