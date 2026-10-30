# Dependency compatibility patches

These are upstream consumer crates, not renamed advisory-bearing crates. No
advisory is ignored by these patches. `UPSTREAM.json` records original registry
archive SHA-256 checksums and source revisions; original manifests and licenses
are retained. Review the diff against those releases when updating.

- askama_derive 0.12.5: replace unmaintained basic-toml with maintained toml 0.9.
  Parsed configuration and syntax delimiters now own strings, with a borrowed
  parser view for each template. This avoids leaking strings or relying on the
  old parser's borrowed-string behavior. Existing UniFFI/Askama versions remain
  compatible; generation and constructor checks validate real bindings.
- i18n-config 0.4.8: replace basic-toml with toml 0.9 and its explicit decode-error
  type. Configuration remains serde-deserialized; no environment/include loader.
- i18n-embed-fl 0.9.4: backport the proc-macro-error3 dependency/import migration
  used by upstream i18n-embed-fl 0.10.1, retaining the age 0.11 localization API.
- tauri-plugin-opener 2.5.5: move the Linux/BSD D-Bus dependency to zbus 6.2, whose
  integrated wire types replace the unmaintained standalone zvariant/zbus_names
  crates. Preserve the opener's existing native behavior and permissions.

The older age pin remains because newer age brings an affected crypto-bigint
branch that cannot be safely fixed with a lockfile update. No cryptographic
implementation or backup format is changed by the localization patches.
Wasmtime/WASI move to the patched 48.0.8 release independently.

Removal: adopt upstream releases that eliminate these transitive crates, remove
matching patch entries/directories, regenerate Cargo.lock, and rerun cargo-deny,
consumer compatibility checks, native binding/startup checks and platform CI.
