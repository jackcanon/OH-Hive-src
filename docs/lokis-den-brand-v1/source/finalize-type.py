from pathlib import Path
import re,json,shutil
p=Path('/Volumes/10TB JBOD/Agents/Claude/output/lokis-den-brand-v1');d=json.loads(Path('/tmp/den-outlines.json').read_text())
(p/'source/editable-type').mkdir(exist_ok=True)
for f in (p/'assets').glob('*.svg'):
 if not ('wordmark' in f.name or 'lockup' in f.name):continue
 s=f.read_text();(p/'source/editable-type'/f.name).write_text(s)
 def convert(m):
  a=m.group(1); text=m.group(2)
  x=re.search(r'x="([^"]+)"',a)[1];y=re.search(r'y="([^"]+)"',a)[1];color=re.search(r'fill="([^"]+)"',a)[1]
  key='endorsement' if text=='BY LOKI’S LAB' else ('wordmark' if 'wordmark' in f.name else 'lockup')
  return f'<g fill="{color}" transform="translate({x} {y}) scale(1 -1)">{d[key]}</g>'
 f.write_text(re.sub(r'<text ([^>]+)>(.*?)</text>',convert,s))
replacements={
'SVG wordmarks use live Georgia type; supplied PNGs fix the rendered appearance. Outline the type before third-party print production when an exact vector lockup is required.':'Production SVG wordmarks have outlined type and no font dependency. Live-type editable copies are included in source/editable-type.',
'Font files are not included. SVG lockups reference Georgia; PNG exports have no font dependency.':'Font files are not included. Production SVG lockups and PNG exports have no font dependency; editable live-type sources reference Georgia and Arial.',
'replace live type with outlines for print vendors, ':'',
'SVGs remain editable.':'SVGs use editable vector paths, including outlined lettering.',
'raised Den Surface, application spacing':'raised Den Surface and slightly deeper Den Iron, application spacing',
'Do not bake rounded corners into assets':'Do not bake rounded corners into assets'
}
for name in ['BRAND-GUIDE.md','brand-guide.html','source/build-guide.py']:
 f=p/name;s=f.read_text()
 for a,b in replacements.items():s=s.replace(a,b)
 f.write_text(s)
shutil.copy('/tmp/den_outline.swift',p/'source/outline-type.swift');shutil.copy('/tmp/den_finalize.py',p/'source/finalize-type.py')
(p/'icons/web/install.html').write_text('''<!-- Place the web icon files together at your chosen public path. Adjust these paths to match. -->
<link rel="icon" href="/icons/favicon.svg" type="image/svg+xml">
<link rel="icon" href="/icons/favicon.ico" sizes="any">
<link rel="apple-touch-icon" href="/icons/apple-touch-icon.png">
<link rel="manifest" href="/icons/site.webmanifest">
<meta name="theme-color" content="#17201F">
''')
(p/'README.md').write_text('''# Loki’s Den — brand and icon pack v1

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
''')
print('Outlined production logos and documentation complete')
