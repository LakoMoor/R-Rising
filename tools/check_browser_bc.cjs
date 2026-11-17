// Optional local-browser smoke test. No browser or npm package is installed by this script.
// Usage: node tools/check_browser_bc.cjs <viewer-url> <local-scene.json> <output-directory>
// RRISING_PLAYWRIGHT_MODULE may point to an existing Playwright installation.
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require(process.env.RRISING_PLAYWRIGHT_MODULE || 'playwright');
const [url, scene, output] = process.argv.slice(2);
if (!url || !scene || !output) throw Error('Expected viewer URL, scene file and output directory');
fs.mkdirSync(output, { recursive: true });
(async () => {
 const browser = await chromium.launch({channel:'chrome',headless:true,args:['--enable-unsafe-webgpu','--enable-dawn-features=allow_unsafe_apis']});
 const results=[];
 try {
  for(const withoutBC of [false,true]) {
   const page=await browser.newPage({viewport:{width:1440,height:1050},deviceScaleFactor:1});
   const errors=[];
   page.on('pageerror',e=>errors.push(String(e)));
   page.on('console',m=>{if(m.type()==='error')errors.push(m.text())});
   await page.addInitScript((withoutBC)=>{
    if(!window.GPUAdapter)return;
    if(withoutBC) {
     const original=Object.getOwnPropertyDescriptor(GPUAdapter.prototype,'features');
     Object.defineProperty(GPUAdapter.prototype,'features',{get(){return new Set([...original.get.call(this)].filter(f=>!f.startsWith('texture-compression-bc')))},configurable:true});
    }
    const request=GPUAdapter.prototype.requestDevice;
    GPUAdapter.prototype.requestDevice=async function(descriptor){
      const device=await request.call(this,descriptor);
      window.bcProbe={requestedFeatures:Array.from(descriptor?.requiredFeatures || []),actualFeatures:Array.from(device.features)};
      return device;
    };
   },withoutBC);
   await page.goto(url,{waitUntil:'networkidle'});
   await page.waitForFunction(()=>document.querySelector('#status').textContent.startsWith('Ready'),null,{timeout:30000});
   await page.locator('canvas').evaluate(c=>{c.width=1280;c.height=720});
   const at=Date.now();
   await page.locator('#scene').setInputFiles(scene);
   await page.waitForFunction(()=>document.documentElement.dataset.loaded==='true'||document.querySelector('#status').textContent.startsWith('Unable'),null,{timeout:60000});
   const state=await page.evaluate(()=>({status:document.querySelector('#status').textContent,loaded:document.documentElement.dataset.loaded,probe:window.bcProbe}));
   if(state.loaded!=='true')throw Error(JSON.stringify(state));
   if(withoutBC && (state.probe.actualFeatures.some(f=>f.startsWith('texture-compression-bc')) || state.probe.requestedFeatures.some(f=>f.startsWith('texture-compression-bc'))))throw Error('BC feature was still enabled');
   await page.locator('canvas').screenshot({path:path.join(output, `browser-bc-${withoutBC?'fallback':'hardware'}.png`)});
   if(errors.length) throw Error(errors.join('\n'));
   results.push({withoutBC,...state,loadMs:Date.now()-at,errors});
   await page.close();
  }
  const report = {browser: browser.version(), featureAbsenceSimulated: true, results};
  fs.writeFileSync(path.join(output, 'browser-bc.json'), JSON.stringify(report,null,2)+'\n');
  console.log(JSON.stringify(report,null,2));
 } finally {await browser.close()}
})().catch(e=>{console.error(e);process.exit(1)});
