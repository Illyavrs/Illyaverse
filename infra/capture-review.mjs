import {chromium} from '@playwright/test';
import {mkdir, readFile} from 'node:fs/promises';
import path from 'node:path';
const output=path.resolve('.impeccable/review');
await mkdir(output,{recursive:true});
const browser=await chromium.launch({headless:true});
const errors=[];
let captures = 0;
const environment = Object.fromEntries((await readFile('.env','utf8')).split(/\r?\n/).filter((line) => line && !line.startsWith('#') && line.includes('=')).map((line) => {const at=line.indexOf('='); return [line.slice(0,at),line.slice(at+1)];}));
for(const [name,width,height] of [['desktop',1440,1000],['mobile',390,844]]){
 const context=await browser.newContext({viewport:{width,height},reducedMotion:'reduce'});
 const page=await context.newPage();page.on('pageerror',error=>errors.push(error.message));
 for(const [surface,url] of [['home','/'],['settings','/settings'],['maps','/beatmaps'],['chat','/chat'],['connect','/connect'],['register','/register'],['docs','/docs/connection']]){
  await page.goto(`http://localhost:3000${url}`,{waitUntil:'networkidle'});
  await page.evaluate(()=>document.fonts.ready);
  await page.screenshot({path:path.join(output,surface==='home'?`${name}.png`:`${name}-${surface}.png`),fullPage:true,animations:'disabled'});
  captures++;
  if(await page.evaluate(()=>document.documentElement.scrollWidth>window.innerWidth))errors.push(`${name}/${surface}: horizontal overflow`);
 }
 const login=await context.request.post('http://localhost:3000/api/v1/auth/login',{headers:{Origin:'http://localhost:3000'},data:{username:environment.BOOTSTRAP_ADMIN_USERNAME,password:environment.BOOTSTRAP_ADMIN_PASSWORD}});
 if(!login.ok()) throw new Error('Local capture account login failed');
 const {user}=await login.json();
 for(const [surface,url] of [['profile',`/players/${user.id}`],['admin','/admin']]){
  await page.goto(`http://localhost:3000${url}`,{waitUntil:'networkidle'});
  await page.evaluate(()=>document.fonts.ready);
  await page.screenshot({path:path.join(output,`${name}-${surface}.png`),fullPage:true,animations:'disabled'});
  captures++;
  if(await page.evaluate(()=>document.documentElement.scrollWidth>window.innerWidth))errors.push(`${name}/${surface}: horizontal overflow`);
 }
 await context.close();
}
await browser.close();
console.log(JSON.stringify({captures,errors},null,2));
if(errors.length)process.exitCode=1;
