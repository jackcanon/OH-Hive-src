from pathlib import Path
import json, html, re
p=Path('/Volumes/10TB JBOD/Agents/Claude/output/lokis-den-brand-v1')
# A continuous squared arch / D with an inset L. Flat, no inherited logo altered.
outer='M18 12H57L82 37V75L69 88H18V12ZM31 25V75H64L69 70V42L52 25H31Z'
inner='M40 37H50V60H61V70H40V37Z'
def mark(color): return f'<path fill="{color}" fill-rule="evenodd" d="{outer}"/><path fill="{color}" d="{inner}"/>'
def svg(body,w=100,h=100): return f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">{body}</svg>'
colors={'copper':'#D26743','ink':'#17201F','cream':'#FFF6DF','white':'#FFFFFF','black':'#000000'}
for name,col in colors.items():
 (p/f'assets/den-symbol-{name}.svg').write_text(svg(mark(col)))
 (p/f'assets/den-wordmark-{name}.svg').write_text(svg(f'<g transform="translate(0 5)">{mark(col)}</g><text x="120" y="76" fill="{col}" font-family="Georgia,serif" font-size="62" letter-spacing="-2">Loki’s Den</text>',440,110))
 (p/f'assets/den-lockup-{name}.svg').write_text(svg(f'<g transform="translate(0 5)">{mark(col)}</g><text x="120" y="69" fill="{col}" font-family="Georgia,serif" font-size="58" letter-spacing="-2">Loki’s Den</text><text x="123" y="99" fill="{col}" font-family="Arial,sans-serif" font-size="16" letter-spacing="1.5">BY LOKI’S LAB</text>',440,120))
for variant,bg,fg in [('dark','#17201F','#D26743'),('light','#ECE5D8','#B74627'),('copper','#D26743','#17201F')]:
 (p/f'assets/den-app-{variant}.svg').write_text(svg(f'<rect width="1024" height="1024" rx="224" fill="{bg}"/><g transform="translate(148 148) scale(7.28)">{mark(fg)}</g>',1024,1024))
 (p/f'assets/den-app-{variant}-fullbleed.svg').write_text(svg(f'<rect width="1024" height="1024" fill="{bg}"/><g transform="translate(148 148) scale(7.28)">{mark(fg)}</g>',1024,1024))
# Small-size variant removes inset L, preserving the doorway silhouette.
(p/'assets/den-favicon.svg').write_text(svg('<rect width="32" height="32" rx="7" fill="#17201F"/><path fill="#D26743" fill-rule="evenodd" d="M7 5H18L26 13V25L22 28H7V5ZM11 9V24H20L22 22V15L16 9H11Z"/>',32,32))
(p/'assets/den-maskable.svg').write_text(svg(f'<rect width="1024" height="1024" fill="#17201F"/><g transform="translate(212 212) scale(6)">{mark("#D26743")}</g>',1024,1024))
md='''# Loki’s Den
## Brand guide • Version 1.0 • September 15, 2026

A home for your AI.

**Direction for review.** Loki’s Den is the name selected for this guide; Jack reports purchasing www.lokisden.app. The visual identity and messaging below are proposed v1 assets. They have not been applied to the shipping app or websites.

## 01 — The idea

Loki’s Den makes working with AI feel like settling into a workspace you know. It brings local and cloud agents together around your projects, with a clear view of which computers and services are involved. Setup should feel approachable to someone attending Office Hours, while leaving room for an experienced builder to go deeper.

**Promise:** Bring your AI together. Make something useful.

**Primary line:** A home for your AI.

**Functional descriptor:** Your workspace for local and cloud AI.

**Positioning:** For people who want useful AI without assembling a stack of tools, Loki’s Den is a personal workspace that brings agents, computers, and projects together. It is designed to be useful on its own, with an optional connection to a community Hive.

This describes the intended product. Release marketing must distinguish available features from previews. Ease of setup is a design commitment, not a measured setup-time claim.

## 02 — One family, distinct roles

| Brand | Role | Visual signature | How to describe it |
|---|---|---|---|
| Loki’s Lab | Maker, publication, experiments and education | Parchment, ink, double-L mark, editorial serif | The workbench where ideas are tested and explained. |
| Loki’s Den | Personal application and workspace | Ink, warm copper, doorway D/L symbol, spacious surfaces | Your agents, your computers, your projects. |
| The Hive | Optional community compute network | Established charcoal and honey-gold treatment | Work with a community and contribute spare compute. |
| Honey | Existing Hive currency | Existing Honey symbol and gold | Use the approved Honey identity beside balances and transactions. |

The Den should work as a complete personal product without joining a Hive. Office Hours can be one community; avoid presenting it as the only Hive. Halo keeps its separate pooled-inference meaning and is not a synonym for the Den.

**Family sentence:** Loki’s Den, by Loki’s Lab. Connect to the Hive when you want to work with a community.

## 03 — Names and addresses

- Public product name: **Loki’s Den**. Use the curly apostrophe in designed copy; a straight apostrophe is acceptable in plain text.
- Friendly short form: **the Den**, after the full name appears. Capitalize Den when referring to the product.
- App bundle requested by Jack: **Den.app**. Store listings and marketing should lead with Loki’s Den so the maker is recognizable.
- Maker endorsement: **by Loki’s Lab**. Use this on landing pages, About screens, onboarding and press materials. It need not repeat in every toolbar.
- Purchased address, as supplied by Jack: **www.lokisden.app**. Recommended short display address: **lokisden.app**, once apex and www routing are configured and checked.
- Parent address: **lokislab.org**. Lab articles link to the Den product site; the Den credits the Lab in its footer and About screen.
- Use **The Hive** as the named community destination; use **a Hive** when discussing any community instance.
- Avoid “Loki’s Lab x Loki’s Den” as the main lockup: the product is made by the Lab. Reserve x for a real collaboration.

## 04 — Personality and voice

**Warm. Capable. Curious. Plainspoken.** Keep the Lab’s honesty and useful curiosity; speak as a helpful teammate who gets the workspace ready. A little dry wit belongs in welcome copy, never in an error that blocks someone’s work.

| Situation | Recommended copy |
|---|---|
| Welcome | Welcome to your Den. Let’s add your first agent. |
| Empty project list | What would you like to work on? |
| Computer setup | Add a computer |
| Local agent label | Runs on this Mac |
| Cloud agent label | Uses your cloud provider |
| Community introduction | Your Den is ready. You can also join a Hive to work with a community. |
| Contribution control | Share spare compute |
| Connection failure | We couldn’t reach this computer. Check that it’s online, then try again. |

Use “agents,” “computers,” “projects,” and “community” in everyday navigation. Explain “local” as running on a computer you control. Keep “harness,” “inference backend,” and “orchestration” in technical documentation.

Do not imply all work stays on-device when a cloud agent is selected. Describe where a task runs and what the user is choosing. Avoid “unlimited,” “free forever,” or “works with everything” without evidence. Avoid mythology jokes, horns, weapons, and franchise references in core branding.

## 05 — The mark: an open workspace

The proposed symbol combines a squared doorway / capital D with a small inset L. Its cut corner and solid strokes echo the Lab’s carved geometry. The L hints at Loki and the maker relationship; the doorway suggests a place to enter and work.

The mark is intentionally independent of the Lab’s double-L. Use the original Lab mark only as a separate maker endorsement. Do not combine it with the Honey symbol or place a bee inside the Den mark.

**Primary application:** copper symbol on ink. **Light application:** ember symbol on parchment. **Monochrome:** one solid color, supplied in ink, cream, white and black.

- Clear space: at least 13 units on a 100-unit symbol grid, measured from the visible mark. Apply the same gap between symbol and wordmark.
- Standard symbol minimum: 32 px high. At 16–24 px use the supplied simplified favicon, which omits the inset L.
- Wordmark minimum: 180 px wide. Endorsed lockup minimum: 240 px wide. Below that, omit the maker line and show it elsewhere.
- Keep the proportions, open counter, cut corner, and orientation intact.
- Use flat fills. Avoid bevels, metallic effects, glows, drop shadows attached to the mark, or patterns through its strokes.
- SVG wordmarks use live Georgia type; supplied PNGs fix the rendered appearance. Outline the type before third-party print production when an exact vector lockup is required.

The symbol is original artwork created for this concept; distinctiveness and trademark clearance are separate from this design exercise.

## 06 — Color system

| Token | Hex | Role |
|---|---|---|
| Den Ink | #17201F | Primary dark ground and light-mode text; inherited from Lab |
| Den Surface | #222D2B | New raised dark panels |
| Parchment | #ECE5D8 | Light-mode ground; inherited from Lab |
| Paper | #F7F3EB | Light cards; inherited from Lab |
| Den Copper | #D26743 | Primary dark-mode action and mark; drawn from the Lab favicon |
| Ember | #B74627 | Light-mode action and mark; inherited from Lab guide |
| Warm Cream | #FFF6DF | Dark-mode text; inherited from Hive/Honey |
| Soft Stone | #C7C0B4 | Secondary dark-mode text |
| Den Iron | #5E6662 | Slightly deepened Lab Iron for secondary text on parchment |
| Honey Gold | #F5B82E | Hive/community and Honey accents; inherited, unchanged |
| Hive Charcoal | #1D1C20 | Existing Hive surfaces; inherited, unchanged |

**Balance:** let neutrals occupy about 85% of a typical screen. Use copper for the main personal action; reserve gold for community and Honey touchpoints. These proportions are composition guidance, not a quota.

**Dark buttons:** Den Copper with Den Ink text. **Light buttons:** Ember with white text. Do not put white small text on Den Copper. Use Ink text on gold. On light surfaces, use a dark honey link color (#8A6100), as the Hive already does.

Color never communicates status by itself. Pair contribution activity with text such as “Sharing compute” and an icon. Keep brand copper separate from error styling. Use labeled green success and red error states with appropriate contrast.

## 07 — Typography

**Marketing and wordmark:** Georgia Regular, inherited from the Lab’s current serif direction. Warm, confident headlines with sentence case. Avoid heavy distressing or faux runes.

**Interface and body:** Geist Sans for the web where bundled; native system sans-serif in the app is an acceptable platform adaptation. The preview uses system sans so it opens without a network connection.

**Data and technical details:** Geist Mono where bundled; native monospace fallback for model names, timings and diagnostics. Keep this out of ordinary welcome and navigation copy.

- Landing headline: 48–72 px desktop; 36–44 px mobile; line height 1.05–1.15.
- Section heading: 28–36 px, serif in marketing and sans in the application.
- Body: 16–18 px, line height 1.5–1.65. Keep paragraphs around 55–75 characters wide.
- Controls: 14–16 px, medium weight. Small labels: 12–13 px; never use them for essential instructions.
- Use uppercase tracking for short labels only. Preserve readable type size under zoom and system accessibility settings.

Font files are not included. SVG lockups reference Georgia; PNG exports have no font dependency.

## 08 — Layout, shape and imagery

Carry forward the Lab’s strong alignment and generous margins. Give the Den fewer competing panels, larger breathing spaces, and one clear next action. Use an 8 px spacing rhythm, 12 px card corners, and 8 px control corners. Prefer thin rules to heavy shadows.

The Den’s shape language is a doorway, a frame, and a purposeful cut corner. Use the cut corner occasionally on a hero illustration or feature frame; keep everyday cards simple. Do not turn every panel into a logo silhouette.

Show real desks, familiar computers, project artifacts, and people making something. Warm side lighting and honest materials fit the identity. Avoid neon AI brains, fantasy taverns, server-room intimidation, and honeycomb wallpaper across personal screens.

Use brief 150–200 ms transitions and honor reduced-motion preferences. No pulsing brand marks, simulated activity, or decorative “thinking” animations that imply work is happening.

## 09 — The Hive inside the Den

Keep the Den’s navigation and identity visible while a community panel uses the existing Hive charcoal and gold. This makes the destination recognizable without making the app feel like an unrelated product.

- Personal sidebar: Projects, Agents, My Computers, The Hive, Settings.
- Personal primary action: copper in dark mode, Ember in light mode.
- Community action: gold with charcoal text, alongside the community’s name.
- Gold beside Honey amounts must use the existing Honey symbol, not the Den D/L.
- Joining and compute contribution are separate choices. A joined community does not imply that sharing is enabled.
- Label task location directly: This computer, My computers, Cloud provider, or the named community when applicable.

**Transition line:** Set up your Den. Connect to the Hive.

This section proposes design behavior. It does not claim that all listed controls have shipped.

## 10 — Ready-to-use messaging

**Homepage headline:** A home for your AI.

**Homepage introduction:** Bring your local and cloud agents together in one familiar workspace. Build around the computers you own, the services you choose, and the projects you want to finish.

**Community section:** Open your Den to a bigger idea. Join a Hive to work with a community and choose when to contribute spare compute.

**Short description:** Loki’s Den is a personal workspace for local and cloud AI, made by Loki’s Lab.

**About line:** Made by Loki’s Lab. Built around useful curiosity.

**Launch CTA:** use “Get Loki’s Den” only when a working download is available; use “Follow development” when linking to current project updates. Do not show an inactive download as a finished release.

## 11 — App and icon delivery

The pack includes rounded transparent application artwork, square full-bleed sources, platform exports, and a simplified small-size mark. The default app icon is copper on Ink; light and copper-background alternatives are included as design variants.

- macOS: Den.icns plus its standard and Retina iconset. Rounded artwork is baked into this traditional icon, with transparent corners.
- Apple asset catalog: AppIcon.appiconset with 16–1024 px representations for macOS. Modern layered Icon Composer artwork is not included.
- Windows: Den.ico with 16, 24, 32, 48, 64, 128 and 256 px frames.
- Linux: PNG sizes 16–1024 and a scalable SVG, ready for placement in the appropriate hicolor directories.
- Web: favicon.svg, multi-size favicon.ico, 180 px Apple touch icon, 192/512 px application icons, maskable 512 px icon, and a starter web manifest.
- Transparent symbol, wordmark and endorsed-lockup PNGs: 512, 1024 and 2048 px widths; five colorways each. SVGs remain editable.

Do not bake rounded corners into assets when a target platform applies its own mask; use the full-bleed or maskable source. Keep the supplied maskable icon’s safe inset. The web manifest is a starter asset, not a complete installable PWA.

## 12 — Source lineage and release checks

**Established sources reviewed:** Loki’s Lab docs/BRAND-GUIDE.md, app/globals.css, public/favicon.svg; Hive apps/web/app/globals.css; Honey docs/honey-logo-package/README.md and existing symbol masters. All were read from the current local checkouts. Honey’s package is marked approved; the Den design is a new proposal. The Lab’s actual favicon uses #D26743 alongside the guide’s #B74627, which is why both have explicit roles here.

**Inherited:** ink, parchment, paper, Ember, copper from the favicon, serif/sans pairing, plainspoken evidence-first voice; Hive’s gold, charcoal, cream and Honey mark remain unchanged.

**New:** D/L doorway mark, personal workspace positioning, raised Den Surface, application spacing and copper-versus-gold family rules.

**Before public rollout:** confirm the visual direction, configure and verify the purchased domain, replace live type with outlines for print vendors, check final platform builds and store crops, and review marketing against the actual release. This pack does not rename existing code, alter the survey, publish a website, or validate domain routing.
'''
(p/'BRAND-GUIDE.md').write_text(md)
# Small deterministic Markdown renderer for this limited guide syntax.
def inline(s):
 s=html.escape(s)
 return re.sub(r'\*\*(.*?)\*\*',r'<strong>\1</strong>',s)
lines=md.splitlines(); out=[]; ul=False; table=False
for line in lines:
 if not line.startswith('- ') and ul: out.append('</ul>'); ul=False
 if not line.startswith('|') and table: out.append('</tbody></table></div>'); table=False
 if line.startswith('|'):
  cells=[x.strip() for x in line.strip('|').split('|')]
  if all(re.fullmatch(r'[-: ]+',x) for x in cells): continue
  if not table:
   out.append('<div class="table-wrap"><table><thead><tr>'+''.join('<th>'+inline(x)+'</th>' for x in cells)+'</tr></thead><tbody>');table=True
  else: out.append('<tr>'+''.join('<td>'+inline(x)+'</td>' for x in cells)+'</tr>')
 elif line.startswith('- '):
  if not ul: out.append('<ul>');ul=True
  out.append('<li>'+inline(line[2:])+'</li>')
 elif line.startswith('## '): out.append('<h2>'+inline(line[3:])+'</h2>')
 elif line.startswith('# '): pass
 elif line: out.append('<p>'+inline(line)+'</p>')
css='''*{box-sizing:border-box}body{margin:0;background:#ECE5D8;color:#17201F;font:17px/1.65 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}header{background:#17201F;color:#FFF6DF;padding:60px max(6vw,24px)}.eyebrow{font:12px/1.4 monospace;letter-spacing:.16em;text-transform:uppercase;color:#C7C0B4}h1{font:clamp(48px,7vw,92px)/1.04 Georgia,serif;margin:28px 0}h2{font:32px/1.2 Georgia,serif;margin-top:64px;border-top:1px solid #AAA194;padding-top:30px}h3{font:25px Georgia,serif}main{max-width:1120px;margin:auto;padding:20px 32px 70px}p{max-width:78ch}a{color:inherit}strong{font-weight:650}.hero{display:flex;gap:50px;align-items:center}.hero img{width:210px;max-width:25vw}.hero p{color:#C7C0B4;font-size:20px}.grid{display:grid;grid-template-columns:repeat(3,1fr);gap:18px;margin:26px 0}.tile{padding:28px;background:#F7F3EB;border-radius:12px}.dark{background:#17201F;color:#FFF6DF}.community{background:#1D1C20;color:#FFF6DF}.tag{display:inline-block;padding:9px 15px;border-radius:8px;background:#D26743;color:#17201F;font-size:14px;font-weight:650}.gold{background:#F5B82E;color:#1D1C20}.swatches{display:flex;flex-wrap:wrap;gap:12px}.swatch{width:150px;padding:16px;border-radius:8px;font-size:13px}.table-wrap{overflow:auto}table{width:100%;border-collapse:collapse;font-size:15px;margin:20px 0}th,td{text-align:left;border-bottom:1px solid #AAA194;padding:14px 12px;vertical-align:top}th{background:#F7F3EB}li{margin:9px 0}.lockup{width:100%;max-width:400px;display:block;margin:26px 0}.note{font-size:13px;color:#626A66}.sizes{display:flex;align-items:center;gap:22px;flex-wrap:wrap}.sizes span{display:flex;align-items:center;gap:8px;font-size:12px}.hero-label{color:#D26743}.family{font:32px Georgia,serif}.preview{border:1px solid #3D4744;display:grid;grid-template-columns:180px 1fr;border-radius:14px;overflow:hidden;background:#17201F;color:#FFF6DF}.sidebar{padding:25px;background:#222D2B}.sidebar p{font-size:14px;color:#C7C0B4;margin:20px 0}.workspace{padding:32px}.workspace h3{font-size:34px;margin-top:0}.row{padding:16px 0;border-top:1px solid #3D4744}.row small{color:#C7C0B4}.community-panel{padding:20px;background:#1D1C20;border-left:3px solid #F5B82E;margin-top:24px}.preview-label{font:12px monospace;color:#626A66;margin-bottom:12px}@media(max-width:700px){.grid{grid-template-columns:1fr}.hero{gap:18px}.hero img{width:90px}header{padding:35px 24px}main{padding:12px 20px}h2{font-size:28px}.preview{grid-template-columns:1fr}.sidebar{display:none}.workspace{padding:22px}}@media print{header{padding:25px}h1{font-size:48px}h2{break-after:avoid;margin-top:28px}tr,.tile,.preview{break-inside:avoid}body{font-size:11pt}main{padding:0}.grid{gap:8px}*{print-color-adjust:exact;-webkit-print-color-adjust:exact}}'''
visual='''<h2>The family at a glance</h2><div class="grid"><div class="tile"><div class="eyebrow" style="color:#626A66">The maker</div><p class="family">Loki’s Lab</p><p>Useful curiosity.<br>Test. Explain. Share.</p></div><div class="tile dark"><div class="eyebrow">Your workspace</div><p class="family">Loki’s Den</p><p>A home for your AI.</p><span class="tag">Personal · Copper</span></div><div class="tile community"><div class="eyebrow">Your community</div><p class="family">The Hive</p><p>Work together.<br>Share spare compute.</p><span class="tag gold">Community · Gold</span></div></div>
<h2>Identity in use</h2><div class="grid"><div class="tile dark"><img class="lockup" src="assets/den-lockup-cream.svg" alt="Loki’s Den by Loki’s Lab cream logo"><img src="assets/den-app-dark.svg" width="110" alt="Dark app icon"></div><div class="tile"><img class="lockup" src="assets/den-lockup-ink.svg" alt="Ink logo"><img src="assets/den-app-light.svg" width="110" alt="Light app icon"></div><div class="tile" style="background:#D26743"><img class="lockup" src="assets/den-lockup-ink.svg" alt="Ink logo on copper"><img src="assets/den-app-copper.svg" width="110" alt="Copper app icon"></div></div>
<div class="sizes"><span><img src="assets/den-favicon.svg" width="16" height="16" alt="16 pixel simplified mark">16</span><span><img src="assets/den-favicon.svg" width="24" height="24" alt="24 pixel simplified mark">24</span><span><img src="assets/den-app-dark.svg" width="32" height="32" alt="32 pixel icon">32</span><span><img src="assets/den-app-dark.svg" width="64" height="64" alt="64 pixel icon">64</span><span><img src="assets/den-app-dark.svg" width="128" height="128" alt="128 pixel icon">128</span></div>
<h2>A familiar place to work</h2><p class="preview-label">ILLUSTRATIVE APPLICATION · Proposed design, sample data</p><div class="preview"><aside class="sidebar"><img src="assets/den-wordmark-cream.svg" width="140" alt="Loki’s Den"><p>Projects</p><p>Agents</p><p>My Computers</p><p style="color:#F5B82E">The Hive</p><p>Settings</p></aside><div class="workspace"><h3>What are we making?</h3><p style="color:#C7C0B4">Your agents and projects, together in your Den.</p><span class="tag">New project</span><div class="row" style="margin-top:25px">Office Hours research<br><small>Writing assistant · Runs on this Mac</small></div><div class="row">Website ideas<br><small>Cloud assistant · Uses your cloud provider</small></div><div class="community-panel"><b style="color:#F5B82E">The Hive</b><p>Work with a community. Choose when to share spare compute.</p><span class="tag gold">Explore communities</span><p style="font-size:13px;color:#C7C0B4">Compute sharing is off.</p></div></div></div>'''
swatches='<h2>The palette</h2><div class="swatches">'+''.join(f'<div class="swatch" style="background:{v};color:{t}">{n}<br>{v}</div>' for n,v,t in [('Ink','#17201F','#FFF6DF'),('Copper','#D26743','#17201F'),('Parchment','#ECE5D8','#17201F'),('Ember','#B74627','#FFFFFF'),('Honey Gold','#F5B82E','#1D1C20'),('Warm Cream','#FFF6DF','#17201F')])+'</div>'
(p/'brand-guide.html').write_text('<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Loki’s Den — Brand Guide v1</title><style>'+css+'</style><header><div class="eyebrow">Loki’s Lab / Product identity / v1 for review</div><div class="hero"><img src="assets/den-app-dark.svg" alt="Loki’s Den icon"><div><h1>Loki’s Den</h1><p>A home for your AI.</p><div class="eyebrow hero-label">lokisden.app · by Loki’s Lab</div></div></div></header><main>'+visual+swatches+''.join(out)+'<p class="note">Prepared by Sif your friendly Codex Agent · September 15, 2026</p></main></html>')
# Portable web/app design tokens, no application integration.
tokens={'dark':{'bg':'#17201F','surface':'#222D2B','text':'#FFF6DF','muted':'#C7C0B4','action':'#D26743','actionText':'#17201F','community':'#F5B82E','communityText':'#1D1C20'},'light':{'bg':'#ECE5D8','surface':'#F7F3EB','text':'#17201F','muted':'#5E6662','action':'#B74627','actionText':'#FFFFFF','community':'#F5B82E','communityText':'#1D1C20'}}
(p/'design-tokens.json').write_text(json.dumps(tokens,indent=2)+'\n')
def lum(h):
 c=[int(h[i:i+2],16)/255 for i in (1,3,5)]; c=[v/12.92 if v<=.04045 else ((v+.055)/1.055)**2.4 for v in c];return .2126*c[0]+.7152*c[1]+.0722*c[2]
pairs=[('Cream on Ink','#FFF6DF','#17201F'),('Stone on Surface','#C7C0B4','#222D2B'),('Ink on Copper','#17201F','#D26743'),('White on Ember','#FFFFFF','#B74627'),('Iron on Paper','#626A66','#F7F3EB'),('Den Iron on Parchment','#5E6662','#ECE5D8'),('Charcoal on Gold','#1D1C20','#F5B82E')]
results=[]
for name,a,b in pairs:
 x,y=sorted([lum(a),lum(b)]);ratio=(y+.05)/(x+.05); assert ratio>=4.5,(name,ratio);results.append(f'- {name}: {ratio:.2f}:1')
(p/'CONTRAST-CHECK.md').write_text('# Text contrast checks\n\nComputed from sRGB relative luminance. All listed pairs exceed 4.5:1. This is a palette check, not a full application accessibility audit.\n\n'+'\n'.join(results)+'\n')
print('Guide, vector assets and palette checks written:',p)
