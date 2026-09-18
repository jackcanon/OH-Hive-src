from pathlib import Path
import struct,json,hashlib,zipfile
from PIL import Image
R=Path(__file__).resolve().parent.parent

def ico(out,files):
    offset=6+16*len(files); entries=[]; blobs=[]
    for s,p in files:
        b=p.read_bytes();entries.append(struct.pack('<BBBBHHII',s%256,s%256,0,0,1,32,len(b),offset));blobs.append(b);offset+=len(b)
    out.write_bytes(struct.pack('<HHH',0,1,len(files))+b''.join(entries)+b''.join(blobs))
ico(R/'platforms/windows/Den.ico',[(s,R/f'platforms/windows/png/den-{s}.png') for s in [16,24,32,48,64,128,256]])
ico(R/'platforms/web/favicon.ico',[(s,R/f'platforms/web/favicon-{s}.png') for s in [16,32,48]])
records=[('icp4',16),('icp5',32),('icp6',64),('ic07',128),('ic08',256),('ic09',512),('ic10',1024),('ic11',32),('ic12',64),('ic13',256),('ic14',512)]
b=b''
for tag,s in records:
    # Use small logical artwork for Retina 16/32 representations.
    names={16:'icon_16x16.png',32:'icon_32x32.png',64:'icon_32x32@2x.png',128:'icon_128x128.png',256:'icon_256x256.png',512:'icon_512x512.png',1024:'icon_512x512@2x.png'}
    name={'ic11':'icon_16x16@2x.png','ic12':'icon_32x32@2x.png','ic13':'icon_128x128@2x.png','ic14':'icon_256x256@2x.png'}.get(tag,names[s])
    data=(R/'platforms/macos/Den.iconset'/name).read_bytes();b+=tag.encode()+struct.pack('>I',len(data)+8)+data
(R/'platforms/macos/Den.icns').write_bytes(b'icns'+struct.pack('>I',len(b)+8)+b)
checks=[]
for p in R.rglob('*.png'):
    with Image.open(p) as im: im.load();assert im.mode=='RGBA' or im.mode=='RGB'
checks.append('All PNG files decoded successfully.')
for p in R.rglob('*.ico'):
    im=Image.open(p)
    for size in im.ico.sizes():im.ico.getimage(size).load()
    checks.append(f'{p.relative_to(R)}: decoded sizes {sorted(im.ico.sizes())}')
im=Image.open(R/'platforms/macos/Den.icns')
for size in im.info['sizes']: im.icns.getimage(size).load()
checks.append('Den.icns: all representations decoded successfully.')
cat=json.loads((R/'platforms/macos/AppIcon.appiconset/Contents.json').read_text())
for x in cat['images']:
    im=Image.open(R/'platforms/macos/AppIcon.appiconset'/x['filename']);assert im.width==int(x['size'].split('x')[0])*int(x['scale'][0])
checks.append('All 10 Xcode app icon entries match their declared dimensions.')
for x in json.loads((R/'platforms/web/site.webmanifest').read_text())['icons']:
    assert (R/'platforms/web'/x['src']).exists()
checks.append('Web manifest icon references resolve.')
(R/'VERIFICATION.txt').write_text('\n'.join(checks)+'\nValidated export files, not installed app builds.\n')
print('\n'.join(checks))
