# Honey — Honey Gold / Version 1.0

Approved direction: A. Honey is the currency for projects powered by the Hive, a shared compute pool.

## Start here
- `proofs/Honey-Gold-Proof-4096.png`: high-resolution visual proof.
- `guide/Honey-Brand-Guide.pdf`: colors, spacing, sizes and production guidance.
- `masters/svg/honey-symbol-gold.svg`: standalone currency symbol.
- `masters/svg/honey-wordmark-gold.svg`: gold symbol with charcoal Honey lettering.
- `masters/svg/honey-app-dark.svg`: primary gold-on-charcoal application icon.

## Contents
- `masters/svg`: scalable symbols and wordmarks in gold, black, white and charcoal; dark and light app icons. All wordmark glyphs are paths, so these files have no font dependency. Symbol masters use a single closed outline.
- `masters/pdf`: matching vector masters with transparent backgrounds where applicable. White masters appear blank on white; place them over a dark background. PDF artwork is scalable; page dimensions are not a required print size.
- `png`: transparent sRGB exports at 512, 1024 and 4096 pixels wide. Names state WIDTH, not height. App icons are square; symbols and wordmarks retain their proportions.
- `icons/macos`: Honey.icns, containing standard and Retina representations, plus the source Honey.iconset. Add ICNS to a macOS app bundle through the app's build configuration. PNG iconsets are supplied for asset-catalog integration.
- `icons/windows`: Honey.ico with 16, 24, 32, 48, 64, 128 and 256 pixel PNG-compressed frames. Suitable for modern Windows shortcuts and application resources. Packaged Windows apps may require additional manifest-specific images.
- `icons/linux`: 16 through 1024 pixel PNGs and scalable SVG. Install into the matching hicolor size/apps directories with the basename honey, then use Icon=honey in a desktop entry.
- `web`: ICO and SVG favicons, PNG favicons, 180 pixel Apple touch icon, 192/512 PWA icons, a 512 pixel maskable icon, web manifest and HTML installation snippet. Adjust paths to your deployment. The touch and maskable versions use a full-bleed background and a safely inset mark.
- `proofs`: final proof PNGs and editable SVG proof board. The proof board uses Arial/Helvetica for annotations; the logo itself is outlined. Presentation copy is illustrative and is not a required tagline.

## Color and usage
Gold #F5B82E (245,184,46); charcoal #1D1C20 (29,28,32); cream #FFF6DF (255,246,223). RGB masters are the source of truth. For printing, have the printer convert the vector masters through the intended paper/press ICC profile and approve a physical proof. No universal CMYK or Pantone match is asserted.

The H's outer stems are 80 design units wide; its central stroke is 32 units. Keep at least 80 units of external clear space around the standalone mark. App-icon backgrounds are intentional containers and have their own inset geometry. Minimum standard mark height: 32 digital pixels or 8 mm in print. The 16-24 pixel version is optically simplified and aligned to a 16 pixel grid: do not enlarge it for general branding.

Gold is an accent, not a small-text color on cream. Use charcoal text on light backgrounds. Do not rotate, stretch, add effects, replace the vertical stroke with a horizontal stroke, or use decorative bee stripes inside the symbol.

## Typography and source
Wordmark: Arial Bold converted into vector paths. Font software is not distributed. UI and document companion type: system sans-serif (Arial, Helvetica, Segoe UI or equivalent). Final logo artwork was constructed as deterministic vectors from approved concept A; it is not an enlarged raster concept image. Concept exploration used built-in image generation. Visual adjacency reference: https://officehours.global/ — dark neutral backgrounds and clean sans-serif typography. No Office Hours artwork is included and no affiliation is implied.

## Verification
PNG dimensions and alpha channels, ICO frame dimensions, ICNS representations, SVG XML and PDF page counts were checked. The guide and proof were visually reviewed. These are artwork assets; launchers and signed app bundles were not built or tested on all three operating systems.
