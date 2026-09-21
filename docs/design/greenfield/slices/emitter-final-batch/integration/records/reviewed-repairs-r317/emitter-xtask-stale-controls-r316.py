from pathlib import Path
import json,subprocess,gzip,difflib,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');O=Path('/tmp/emitter-xtask-stale-controls-r316');O.mkdir(exist_ok=False);j=json.loads((R/'ratchets/h2-6c-qualification.v1.json').read_text());old=json.loads(subprocess.check_output(['git','show','3b1f5fe87fd31e3b303bb44bd257342735452ed9:ratchets/h2-6c-qualification.v1.json'],cwd=R));selected=[]
for target in ['es2015','es2022','esnext']:
 id=f'typescript-6.0.3/conformance/esDecorators/classDeclaration/esDecorators-classDeclaration-sourceMap.ts#target%3D{target}';rows=[c for c in j['cases'] if c['case_id']==id];prior=[c for c in old['cases'] if c['case_id']==id];assert len(rows)==len(prior)==1 and rows[0]==prior[0];c=rows[0];selected.append((target,c['case_fingerprint_sha256'],c['observation_input_sha256']))
files=[]
p='crates/xtask/tests/unit/h2_2c_acceptance/de_legacy_collector.rs';before=(R/p).read_text();start=before.index('fn nonbundle_declaration_maps_dispose_javascript_parse_metadata()');head=before[:start];body=before[start:];needle='''    let qualification = pinned(
        &workspace.join(H2_6C_QUALIFICATION_RELATIVE_PATH),
        "af689ec23311d9cc733f2606e2acfd1d346edbc51bc512bb185c0c3111f1d8ec",
    )?;''';assert body.count(needle)==1
replacement='''    // The container is re-minted by the walk. Pin the three complete original
    // cases below so unrelated provenance/host repairs cannot stale this test.
    let qualification: Value = serde_json::from_slice(&fs::read(
        workspace.join(H2_6C_QUALIFICATION_RELATIVE_PATH),
    )?)?;
    assert!(census_fingerprint_verifies(
        &qualification,
        "qualification_fingerprint_sha256"
    ));''';body=body.replace(needle,replacement)
rows='    let original_cases = [\n'+''.join('        (\n'+''.join('            '+json.dumps(x)+',\n' for x in row)+'        ),\n' for row in selected)+'    ];\n    for (target, fingerprint, input_sha) in original_cases {';body=body.replace('    for target in ["es2015", "es2022", "esnext"] {',rows)
needle='''        let observed = collect_case(&workspace, case, &inputs)?;''';guard='''        if case["case_fingerprint_sha256"] != fingerprint
            || case["observation_input_sha256"] != input_sha
            || case["disposition"] != "admitted-for-execution"
            || !census_fingerprint_verifies(case, "case_fingerprint_sha256")
        {
            return Err(failure(format!("{id}: original case identity changed")));
        }
''';assert body.count(needle)==1;body=body.replace(needle,guard+needle);files.append((p,before,head+body))
p='crates/xtask/tests/unit/h2_7b_acceptance/tests.rs';before=(R/p).read_text();start=before.index('fn h2_6c_current_manifest_keeps_only_unclosed_historical_refusals()');end=before.index('\n#[test]',start);oldbody=before[start:end];h=oldbody[oldbody.index('    let historical ='):oldbody.index('    let expected =')];newbody='''fn h2_6c_current_manifest_keeps_only_unclosed_historical_refusals() {
    let workspace = workspace();
'''+h+'''    // Same pinned promotion/migration chain as the live acceptance projection.
    let expected = super::h2_6c_de_promotions::adjusted_refusals(historical.clone())
        .expect("declaration promotions consume each original refusal once");
    let expected = super::h2_6c_output_promotions::adjusted_refusals(expected)
        .expect("output promotions consume each original refusal once");
    let expected = super::h2_6c_refusal_migrations::adjust_refusal_totals(expected)
        .expect("remaining migrations retain their owned refusals");
    assert_eq!(
        historical.values().sum::<u64>(),
        (super::h2_6c_de_promotions::promoted_count()
            + super::h2_6c_output_promotions::promoted_count()
            + super::h2_6c_refusal_migrations::count()) as u64,
        "every historical refusal has a pinned closing row"
    );
    assert!(expected.is_empty(), "all original option refusals are closed");
    let listed = super::load_h2_6c_divergence_manifest_state(&workspace, true)
        .expect("load the canonical manifest through the acceptance loader");
    let mut actual = BTreeMap::<String, u64>::new();
    for divergence in listed.entries.values() {
        if let Some(option) = divergence.refused_option.as_deref() {
            *actual.entry(option.to_owned()).or_default() += 1;
        }
    }
    assert_eq!(actual, expected, "no stale historical refusals remain");
    assert!(listed.entries.is_empty(), "no unowned divergence remains");
    assert!(
        !workspace.join(super::H2_6C_KNOWN_DIVERGENCES_RELATIVE_PATH).exists(),
        "the empty divergence manifest must stay retired"
    );
}
''';files.append((p,before,before[:start]+newbody+before[end:]))
p='crates/xtask/src/h2_3d_acceptance.rs';before=(R/p).read_text();start=before.index('fn expected_owner_activity(');end=before.index('\nfn execute_owner_control',start);body=before[start:end];needle='            H2RuntimeSlice::H2_1a if module != 4 && module != 200 => output_units,';assert body.count(needle)==1;body=body.replace(needle,'''            // Only the implied-format composite constructs this owner. AMD/UMD
            // use the direct module delegate and retain H2.1b/H2.1c below.
            H2RuntimeSlice::H2_1a if matches!(module, 1 | 5..=7 | 99 | 100..=102 | 199) => output_units,''');files.append((p,before,before[:start]+body+before[end:]))
patch=[];manifest={'status':'applied test-side fixes after actual177 and independent310all14controlprobe; focused tests pending','head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'selected_original_cases_unchanged_vs_trusted_base':selected,'paths':[],'limits':['actual177 prose about CI abolition is not adopted; repository complete gate remains mandatory','two ignored historical collector pins stay untouched; this is active-regression repair only']}
for p,b,a in files:
 assert (R/p).read_text()==b;(R/p).write_text(a);subprocess.run(['rustfmt','--edition','2021','--config','skip_children=true',str(R/p)],check=True);a=(R/p).read_text();patch.extend(difflib.unified_diff(b.splitlines(True),a.splitlines(True),fromfile='a/'+p,tofile='b/'+p));manifest['paths'].append({'path':p,'before_sha256':hashlib.sha256(b.encode()).hexdigest(),'after_sha256':hashlib.sha256(a.encode()).hexdigest()})
(O/'changes.patch.gz').write_bytes(gzip.compress(''.join(patch).encode(),mtime=0));(O/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n');print(json.dumps(manifest,indent=2))
