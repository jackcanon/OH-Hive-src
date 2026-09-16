const fs=require('fs'),path=require('path');
const sharp=require('/Users/dit1/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/sharp');
const root=path.resolve(__dirname,'..');
async function png(src,dest,size){fs.mkdirSync(path.dirname(path.join(root,dest)),{recursive:true});await sharp(path.join(root,src)).resize(size).png().toFile(path.join(root,dest));}
(async()=>{
for(const file of fs.readdirSync(path.join(root,'assets')).filter(x=>x.endsWith('.svg')&&!x.includes('fullbleed'))){
 for(const size of (file.includes('symbol')||file.includes('wordmark')||file.includes('lockup')?[512,1024,2048]:[1024])) await png('assets/'+file,'png/'+file.replace('.svg','-'+size+'.png'),size);
}
const sizes=[16,24,32,48,64,128,256,512,1024];
for(const size of sizes)await png(size<32?'assets/den-favicon.svg':'assets/den-app-dark.svg','icons/linux/den-'+size+'.png',size);
fs.copyFileSync(path.join(root,'assets/den-app-dark.svg'),path.join(root,'icons/linux/den.svg'));
let contents={images:[],info:{author:'xcode',version:1}};
for(const size of [16,32,128,256,512])for(const scale of [1,2]){
 const pixels=size*scale,name=`icon_${size}x${size}${scale===2?'@2x':''}.png`;
 await png(pixels<32?'assets/den-favicon.svg':'assets/den-app-dark.svg','icons/macos/Den.iconset/'+name,pixels);
 const catalog=path.join(root,'icons/macos/AppIcon.appiconset');fs.mkdirSync(catalog,{recursive:true});fs.copyFileSync(path.join(root,'icons/macos/Den.iconset',name),path.join(catalog,name));
 contents.images.push({idiom:'mac',size:`${size}x${size}`,scale:`${scale}x`,filename:name});
}
fs.writeFileSync(path.join(root,'icons/macos/AppIcon.appiconset/Contents.json'),JSON.stringify(contents,null,2));
for(const [src,name,size]of [['den-app-dark-fullbleed','apple-touch-icon',180],['den-app-dark-fullbleed','icon-192',192],['den-app-dark-fullbleed','icon-512',512],['den-maskable','maskable-512',512],['den-favicon','favicon-16',16],['den-favicon','favicon-32',32]])await png('assets/'+src+'.svg','icons/web/'+name+'.png',size);
fs.copyFileSync(path.join(root,'assets/den-favicon.svg'),path.join(root,'icons/web/favicon.svg'));
fs.writeFileSync(path.join(root,'icons/web/site.webmanifest'),JSON.stringify({name:'Loki’s Den',short_name:'Den',start_url:'/',display:'standalone',background_color:'#17201F',theme_color:'#17201F',icons:[{src:'icon-192.png',sizes:'192x192',type:'image/png'},{src:'icon-512.png',sizes:'512x512',type:'image/png'},{src:'maskable-512.png',sizes:'512x512',type:'image/png',purpose:'maskable'}]},null,2));
console.log('PNG exports and platform sources complete');
})();
