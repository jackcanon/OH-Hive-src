# Fenrir — the Loki's Den mark

Jack selected Fenrir on 2026-09-18 and asked for it to be rolled out across the applications.
This is the record of what was imported, what was rewired, and what is deliberately untouched.

## The pack lives in the repository

**`docs/lokis-den-fenrir-v1/`** — 165 files copied from the approved delivery at
`/Volumes/10TB JBOD/Agents/Claude/output/lokis-den-fenrir-v1/` and **verified against the pack's
own `SHA256SUMS.txt` after the copy: 165 of 165 OK, zero mismatches.**

Same reasoning as the v1 pack before it (`LOKI-DEN-BRAND-ROLLOUT-2026-09-16.md`): `output/` is
where generated work goes to be deleted by whoever next tidies up. Version control is the only
copy that survives. `docs/honey-logo-package/` and `docs/lokis-den-brand-v1/` already establish
the pattern.

Contents: editable SVG masters with lettering outlined to paths, transparent PNG symbols,
wordmarks and endorsed lockups, macOS ICNS/iconset/AppIcon catalog plus Icon Composer layers,
Windows multi-resolution ICO, Linux hicolor hierarchy and desktop-entry example, web
favicon/touch/maskable set with a starter manifest, `palette.json`, and the rebuild sources.

**`docs/lokis-den-brand-v1/` is not deleted.** Its carved-doorway D/L mark is superseded for
Loki's Den, but it stays as the historical pack.

## What changed

**`apps/desktop-swift/scripts/build-app.sh`** — `ICON_SRC` now reads
`docs/lokis-den-fenrir-v1/platforms/macos/Den.icns` instead of the v1
`docs/lokis-den-brand-v1/icons/macos/Den.icns`. The existing bundling step already copies that
file to `Contents/Resources/AppIcon.icns`, which is what the generated Info.plist's
`CFBundleIconFile=AppIcon` names, so nothing else in the script needed touching. No bundle
identifier, executable name, app display name, signing identity or user-data path changes —
`media.happyjack.hive` stays exactly as it is, because changing it would orphan every existing
install's Application Support directory.

That is the only shipping surface in this repository that carries the Den mark.

## What is deliberately unchanged

| Surface | Mark it carries | Why it stays |
|---|---|---|
| `apps/desktop/src-tauri/icons/` (Tauri) | Hive honeycomb | `productName` is **Hive**; this is the community app, not the Den |
| `apps/web/public/` favicons, touch and maskable icons, `site.webmanifest` | Hive/Honey | `apps/web` is the Hive community site (ohghive.com); title and manifest both say Hive |
| `apps/halo-bench/HaloBench.icns` | Halo | Halo keeps its separate pooled-inference identity |

The naming decision from the v1 rollout still holds: **Hive is still the correct word for the
community.** Repainting the community site with the Den's wolf would be the wrong change, not an
incomplete one.

## Ready but not wired to anything yet

These are in the repository and correct, waiting on a target that does not exist here yet:

| Target | Source in the pack |
|---|---|
| Windows Den build | `platforms/windows/Den.ico` (16/24/32/48/64/128/256) |
| Linux Den packaging | `platforms/linux/hicolor/`, `lokis-den.desktop.example` |
| Den website (lokisden.app) | `platforms/web/` + `head-snippet.html` |
| Xcode asset-catalog consumers | `platforms/macos/AppIcon.appiconset/` |
| Icon Composer | `platforms/macos/composer-layers/` (import-ready layers, **not** a compiled `.icon`) |

The Den website is a separate checkout (`/Volumes/10TB JBOD/AI-Workflow-Storage/projects/lokisden`)
and is not touched by this import. Dropping `platforms/web/` into it is a website change and a
deploy, and belongs to whoever owns that release.

## Verification

Done by actually running it, not by reading the script:

- The 165-file copy was checksum-verified against the pack's own manifest.
- `sips` reports the new `Den.icns` as a 1024 × 1024 ICNS.
- A full `build-app.sh` run with the real Developer ID produced `Loki's Den.app`, and the icon
  inside the bundle is byte-identical to the Fenrir ICNS
  (`ba09bec4269887c007907c66a4767b5d216686108df816922b5748a0e2ba19f9`, 239,964 bytes) and
  different from the v1 mark it replaced (`00e2bc32…`, 56,740 bytes).

macOS caches app icons aggressively. An already-installed copy of the Den keeps showing its old
icon until it is replaced, and the Dock may need a restart to pick up the new one.
