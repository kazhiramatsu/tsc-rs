import fs from 'node:fs';
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
const root = '/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep';
const prep = '/tmp/emitter-w5-stratum-repair-r520';
const actual = `${root}/crates/oracle/h2-7a-witnesses.mjs`;
const actualUrl = pathToFileURL(actual).href;
let source = fs.readFileSync(`${prep}/candidate-h2-7a-witnesses.mjs`, 'utf8');
const marker = 'try {\n  if (MODE === INTERNAL_OBSERVE_MODE)';
assert.equal(source.split(marker).length, 2);
source = source.slice(0, source.lastIndexOf(marker));
source = source.replace('const GENERATOR_PATH = fileURLToPath(import.meta.url);', `const GENERATOR_PATH = fileURLToPath(${JSON.stringify(actualUrl)});`);
source = source.replace(/from "(\.\.?\/[^"\n]+)"/g, (_, relative) => `from ${JSON.stringify(new URL(relative, actualUrl).href)}`);
source += '\nexport { prepareStaticContext, currentStratumCensusMatches, verifyM1Projection, verifyS2Projection, parseCensusJsonl };\n';
process.env.TSRS_H2_7A_STRATUM_CENSUS = `${root}/target/h2-7a/stratum-census/census.jsonl`;
const module = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
const before = fs.readFileSync(`${root}/ratchets/h2-7a-witnesses.v1.json`);
const witness = JSON.parse(before);
const context = module.prepareStaticContext();
module.verifyM1Projection(context.caseManifest.cases, witness.observations, context.stratum, 'external candidate static context');
module.verifyS2Projection(context.caseManifest.cases, witness.observations, context.m2Supplement, 'external candidate static context');
assert.deepEqual(context.stratum, witness.stratum);
const rows = new Map(fs.readFileSync(process.env.TSRS_H2_7A_STRATUM_CENSUS, 'utf8').trim().split('\n').map(x => { const row = JSON.parse(x); return [row.case_id, row]; }));
const qualification = new Map(JSON.parse(fs.readFileSync(`${root}/ratchets/h2-6c-qualification.v1.json`, 'utf8')).cases.map(x => [x.case_id, x]));
for (const id of context.stratum.case_ids) assert.equal(module.currentStratumCensusMatches(rows.get(id), qualification.get(id)), true, id);
const id = context.stratum.case_ids[0], original = rows.get(id), expected = qualification.get(id);
const mutants = [
 ['missing declaration', x => x.writes_missing.push({kind:'declaration',path:'/x.d.ts'})],
 ['missing JavaScript', x => x.writes_missing.push({kind:'javascript',path:'/x.js'})],
 ['different bytes', x => x.writes_diverging.push({kind:'javascript',path:'/x.js',bytes_equal:false,bom_equal:true})],
 ['unexpected write', x => x.writes_rust_only.push('/extra.js')],
 ['write count', x => x.writes_exact++],
 ['diagnostic count', x => x.reported_diagnostics.rust++],
 ['forged equal diagnostic counts', x => {x.reported_diagnostics.rust++;x.reported_diagnostics.expected++;}],
 ['map count', x => x.source_maps_count++],
 ['emit skipped', x => x.emit_skipped = !x.emit_skipped],
];
for (const [name, mutate] of mutants) { const row = structuredClone(original);mutate(row);assert.equal(module.currentStratumCensusMatches(row,expected),false,name); }
assert.deepEqual(fs.readFileSync(`${root}/ratchets/h2-7a-witnesses.v1.json`),before);
const result={qualified:false,scope:'external candidate static-context and predicate probe; supplied exact r519 census, no fresh census or TypeScript observation qualification',cases:context.caseSpecs.length,signed_stratum:context.stratum.count,complete_census_rows:context.stratum.count,predicate_rejections:mutants.length,m1_projection_preserved:true,s2_projection_preserved:true,historical_census_sha256:context.stratum.census_jsonl_sha256};
fs.writeFileSync(`${prep}/candidate-probe-result.json`,JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify(result));
