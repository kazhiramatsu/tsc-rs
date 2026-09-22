from pathlib import Path
import subprocess,json,hashlib
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
changes=[]
def replace(rel,old,new,count=1):
 p=root/rel;s=p.read_text();assert s.count(old)==count,(rel,s.count(old),count);p.write_text(s.replace(old,new));changes.append(rel)
replace('crates/xtask/src/utf16_literal_recovery_census.rs','fs::write(\n        &out,','fs::write(\n        out,')
replace('crates/compiler/tests/integration/declaration_transformer_replay_decision_equal.rs','(&self.common_source_directory)\n            .to_str()','self.common_source_directory\n            .to_str()')
replace('crates/compiler/tests/integration/declaration_transformer_replay_decision_equal.rs','alternate_result: alternate_result.map(Into::into),','alternate_result,',3)
for rel in ['crates/compiler/tests/integration/h2_5h_static_this_super_rows.rs','crates/compiler/tests/emitter_final_rows.rs','crates/compiler/tests/h2_5h_utf16_literal_rows.rs']:
 for key in ['emitted_files','source_maps']:
  replace(rel,f'!= !expected_result["{key}"].is_null()',f'== expected_result["{key}"].is_null()')
replace('crates/harness/tests/integration/module_suffixes_oracle_contract.rs','''        (|| -> Result<Vec<String>, ConfigHostError> {
            Ok(self
                .files
                .keys()
                .filter(|path| extensions.iter().any(|extension| path.ends_with(extension)))
                .cloned()
                .collect())
        })()
        .map(|paths| paths.into_iter().map(Into::into).collect())''','''        Ok(self
            .files
            .keys()
            .filter(|path| extensions.iter().any(|extension| path.ends_with(extension)))
            .cloned()
            .map(Into::into)
            .collect())''')
replace('crates/compiler/tests/integration/emit_session_contract.rs','''        (|| -> Result<(), String> {
            panic!(
                "existing project parent must not be created: {}",
                path.display()
            )
        })()
        .map_err(Into::into)''','''        panic!(
            "existing project parent must not be created: {}",
            path.display()
        )''')
replace('crates/compiler/examples/h2_baseline_qualification.rs','''        (|| -> Result<(), String> {
            Err(format!(
                "unexpected parent-directory construction for {}",
                path.display()
            ))
        })()
        .map_err(Into::into)''','''        Err(format!(
            "unexpected parent-directory construction for {}",
            path.display()
        )
        .into())''')
# Closure-free controlled sinks preserve the attempt event before the failure
# decision and the String -> JsString conversion on the same error message.
for rel in ['crates/compiler/tests/h2_7e_declaration_maps.rs','crates/compiler/tests/h2_7e_declaration_map_apis.rs']:
 p=root/rel;s=p.read_text();start=s.index('        (|| -> Result<(), String> {');end=s.index('map_err(Into::into)',start)+len('map_err(Into::into)');old=s[start:end];assert 'return ' not in old and '?' not in old
 body=old[len('        (|| -> Result<(), String> {'):];body=body[:body.rfind('}')]
 body=body.replace('Err("H2.7e controlled system failure".to_owned())','Err("H2.7e controlled system failure".into())')
 replace(rel,old,body.strip('\n'))
Path('/tmp/emitter-lint-repairs-r241-paths.json').write_text(json.dumps(sorted(set(changes)),indent=2)+'\n')
print('Repaired',len(set(changes)),'files')
