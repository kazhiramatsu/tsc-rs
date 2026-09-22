import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import ts from '/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/vendor/typescript-6.0.3/lib/typescript.js';
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'emitter-bom-r201-'));
const observations=[];
try {
  for(const [name,text] of [['plain','/* c */x;'],['single','\uFEFF/* c */x;'],['double','\uFEFF\uFEFF/* c */x;'],['embedded',' \uFEFF/* c */x;'],['bom-only','\uFEFF'],['non-ascii','\uFEFF/* 😀 */\u2028x;']]) {
    const file=path.join(dir,name+'.ts');fs.writeFileSync(file,text,'utf8');
    const actual=ts.sys.readFile(file), projected=text.replace(/^\uFEFF/,'');assert.equal(actual,projected,name);
    observations.push({name,input_utf8_hex:Buffer.from(text).toString('hex'),sys_read_file:actual,virtual_read:projected});
  }
  console.log(JSON.stringify({typescript:ts.version,observer_sha256:crypto.createHash('sha256').update(fs.readFileSync(import.meta.filename)).digest('hex'),observations},null,2));
} finally { fs.rmSync(dir,{recursive:true}); }
