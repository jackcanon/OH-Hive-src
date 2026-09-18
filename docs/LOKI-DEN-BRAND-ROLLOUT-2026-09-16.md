# Loki's Den brand rollout — the to-do behind the brand pack

> **Superseded for the mark itself, 2026-09-18.** Jack selected **Fenrir** over this pack's carved
> doorway D/L. The pack below stays in the repository as history; the live Den artwork is
> `docs/lokis-den-fenrir-v1/`, and **B-1 (app icon) is done** — see `FENRIR-INTEGRATION.md`. The
> naming, palette and queue reasoning in this document still stand, including the rule that Hive
> remains the right word for the community and that the bundle identifier does not move.

Loki, 2026-09-16. Sif produced a v1 identity for Loki's Den after Jack selected the name and
reported buying **lokisden.app**. Jack's instruction: **do not let this get lost, and queue the
logo/icon rollout for later.** This is that record.

## Where the pack lives now

**`docs/lokis-den-brand-v1/`** in this repo — copied from
`/Volumes/10TB JBOD/Agents/Claude/output/lokis-den-brand-v1/`, all **137 files verified against the
pack's own `SHA256SUMS.txt` after the copy**.

That move is the point. It was sitting in an `output/` directory, which is where generated work goes
to be deleted by whoever next tidies up, and the only other copy is a single markdown file under
`Projects/Websites/lokislab/docs/`. Now it is version-controlled and pushed. There is precedent:
`docs/honey-logo-package/` (3.8M) already lives here, so a 3.2M brand pack in `docs/` is the
established pattern rather than an exception.

Contents: visual HTML guide and editable Markdown, preview, outlined SVG symbols, wordmarks and
lockups in five colors, transparent PNGs, three app-icon colorways, macOS ICNS/iconset/AppIcon
catalog, Windows ICO, Linux PNG/SVG, web/favicon/touch/maskable icons with a starter manifest,
design tokens and contrast checks, plus production sources.

## The identity, in one paragraph

Inherits Loki's Lab's Ink/parchment/serif typography. **Den copper `#D26743`** (taken from the
actual Lab favicon) is the Den's color; **Hive/Honey gold `#F5B82E`** stays the community's. A
slightly deeper **Den Iron `#5E6662`** was chosen to fix an inherited Iron-on-parchment contrast
shortfall — seven chosen text pairs clear 4.5:1. The mark is a carved doorway **D** with an inset
**L**, simplified for small favicons. Primary line: **"A home for your AI."**

Naming settled with it: **Loki's Den** for this workspace, **Loki's Lab** as maker, **The Hive** for
the optional community, **Honey** for the community currency, and **Halo** keeps its separate
pooled-inference meaning.

## Status: proposed, not approved

Sif was explicit that the visual direction is for review and was not silently approved or deployed.
Nothing has been renamed, no site deployed, no DNS changed, no signed app build produced. Her own
verification notes that platform launcher and store integration are untested, and that no modern
layered Icon Composer asset is claimed.

**So the first item below is Jack's, not an engineer's.**

## The queue

**B-0 — Jack reviews the visual direction.** Open `docs/lokis-den-brand-v1/brand-guide.html`, or
`brand-preview.png` for the quick version. Everything below is blocked on a yes, and none of it
should start before then — a rename half-applied against an unapproved mark is worse than no rename.

**B-1 — App icon.** Replace the Den app icon from the ICNS/iconset/AppIcon catalog already in the
pack, and check it at every size the Dock and Finder actually use. Coordinates with the queued
`build-app.sh` work (S-3 in Sif's queue), since the bundle assembly is being touched anyway —
doing both in one pass avoids rebuilding the app twice.

**B-2 — In-app identity.** Wherever the app currently says "Hive" about *the Den*, it should say
Loki's Den. This is a careful pass, not a find-and-replace: the word **Hive is still correct** for
the community, `ohghive.com` is unchanged, and the `OHHive` module and `media.happyjack.hive` bundle
identifier are load-bearing. **Changing the bundle identifier would orphan every existing install's
Application Support directory**, so that decision is separate and deliberate. Pairs naturally with
the settings reorganization (`LOKI-SETTINGS-REORG-2026-09-16.md`), which is already separating the
two products in the UI.

**B-3 — Web and favicon.** lokisden.app, plus favicon/touch/maskable icons and the starter manifest
from the pack. Jack reports the domain is purchased; routing has not been tested.

**B-4 — The survey and any published material** still using "Hive" as the broad product label, per
the earlier naming entry.

## Do not start with a repository rename

`OH-Hive-src`, the `OHHive` Swift module and the `hive`/`hive-core` crate names are internal and
cost real churn to change — every import, every path in every doc and handoff, every CI reference.
The user-visible surfaces above are what carry the brand. A repo rename, if it ever happens, is a
deliberate standalone task after the product reads correctly, not part of this rollout.
