from pathlib import Path
import json, shutil
P=Path(__file__).resolve().parent.parent
OUTER='M345 105 278 319 339 458 190 748 557 1145 590 1031 621 1062 597 1161 904 957 942 783 1028 636 963 456 1003 309 975 104 770 249 535 231Z'
SMALL='M345 105 278 319 339 458 190 748 597 1161 904 957 942 783 1028 636 963 456 1003 309 975 104 770 249 535 231Z'
DEFS='''<defs>
<linearGradient id="copper" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#E58B5C"/><stop offset=".48" stop-color="#D26743"/><stop offset="1" stop-color="#B74627"/></linearGradient>
<linearGradient id="ember" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#D26743"/><stop offset="1" stop-color="#713B29"/></linearGradient>
<linearGradient id="cream" x1="0" y1="0" x2=".9" y2="1"><stop stop-color="#FFF6DF"/><stop offset="1" stop-color="#E2CBA2"/></linearGradient>
<linearGradient id="ground" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#293834"/><stop offset=".55" stop-color="#17201F"/><stop offset="1" stop-color="#101817"/></linearGradient>
<mask id="mono"><path d="'''+OUTER+'''" fill="white"/><path d="M840 532 945 475 917 539 839 596Z M783 763 842 799 928 784 882 858 901 875 751 887 832 855Z" fill="black"/></mask>
</defs>'''
def path(d,c):return f'<path d="{d}" fill="{c}"/>'
def wolf(mode='color',small=False):
 if mode not in ['color','flat']:
  c={'ink':'#17201F','cream':'#FFF6DF','copper':'#D26743','white':'#FFFFFF','black':'#000000'}[mode]
  return f'<rect x="150" y="60" width="920" height="1130" fill="{c}" mask="url(#mono)"/>'
 c='url(#copper)' if mode=='color' else '#D26743';e='url(#ember)' if mode=='color' else '#8E4D31';w='url(#cream)' if mode=='color' else '#FFF6DF'
 b=path(SMALL if small else OUTER,c)
 regions=[('M345 105 332 331 408 331 535 231Z',e),('M278 319 339 458 408 331 345 105Z','#B74627'),('M339 458 190 748 557 1145 590 1031 371 757 391 555Z',e),('M391 555 371 757 590 1031 621 1062 597 1161 904 957 754 925 540 762Z',e),('M975 104 906 367 963 456 1003 309Z',e),('M770 249 975 104 906 367 904 381Z',c),('M408 331 535 231 770 249 904 381 832 608 702 655 624 530 621 464Z',c),('M408 331 391 555 540 762 624 530 621 464Z','#B74627'),('M624 530 702 655 754 925 619 806 540 762Z',w),('M904 381 963 456 1028 636 942 783 927 749 951 640 945 475 840 532 832 608Z',c),('M945 475 951 640 927 749 942 783 906 874 882 858 928 784 900 752 832 608 840 532Z',w),('M702 655 832 608 900 752 783 763 782 800 832 855 751 887 901 875 906 874 884 910 754 925Z',w),('M840 532 945 475 917 539 839 596Z','#17201F'),('M872 531 891 518 880 545 859 555Z','#FFF6DF'),('M783 763 900 752 928 784 882 858 901 875 751 887 832 855 782 800Z','#17201F')]
 if small:
  regions=[regions[i] for i in [0,2,3,4,6,8,10,11,12,14]]
 for d,col in regions:b+=path(d,col)
 return b

def svg(body,w=1024,h=1024,box=None):return f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="{box or f"0 0 {w} {h}"}">'+DEFS+body+'</svg>'
def save(name,s):f=P/name;f.parent.mkdir(parents=True,exist_ok=True);f.write_text(s)
for mode in ['color','flat','ink','cream','copper','white','black']:
 save(f'masters/fenrir-symbol-{mode}.svg',svg(wolf(mode),860,1070,'180 100 860 1070'))
outline=json.loads(Path('/tmp/den-outlines.json').read_text())
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
shutil.copy('/tmp/den-outlines.json',P/'source/outlined-type.json')
save('palette.json',json.dumps({'ink':'#17201F','copper':'#D26743','ember':'#B74627','copperShadow':'#8E4D31','parchment':'#ECE5D8','cream':'#FFF6DF','note':'Gradients in dimensional masters add controlled highlight/shadow tones. Flat master uses solid brand fills.'},indent=2))
print('Vector master family built')
