from pathlib import Path
import re,json,hashlib,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');O=Path('/tmp/emitter-source-promotion-r429');O.mkdir(exist_ok=False);h=lambda b:hashlib.sha256(b).hexdigest();rows=[]
p='crates/xtask/src/h2_1a_acceptance.rs';old=(R/p).read_text();s=old
start=s.index('static CURRENT_EXACT_SOURCE_PROMOTIONS:');end=s.index('\nfn failure(',start);part=s[start:end]
part=part.replace('static CURRENT_EXACT_SOURCE_PROMOTIONS: &[(&str, &str, &str)] = &[','''#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CurrentExactSourcePromotion {
    case_id: &'static str,
    case_fingerprint_sha256: &'static str,
    required_slice: &'static str,
    expected_extra_activity: &'static [(H2RuntimeSlice, u64)],
}

static CURRENT_EXACT_SOURCE_PROMOTIONS: &[CurrentExactSourcePromotion] = &[''')
pattern=r'    \(\n        ("[^"\n]+"),\n        ("[^"\n]+"),\n        ("[^"\n]+"),\n    \),'
part,n=re.subn(pattern,lambda m:'    CurrentExactSourcePromotion {\n        case_id: '+m[1]+',\n        case_fingerprint_sha256: '+m[2]+',\n        required_slice: '+m[3]+',\n        expected_extra_activity: &[],\n    },',part);assert n==7
needle='];\n\nfn current_exact_source_promotion(case: &Value) -> Result<bool, Box<dyn Error>> {'
assert part.count(needle)==1
part=part.replace(needle,'''    // Both parsers attach each decorator to its using VariableStatement.
    // Class-fields routing therefore records H2.4b once for this source.
    CurrentExactSourcePromotion {
        case_id: "typescript-6.0.3/conformance/decorators/invalid/decoratorOnUsing.ts#default",
        case_fingerprint_sha256: "4437e98b97ba1e53501aaba0efb82063f099d2549da4be233e3edf623031beff",
        required_slice: "H2.9",
        expected_extra_activity: &[(H2RuntimeSlice::H2_4b, 1)],
    },
];

fn current_exact_source_promotion(
    case: &Value,
) -> Result<Option<&'static CurrentExactSourcePromotion>, Box<dyn Error>> {''')
part=part.replace('let Some((_, fingerprint, required_slice)) = CURRENT_EXACT_SOURCE_PROMOTIONS','let Some(promotion) = CURRENT_EXACT_SOURCE_PROMOTIONS').replace('.find(|(id, _, _)| *id == case_id)','.find(|promotion| promotion.case_id == case_id)').replace('return Ok(false);','return Ok(None);').replace('!= *fingerprint','!= promotion.case_fingerprint_sha256').replace('json!([required_slice])','json!([promotion.required_slice])').replace('Ok(true)','Ok(Some(promotion))')
s=s[:start]+part+s[end:]
s=s.replace('    match diagnostic_expectation {','    let mut source_promotion = None;\n    match diagnostic_expectation {',1)
a='''            if !current_exact_source_promotion(case)? {
                return Err(failure(format!(
                    "{case_id}: exact source promotion is not recorded"
                )));
            }'''
b='''            source_promotion = Some(current_exact_source_promotion(case)?.ok_or_else(|| {
                failure(format!("{case_id}: exact source promotion is not recorded"))
            })?);''';assert s.count(a)==1;s=s.replace(a,b)
a='''        if slice != H2RuntimeSlice::H2_1a && activity.runtime_slice(slice) != 0 {
            return Err(failure(format!(
                "{case_id}: unadmitted {} activity",
                slice.name()
            )));
        }'''
b='''        if slice == H2RuntimeSlice::H2_1a {
            continue;
        }
        let expected = source_promotion
            .and_then(|promotion| {
                promotion.expected_extra_activity.iter().find_map(|&(owner, count)| {
                    (owner == slice).then_some(count)
                })
            })
            .unwrap_or(0);
        let observed = activity.runtime_slice(slice);
        if observed != expected {
            return Err(failure(format!(
                "{case_id}: {} activity {observed} differs from expected {expected}",
                slice.name()
            )));
        }''';assert s.count(a)==1;s=s.replace(a,b)
a='if current_exact_source_promotion(case)? {';assert s.count(a)==1;s=s.replace(a,'if current_exact_source_promotion(case)?.is_some() {')
for path,original,new in [(p,old,s)]:
 dst=O/path;dst.parent.mkdir(parents=True);dst.write_text(new);rows.append({'path':path,'before':h(original.encode())})
p='crates/xtask/tests/unit/h2_1a_acceptance/tests.rs';old=(R/p).read_text();s=old.replace('CURRENT_EXACT_SOURCE_PROMOTIONS.len(), 7','CURRENT_EXACT_SOURCE_PROMOTIONS.len(), 8').replace('for (case_id, _, required_slice) in super::CURRENT_EXACT_SOURCE_PROMOTIONS {','for promotion in super::CURRENT_EXACT_SOURCE_PROMOTIONS {\n        let case_id = promotion.case_id;\n        let required_slice = promotion.required_slice;').replace('case["case_id"] == *case_id','case["case_id"] == case_id').replace('if *required_slice == "H2.9"','if required_slice == "H2.9"').replace('(writes, diagnostics), (8, 20)','(writes, diagnostics), (9, 21)')
needle='        let (case_writes, case_diagnostics) = super::execute_observed('
start=s.index('fn current_source_promotions_compare_original_observations_and_pin_their_owner()');i=s.index(needle,start);s=s[:i]+'''        assert_eq!(
            super::current_exact_source_promotion(case).expect("validate exact source promotion"),
            Some(promotion),
        );
'''+s[i:]
dst=O/p;dst.parent.mkdir(parents=True);dst.write_text(s);rows.append({'path':p,'before':h(old.encode())})
subprocess.run(['rustfmt','--edition','2021','--config','skip_children=true',*[str(O/r['path']) for r in rows]],check=True)
patch=b''
for r in rows:
 r['after']=h((O/r['path']).read_bytes());q=subprocess.run(['diff','-u','--label','a/'+r['path'],'--label','b/'+r['path'],str(R/r['path']),str(O/r['path'])],capture_output=True);assert q.returncode==1;patch+=q.stdout
(O/'proposal.patch').write_bytes(patch);(O/'manifest.json').write_text(json.dumps({'head':'8859338f1c761d867cbc7f5ef58987dc3777f944','files':rows,'provenance':'/tmp/emitter-decorator-provenance-r428/manifest.json','qualified':False},indent=2)+'\n');print(patch.decode())
