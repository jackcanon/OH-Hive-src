# Loki’s Den — Fenrir logo package
Version 2 · 18 September 2026

Fenrir is the symbol of **Loki’s Den**, the personal local/cloud agent harness by **Loki’s Lab**. The Hive remains the optional community compute network. Use “Loki’s Den” in marketing, “Den” for compact labels, and “Den.app” when referring to the Mac application. Fenrir is the artwork name, not a product rename.

This is the primary Fenrir mark, updated at the user’s request from jaw study 02. The editable vector interpretation uses its composed front-facing head: two intact eyes, a broad muzzle, lowered brow, deep angular jaw and restrained canine tips. Copper facets and cream markings connect it to the approved character. The expression is simplified for icon clarity; the roaring expression remains promotional artwork. This version supersedes the earlier single-eye Fenrir mark and the original D/L mark. Reference studies are retained in `reference/`.

## Start here
- `preview.html`: visual guide, variants and actual-size icon checks.
- `masters/`: editable SVG artwork, with lettering converted to paths.
- `png/`: transparent symbols, wordmarks and endorsed lockups at 512, 1024 and 2048 pixels wide; app artwork at 1024.
- `platforms/web/`: SVG/ICO favicons, touch icon, regular and maskable PWA icons, manifest and integration snippet.
- `platforms/macos/`: Den.icns, 10-image Den.iconset, Xcode AppIcon.appiconset, and aligned Icon Composer source layers.
- `platforms/windows/`: multi-resolution Den.ico plus individual PNG sizes.
- `platforms/linux/`: hicolor PNG/SVG icon hierarchy and desktop entry example.
- `palette.json`: machine-readable colors.
- `VERIFICATION.txt`: export validation results.
- `source/`: rebuild sources. Requires Python/Pillow, Node/sharp, and optional Swift/CoreText to regenerate outlined lettering. Runtime paths in scripts reflect the authoring machine; change them for another environment.

## Identity and use
**Primary mark:** copper wolf on Ink. **Light surface:** color wolf or Ink silhouette. **Dark surface:** color wolf or Cream silhouette. Use the flat color variant for print and the solid variants for engraving, stamps and single-color reproduction. Use the endorsed lockup when introducing the product; use the simpler wordmark once the parent brand is established.

The symbol leads. Do not add an L, horns, a ring or Hive hexagons to the wolf. Maintain the front-facing head and proportions. Do not stretch, rotate, outline or recolor individual facets. Preserve space around the mark equal to at least one fifth of its width; keep other content out of this area. Platform icon tiles already contain their own safe margins.

Use the unendorsed wordmark at 180 px wide or greater; endorsed lockup at 240 px or greater. Below these sizes use the symbol. Use prepared small app assets at 16–32 px; they remove pupil highlights, canine tips and minor shading facets. Minimum icon size is 16 px. At that size silhouette and palette carry the identity; facial detail is intentionally reduced.

## Color and typography
| Role | Color |
|---|---|
| Ink | #17201F |
| Copper | #D26743 |
| Ember | #B74627 |
| Copper shadow | #8E4D31 |
| Parchment | #ECE5D8 |
| Cream | #FFF6DF |

Primary lettering uses Georgia, with a sans-serif endorsement. Supplied SVG wordmarks use paths and require no font installation. For surrounding product interfaces use the platform system sans-serif. Color artwork includes intermediate facet shades and restrained gradients; use the flat master when a limited palette is required. Copper is an identity color, not a blanket approval for small body text on every surface.

## Web
Copy the contents of `platforms/web/` into the same public directory and adapt `head-snippet.html` paths. Manifest defaults assume deployment at the domain root; adjust start_url and scope for a subdirectory application. Keep maskable icons separate from ordinary icons: their smaller wolf is deliberately inside the mask-safe area. Apple touch and PWA images use a full-bleed background so the operating system can apply its own mask.

## macOS
For conventional Xcode icon assets, copy `AppIcon.appiconset` into your asset catalog and select it as the app icon. `Den.icns` supports applications that use an ICNS bundle resource. Both include small-size artwork and Retina representations.

For Apple's Icon Composer, import either background-dark or background-light plus wolf-color as aligned 1024 px layers. Use wolf-mono as a starting point for a monochrome appearance. Background layers are full-bleed; let the system supply the final mask. Inspect material, lighting and appearance behavior in Icon Composer before shipping. These are import-ready SVG/PNG layers, **not a compiled .icon project**. See [Apple Icon Composer](https://developer.apple.com/icon-composer/).

## Windows
Use `Den.ico` as the application/shortcut icon resource. It contains 16, 24, 32, 48, 64, 128 and 256 px frames. Individual PNGs include additional shell/tile sizes. Microsoft Store/MSIX artwork requirements depend on your package manifest; these source images are not a completed store submission or MSIX asset manifest.

## Linux
Install `hicolor/` beneath your app's icon data directory, or merge it into `~/.local/share/icons/hicolor/` for a per-user installation. Set `Icon=lokis-den` in the desktop entry. The provided desktop file is an example: replace its Exec and TryExec values with the actual installed executable. Refresh the desktop icon cache if required by the environment.

## Verification and scope
PNG files and every ICO/ICNS representation were decoded; Xcode asset dimensions and manifest links were checked. macOS sips recognizes the ICNS as 1024 × 1024. Artwork was reviewed at large and small sizes. This package has not been integrated into or tested inside shipping macOS, Windows or Linux builds. Existing application and website assets were not replaced.
