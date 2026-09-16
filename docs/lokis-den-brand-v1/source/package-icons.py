from pathlib import Path
from PIL import Image
import struct, shutil
p=Path('/Volumes/10TB JBOD/Agents/Claude/output/lokis-den-brand-v1')
# Preserve individually rendered small-size artwork in ICO containers.
def ico(target,sizes):
 target.parent.mkdir(parents=True,exist_ok=True)
 blobs=[(p/f'icons/linux/den-{s}.png').read_bytes() for s in sizes]
 offset=6+16*len(sizes);header=struct.pack('<HHH',0,1,len(sizes)); entries=[]
 for size,data in zip(sizes,blobs):
  entries.append(struct.pack('<BBBBHHII',size%256,size%256,0,0,1,32,len(data),offset));offset+=len(data)
 target.write_bytes(header+b''.join(entries)+b''.join(blobs))
ico(p/'icons/windows/Den.ico',[16,24,32,48,64,128,256]);ico(p/'icons/web/favicon.ico',[16,32,48])
# Native ICNS container using its documented PNG-bearing size records.
records=[('icp4',16),('icp5',32),('icp6',64),('ic07',128),('ic08',256),('ic09',512),('ic10',1024)]
body=b''
for tag,size in records:
 data=(p/f'icons/linux/den-{size}.png').read_bytes();body+=tag.encode()+struct.pack('>I',len(data)+8)+data
(p/'icons/macos/Den.icns').write_bytes(b'icns'+struct.pack('>I',len(body)+8)+body)
# Check every PNG and each container representation using an independent decoder.
n=0
for f in p.rglob('*.png'):
 im=Image.open(f);im.load();assert im.mode in ['RGBA','RGB'];n+=1
for f in [p/'icons/windows/Den.ico',p/'icons/web/favicon.ico']:
 im=Image.open(f)
 for size in im.ico.sizes(): im.ico.getimage(size).load()
im=Image.open(p/'icons/macos/Den.icns')
for size in im.info['sizes']:
 frame=im.icns.getimage(size); frame.load()
shutil.copy('/tmp/build_den_brand.py',p/'source/build-guide.py')
shutil.copy('/tmp/finish_den.py',p/'source/package-icons.py')
print(f'Validated {n} PNGs, ICO frames, and ICNS representations')
