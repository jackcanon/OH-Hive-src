from pathlib import Path
import json, shutil
P=Path(__file__).resolve().parent.parent
OUTER='M325 100 490 238 750 238 945 100 985 354 927 478 1030 665 957 720 1000 900 840 1050 620 1160 385 1050 205 900 248 720 180 665 300 468 267 354Z'
SMALL=OUTER
EYES='M371 461 533 526 551 597 422 552Z M707 526 871 461 821 552 689 597Z'
NOSE='M549 732 691 732 726 771 644 849 597 849 514 771Z'
MOUTH='M441 863 475 904 620 858 765 904 799 863 779 930 620 891 461 930Z'
DEFS='''<defs>
<linearGradient id="copper" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#E58B5C"/><stop offset=".48" stop-color="#D26743"/><stop offset="1" stop-color="#B74627"/></linearGradient>
<linearGradient id="ember" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#D26743"/><stop offset="1" stop-color="#713B29"/></linearGradient>
<linearGradient id="cream" x1="0" y1="0" x2=".9" y2="1"><stop stop-color="#FFF6DF"/><stop offset="1" stop-color="#E2CBA2"/></linearGradient>
<linearGradient id="ground" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#293834"/><stop offset=".55" stop-color="#17201F"/><stop offset="1" stop-color="#101817"/></linearGradient>
<mask id="mono"><path d="'''+OUTER+'''" fill="white"/><path d="'''+EYES+' '+NOSE+' '+MOUTH+'''" fill="black"/></mask>
</defs>'''
def path(d,c):return f'<path d="{d}" fill="{c}"/>'
def wolf(mode='color',small=False):
 if mode not in ['color','flat']:
  c={'ink':'#17201F','cream':'#FFF6DF','copper':'#D26743','white':'#FFFFFF','black':'#000000'}[mode]
  return f'<rect x="150" y="60" width="920" height="1130" fill="{c}" mask="url(#mono)"/>'
 c='url(#copper)' if mode=='color' else '#D26743';e='url(#ember)' if mode=='color' else '#8E4D31';w='url(#cream)' if mode=='color' else '#FFF6DF'
 b=path(OUTER,c)
 regions=[
 ('M325 100 300 468 413 354 490 238Z',e),
 ('M325 100 267 354 300 468 350 314Z','#B74627'),
 ('M945 100 750 238 825 354 927 478Z','#B74627'),
 ('M945 100 927 478 985 354Z',e),
 ('M300 468 180 665 248 720 385 1050 352 764 422 552Z',e),
 ('M248 720 205 900 385 1050 352 764Z','#B74627'),
 ('M927 478 1030 665 957 720 840 1050 888 764 821 552Z',e),
 ('M957 720 1000 900 840 1050 888 764Z','#B74627'),
 ('M490 238 413 354 371 461 533 526 572 644 620 658 668 644 707 526 871 461 825 354 750 238Z',c),
 ('M490 238 620 282 533 526 371 461 413 354Z','#D9784C'),
 ('M620 282 750 238 825 354 871 461 707 526Z','#C35935'),
 ('M385 1050 422 552 551 597 572 644 475 790 441 863 469 1020 620 1160Z',w),
 ('M840 1050 821 552 689 597 668 644 765 790 799 863 771 1020 620 1160Z',w),
 ('M422 552 475 790 441 863 352 764Z','#E8CFA7'),
 ('M821 552 765 790 799 863 888 764Z','#E8CFA7'),
 ('M572 644 620 658 668 644 726 771 765 904 620 858 475 904 514 771Z',w),
 ('M461 930 620 891 779 930 755 1020 690 1070 550 1070 485 1020Z',w),
 ('M461 930 550 962 485 1020Z','#E2CBA2'),
 ('M779 930 690 962 755 1020Z','#D7B991'),
 ('M550 962 690 962 755 1020 485 1020Z','#F6E5C6'),
 ('M485 1020 755 1020 690 1070 550 1070Z','#B74627'),
 (EYES,'#17201F'),(NOSE,'#17201F'),(MOUTH,'#17201F')]
 if small:
  regions=[v for i,v in enumerate(regions) if i not in [1,3,5,7,9,10,13,14,17,18,19]]
 for d,col in regions:b+=path(d,col)
 if not small:
  b+=path('M442 507 475 522 483 546 452 534Z M766 522 799 507 789 534 758 546Z','#FFF6DF')
  b+=path('M456 892 479 909 474 931Z M761 909 784 892 766 931Z','#FFF6DF')
 return b

def svg(body,w=1024,h=1024,box=None):return f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="{box or f"0 0 {w} {h}"}">'+DEFS+body+'</svg>'
def save(name,s):f=P/name;f.parent.mkdir(parents=True,exist_ok=True);f.write_text(s)
for mode in ['color','flat','ink','cream','copper','white','black']:
 save(f'masters/fenrir-symbol-{mode}.svg',svg(wolf(mode),860,1070,'180 100 860 1070'))
outline=json.loads((P/'source/outlined-type.json').read_text())
for mode,c in [('color','#17201F'),('ink','#17201F'),('cream','#FFF6DF'),('white','#FFFFFF'),('black','#000000')]:
 symbol=wolf('color' if mode=='color' else mode)
 icon=f'<g transform="translate(-4 -1) scale(.085)">{symbol}</g>'
 for kind in ['wordmark','lockup']:
  ty=76 if kind=='wordmark' else 69
  text=f'<g fill="{c}" transform="translate(116 {ty}) scale(1 -1)">{outline[kind]}</g>'
  if kind=='lockup':text+=f'<g fill="{c}" transform="translate(119 99) scale(1 -1)">{outline["endorsement"]}</g>'
  save(f'masters/fenrir-{kind}-{mode}.svg',svg(icon+text,440,120 if kind=='lockup' else 110))
# Display tile includes safe padding; Apple full-bleed inputs deliberately omit the outer mask.
for variant,bg in [('dark','url(#ground)'),('light','#ECE5D8')]:
 for small in [False,True]:
  fg=f'<g transform="translate(74 44) scale(.72)">{wolf(small=small)}</g>'
  tile=f'<rect x="64" y="64" width="896" height="896" rx="198" fill="{bg}"/>'
  save(f'masters/fenrir-app-{variant}{"-small" if small else ""}.svg',svg(tile+fg))
 save(f'masters/fenrir-app-{variant}-fullbleed.svg',svg(f'<rect width="1024" height="1024" fill="{bg}"/><g transform="translate(74 44) scale(.72)">{wolf()}</g>'))
# Dedicated compact favicon keeps the wolf large at 16/24 px.
save('masters/fenrir-favicon.svg',svg('<rect width="1024" height="1024" rx="210" fill="#17201F"/><g transform="translate(43 12) scale(.77)">'+wolf(small=True)+'</g>'))
save('masters/fenrir-maskable.svg',svg('<rect width="1024" height="1024" fill="#17201F"/><g transform="translate(208 181) scale(.5)">'+wolf()+'</g>'))
# Native system layers, with identical origins and canvas sizes.
save('platforms/macos/composer-layers/background-dark.svg',svg('<rect width="1024" height="1024" fill="#17201F"/>'))
save('platforms/macos/composer-layers/background-light.svg',svg('<rect width="1024" height="1024" fill="#ECE5D8"/>'))
save('platforms/macos/composer-layers/wolf-color.svg',svg('<g transform="translate(74 44) scale(.72)">'+wolf()+'</g>'))
save('platforms/macos/composer-layers/wolf-mono.svg',svg('<g transform="translate(74 44) scale(.72)">'+wolf('white')+'</g>'))

save('palette.json',json.dumps({'ink':'#17201F','copper':'#D26743','ember':'#B74627','copperShadow':'#8E4D31','parchment':'#ECE5D8','cream':'#FFF6DF','note':'Gradients in dimensional masters add controlled highlight/shadow tones. Flat master uses solid brand fills.'},indent=2))
print('Vector master family built')
