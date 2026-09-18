const fs=require('fs'),path=require('path');
const sharp=require('/Users/dit1/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/sharp');
const root=path.resolve(__dirname,'..');
async function png(src,dst,size){const dest=path.join(root,dst);fs.mkdirSync(path.dirname(dest),{recursive:true});await sharp(path.join(root,src)).resize(size).png().toFile(dest);}
(async()=>{
for(const f of fs.readdirSync(path.join(root,'masters')).filter(x=>x.endsWith('.svg'))){for(const s of (f.includes('symbol')||f.includes('wordmark')||f.includes('lockup')?[512,1024,2048]:[1024]))await png('masters/'+f,'png/'+f.replace('.svg','-'+s+'.png'),s);}
const icon=s=>s<=32?'masters/fenrir-app-dark-small.svg':'masters/fenrir-app-dark.svg';
for(const s of [16,20,24,32,40,44,48,64,71,96,128,150,256,310,512,1024])await png(icon(s),`platforms/windows/png/den-${s}.png`,s);
for(const s of [16,24,32,48,64,128,256,512,1024])await png(icon(s),`platforms/linux/hicolor/${s}x${s}/apps/lokis-den.png`,s);
fs.mkdirSync(path.join(root,'platforms/linux/hicolor/scalable/apps'),{recursive:true});fs.copyFileSync(path.join(root,'masters/fenrir-app-dark.svg'),path.join(root,'platforms/linux/hicolor/scalable/apps/lokis-den.svg'));
let cat={images:[],info:{author:'xcode',version:1}};
for(const s of [16,32,128,256,512])for(const scale of [1,2]){
 const name=`icon_${s}x${s}${scale==2?'@2x':''}.png`;await png(icon(s),`platforms/macos/Den.iconset/${name}`,s*scale);
 const dir=path.join(root,'platforms/macos/AppIcon.appiconset');fs.mkdirSync(dir,{recursive:true});fs.copyFileSync(path.join(root,'platforms/macos/Den.iconset',name),path.join(dir,name));cat.images.push({idiom:'mac',size:`${s}x${s}`,scale:`${scale}x`,filename:name});}
fs.writeFileSync(path.join(root,'platforms/macos/AppIcon.appiconset/Contents.json'),JSON.stringify(cat,null,2));
for(const name of ['background-dark','background-light','wolf-color','wolf-mono'])await png(`platforms/macos/composer-layers/${name}.svg`,`platforms/macos/composer-layers/${name}.png`,1024);
for(const [file,name,s]of [['fenrir-favicon','favicon-16',16],['fenrir-favicon','favicon-32',32],['fenrir-favicon','favicon-48',48],['fenrir-app-dark-fullbleed','apple-touch-icon',180],['fenrir-app-dark-fullbleed','icon-192',192],['fenrir-app-dark-fullbleed','icon-512',512],['fenrir-maskable','maskable-192',192],['fenrir-maskable','maskable-512',512]])await png(`masters/${file}.svg`,`platforms/web/${name}.png`,s);
fs.copyFileSync(path.join(root,'masters/fenrir-favicon.svg'),path.join(root,'platforms/web/favicon.svg'));
fs.writeFileSync(path.join(root,'platforms/web/site.webmanifest'),JSON.stringify({name:'Loki’s Den',short_name:'Den',start_url:'/',scope:'/',display:'standalone',background_color:'#17201F',theme_color:'#17201F',icons:[{src:'icon-192.png',sizes:'192x192',type:'image/png',purpose:'any'},{src:'icon-512.png',sizes:'512x512',type:'image/png',purpose:'any'},{src:'maskable-192.png',sizes:'192x192',type:'image/png',purpose:'maskable'},{src:'maskable-512.png',sizes:'512x512',type:'image/png',purpose:'maskable'}]},null,2));
console.log('Raster/platform exports complete');})();
