const { chromium } = require('/home/dev/chromium-scratch/node_modules/playwright-core');
const zlib = require('zlib'); const fs = require('fs');
function decodePng(buf){
  let p=8, w,h,ct,idat=[];
  while(p<buf.length){const len=buf.readUInt32BE(p);const type=buf.toString('ascii',p+4,p+8);const d=buf.slice(p+8,p+8+len);
    if(type==='IHDR'){w=d.readUInt32BE(0);h=d.readUInt32BE(4);ct=d[9];if(d[8]!==8||d[12]!==0)throw new Error('png fmt');}
    if(type==='IDAT')idat.push(d); p+=12+len;}
  const bpp=ct===6?4:3; const raw=zlib.inflateSync(Buffer.concat(idat)); const stride=w*bpp; const out=Buffer.alloc(h*stride);
  for(let y=0;y<h;y++){const f=raw[y*(stride+1)]; const row=raw.slice(y*(stride+1)+1,(y+1)*(stride+1));
    for(let x=0;x<stride;x++){const a=x>=bpp?out[y*stride+x-bpp]:0;const b=y>0?out[(y-1)*stride+x]:0;const c=(x>=bpp&&y>0)?out[(y-1)*stride+x-bpp]:0;
      let v=row[x]; if(f===1)v+=a; else if(f===2)v+=b; else if(f===3)v+=((a+b)>>1); else if(f===4){const pa=Math.abs(b-c),pb=Math.abs(a-c),pc=Math.abs(a+b-2*c);v+=(pa<=pb&&pa<=pc)?a:(pb<=pc?b:c);} out[y*stride+x]=v&255;}}
  return {w,h,bpp,data:out};
}
// Usage: node measure.js '<scenarios json>' <output dir> <html dir from build.py>
const scen = JSON.parse(process.argv[2]);
const outdir = process.argv[3];
const htmldir = process.argv[4];
(async()=>{
  const browser = await chromium.launch({executablePath:'/home/dev/.cache/ms-playwright/chromium_headless_shell-1148/chrome-linux/headless_shell',args:['--disable-gpu','--no-zygote','--renderer-process-limit=1','--hide-scrollbars'],headless:true});
  const results=[];
  for(const s of scen){
    const ctx = await browser.newContext({viewport:{width:s.w,height:s.h},deviceScaleFactor:1});
    const page = await ctx.newPage();
    const sb = s.w>=1024 ? 'sb':'nosb';
    const sel = {hero:'[data-ui=hero]',item:'[data-ui=item-backdrop]',corner:'[data-ui=hero-corner]'}[s.kind];
    const imgSel = {hero:'[data-ui=hero-backdrop]',item:'[data-ui=item-backdrop-image]',corner:'[data-ui=hero-backdrop]'}[s.kind];
    // white page: alpha map
    await page.goto(`file://${htmldir}/${s.kind}-white-${sb}${s.wash?'-wash':''}.html`); await page.waitForTimeout(300);
    const geo = await page.evaluate(([sel,imgSel])=>{const r=e=>{const b=document.querySelector(e).getBoundingClientRect();return {x:b.x,y:b.y,w:b.width,h:b.height}};
      const fr=document.querySelector('[data-ui=backdrop-frame]'); return {box:r(sel),img:r(imgSel),frame:fr?(()=>{const b=fr.getBoundingClientRect();return {x:b.x,y:b.y,w:b.width,h:b.height}})():null,vw:innerWidth,vh:innerHeight};},[sel,imgSel]);
    const pngbuf = await page.screenshot({type:'png'}); const png=decodePng(pngbuf);
    const lum=(X,Y)=>{X=Math.round(X);Y=Math.round(Y); if(X<0||Y<0||X>=png.w||Y>=png.h) return null; const i=(Y*png.w+X)*png.bpp; return (png.data[i]+png.data[i+1]+png.data[i+2])/3;};
    const I=geo.img; const sc=Math.max(I.w/1920,I.h/1080); const dw=1920*sc, dh=1080*sc; const ox=I.x+(I.w-dw)/2, oy=I.y;
    const vis={x0:Math.max(0,(Math.max(I.x,0)-ox)/dw),x1:Math.min(1,(Math.min(I.x+I.w,geo.vw)-ox)/dw),y0:Math.max(0,(Math.max(I.y,0)-oy)/dh),y1:Math.min(1,(Math.min(I.y+I.h,geo.vh)-oy)/dh)};
    const alpha=(px,py)=>{const L=lum(ox+px*dw, oy+py*dh); if(L===null) return null; return +(1-(L-7)/(255-7)).toFixed(2);};
    const rows={}; for(const q of [0.03,0.10,0.25,0.5]){rows[q]=[];for(let p=0;p<=100;p+=5){rows[q].push([p,alpha(p/100,q)])}}
    // Footprints of OpenPosterDB backdrop badges, in 1920x1080 image px (see safe-areas.md "Badge footprints").
    const VW=88,VH=102,VSP=7,HSP=10,M=20;
    const fps=[];
    for(let n=1;n<=5;n++) fps.push({id:`stack${n}_tr`,x:1920-VW-M,y:M,w:VW,h:VH*n+VSP*(n-1)});
    for(let k=0;k<5;k++) fps.push({id:`stack5_tr_b${k+1}`,x:1920-VW-M,y:M+k*(VH+VSP),w:VW,h:VH});
    for(const n of [3,5]){const w=VW*n+HSP*(n-1); fps.push({id:`row${n}_tr`,x:1920-w-M,y:M,w,h:VH}); fps.push({id:`row${n}_tc`,x:(1920-w)/2,y:M,w,h:VH});}
    fps.push({id:'pillrow3_tr',x:1920-520-M,y:M,w:520,h:58}); fps.push({id:'pillrow3_tc',x:(1920-520)/2,y:M,w:520,h:58});
    const stat=(f)=>{const nx=Math.max(3,Math.ceil(f.w/16)+1), ny=Math.max(3,Math.ceil(f.h/16)+1); let sum=0,n=0,max=0,miss=0,tot=0;
      for(let u=0;u<nx;u++)for(let v=0;v<ny;v++){tot++; const px=(f.x+f.w*u/(nx-1))/1920, py=(f.y+f.h*v/(ny-1))/1080;
        if(px<vis.x0||px>vis.x1||py<vis.y0||py>vis.y1){miss++;continue;} const a=alpha(px,py); if(a===null){miss++;continue;} sum+=a;n++;max=Math.max(max,a);}
      return {id:f.id,x0:+(f.x/19.2).toFixed(1),x1:+((f.x+f.w)/19.2).toFixed(1),y0:+(f.y/10.8).toFixed(1),y1:+((f.y+f.h)/10.8).toFixed(1),mean:n?+(sum/n).toFixed(2):null,max:n?+max.toFixed(2):null,cropped:miss,of:tot}};
    const stats=fps.map(stat);
    // grid page: screenshot + content rects
    await page.goto(`file://${htmldir}/${s.kind}-grid-${sb}${s.wash?'-wash':''}.html`); await page.waitForTimeout(300);
    const content = await page.evaluate(()=>{const g=e=>{const el=document.querySelector(e); if(!el) return null; const b=el.getBoundingClientRect(); return {x:+b.x.toFixed(1),y:+b.y.toFixed(1),w:+b.width.toFixed(1),h:+b.height.toFixed(1)}}; return {heroContent:g('[data-ui=hero-content]'),logo:g('[data-ui=hero-logo]'),overview:g('[data-ui=hero-overview]'),actions:g('[data-ui=hero-actions]'),itemHeader:g('[data-ui=item-header]'),itemBody:g('[data-ui=item-body]'),itemLogo:g('[data-ui=item-logo]'),itemPoster:g('[data-ui=item-poster]')}});
    if(s.shot){await page.screenshot({path:`${outdir}/${s.name}.jpg`,type:'jpeg',quality:60});}
    results.push({name:s.name,wash:!!s.wash,kind:s.kind,vp:[s.w,s.h],geo,scale:+sc.toFixed(4),drawn:{w:+dw.toFixed(1),h:+dh.toFixed(1)},visible_pct:{x0:+(vis.x0*100).toFixed(1),x1:+(vis.x1*100).toFixed(1),y0:+(vis.y0*100).toFixed(1),y1:+(vis.y1*100).toFixed(1)},alpha_rows:rows,footprints:stats,content});
    await ctx.close();
  }
  await browser.close();
  fs.writeFileSync(outdir+'/measure.json',JSON.stringify(results,null,1));
  console.log('done',results.length);
})().catch(e=>{console.error('FAIL',e);process.exit(1)});
