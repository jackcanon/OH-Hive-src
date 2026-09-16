# Loki’s Den — brand and icon pack v1

Prepared September 15, 2026. Design direction for review, not a production application release.

## Open first

- `brand-guide.html`: complete visual guide; open in a browser. Works offline and includes print styles.
- `BRAND-GUIDE.md`: editable text guide.
- `brand-preview.png`: visual preview.

## Logos

- `assets/den-symbol-*.svg`: D/L doorway mark, five colors.
- `assets/den-wordmark-*.svg`: symbol + Loki’s Den.
- `assets/den-lockup-*.svg`: symbol + Loki’s Den + BY LOKI’S LAB.
- All production SVG lettering is outlined; no font installation needed.
- `png/`: transparent exports at 512, 1024 and 2048 px WIDTH; proportions vary.
- `assets/den-app-*.svg`: rounded app artwork in dark, light and copper treatments.
- `assets/*-fullbleed.svg`: square sources for platforms that supply their own masks.

## Icons

- `icons/macos/Den.icns`, `Den.iconset`, and `AppIcon.appiconset`.
- `icons/windows/Den.ico`.
- `icons/linux/`: PNGs and scalable SVG.
- `icons/web/`: favicons, touch/PWA/maskable artwork, manifest, installation snippet.

These are artwork files. They have not been installed into a signed Den.app, tested in every operating-system launcher, or submitted to stores. The AppIcon catalog targets traditional macOS icons; a layered Icon Composer source is not included. Web files are starter integration assets and do not make a site installable by themselves.

## Design decisions

Copper belongs to personal Den actions. Honey gold identifies community/Honey actions. Keep the original Honey mark for currency. Serif marketing typography, ink, parchment, and carved geometry carry the Loki’s Lab family resemblance.

## Sources and verification

Source design references: the current Loki’s Lab brand guide, CSS, and favicon; Hive web CSS; approved Honey pack README. The Den symbol is newly constructed vector artwork. `CONTRAST-CHECK.md` records numeric palette checks.

Checked SVG XML, PNG readability and dimensions, all supplied ICO frames, ICNS PNG representations, asset catalog references, and web manifest references. Browser preview checked at 1440 px and 390 px; no horizontal page overflow and all images loaded. Production SVGs use outlined Georgia/Arial glyphs generated with CoreText. No font files are redistributed.

Sources in `source/` reproduce this local build; runtime paths and workspace output path are specific to this Mac. Sequence: build-guide.py, outline-type.swift, finalize-type.py, export.cjs, package-icons.py. Do not regenerate after manual design edits without carrying those edits into the sources. Live-type logo copies are in `source/editable-type`.

Domain purchase is user-reported. This pack does not configure DNS, publish the site, rename existing repositories, or update the earlier survey.

Sif your friendly Codex Agent
