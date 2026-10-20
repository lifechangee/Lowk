import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os'; import { join } from 'node:path';
const root='~/Applications/Projects/Lowk', S=process.argv[2];
const work=mkdtempSync(join(tmpdir(),'lowk-f5-')), data=join(work,'data'); mkdirSync(data);
writeFileSync(join(data,'settings.toml'),`[onboarding]\ndone = true\nrecording_notice_ack = true\n[vault]\npath = "${work}/vault"\n`);
const port=19000+Math.floor(Math.random()*500), token='abc123f5';
const app=spawn(root+'/target/qa/debug/lowk',[],{env:{...process.env,LOWK_QA_MULTI:'1',LOWK_DATA_DIR:data,LOWK_QA_PORT:String(port),LOWK_QA_TOKEN:token},stdio:'ignore'});
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
const cmd=async l=>(await fetch(`http://127.0.0.1:${port}/cmd`,{method:'POST',headers:{'X-Lowk-Qa-Token':token},body:l})).text();
const ev=async(w,e)=>JSON.parse(await cmd(`eval ${w} ${e}`)).v;
let sp;
try{
 for(let i=0;i<100;i++){try{await cmd('pid');break}catch{await sleep(100)}}
 await sleep(2500);
 await cmd('exclusion off'); await cmd('overlay-show'); await sleep(800); await cmd('demo-start'); await sleep(1500);
 const w=JSON.parse(await cmd('win overlay')); console.log(JSON.stringify(w));
 const sc=w.scale||1;
 sp=spawn(S+'/split',[w.x/sc,w.y/sc,w.w/sc,w.h/sc].map(String),{stdio:'ignore'});
 await sleep(6000);
 console.log('tone',await cmd('tone'));
 console.log(JSON.stringify(await ev('overlay',`[...document.querySelectorAll('.ov [data-blk]')].map(e=>e.className.split(' ')[0]+':'+e.style.getPropertyValue('--fg')+'@'+Math.round(e.getBoundingClientRect().x)+','+Math.round(e.getBoundingClientRect().y))`)));
 for(const t of [60,90]){await cmd('opacity '+t);await sleep(1500);
  const cap=join(S,'qa-cap');spawnSync(cap,['shot',await cmd('pid'),'overlay',`${S}/f5-split-t${t}.png`]);}
}finally{ sp?.kill(); try{await cmd('exclusion on');await cmd('quit')}catch{} await sleep(500); app.kill();}
