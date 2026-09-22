from pathlib import Path
import hashlib,json
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');out=Path('/tmp/emitter-layout-followup-r493');changes=[];proposals=[]
def update(path,edit):
 p=R/path;old=p.read_text();new=edit(old);assert old!=new,path;proposals.append((p,old,new))
 changes.append(dict(path=path,before_sha256=hashlib.sha256(old.encode()).hexdigest(),after_sha256=hashlib.sha256(new.encode()).hexdigest()))
def once(s,a,b):
 assert s.count(a)==1,a;return s.replace(a,b)
def rust(s):
 guard='    if violations.is_empty() {\n        return Err("frozen layout exception no longer needed; retire it".into());\n    }\n'
 s=once(s,guard,'');s=once(s,'    if contract["sha256"] !=',guard+'    if contract["sha256"] !=')
 return once(s,'    if std::str::from_utf8(bytes)?.lines().nth(line)', '    // cfg_line is one-based; zero-based nth(line) selects its following module declaration.\n    if std::str::from_utf8(bytes)?.lines().nth(line)')
def python(s):
 guard='    if not violations:\n        raise ValueError("frozen layout exception no longer needed; retire it")\n'
 s=once(s,guard,'');s=once(s,'    if hashlib.sha256(raw)',guard+'    if hashlib.sha256(raw)')
 removed='            b\'#[cfg(test)]\\n#[path = "../tests/unit/recovery_parse_snapshot/tests.rs"]\\nmod tests;\\n\',\n'
 s=once(s,removed,'')
 point='        frozen.write_bytes(raw)\n        other.write_bytes(raw)'
 addition='        frozen.write_bytes(b\'#[cfg(test)]\\n#[path = "../tests/unit/recovery_parse_snapshot/tests.rs"]\\nmod tests;\\n\')\n        check(1, "frozen layout exception no longer needed")\n'
 return once(s,point,addition+point)
def rusttests(s):
 old='        "#[cfg(test)]\\n#[path = \\"../tests/unit/recovery_parse_snapshot/tests.rs\\"]\\nmod tests;\\n"\n            .to_owned(),\n'
 # Use the literal as present in the Rust source, preserving escaped quotes.
 old=old.replace('\\\\"','\\"')
 s=once(s,old,'')
 point='    fs::write(&frozen_path, &original).unwrap();\n    workspace.write("crates/xtask/src/other.rs", &original);'
 addition='''    workspace.write(
        FROZEN_TEST_SOURCE,
        "#[cfg(test)]\\n#[path = \\"../tests/unit/recovery_parse_snapshot/tests.rs\\"]\\nmod tests;\\n",
    );
    assert!(audit_unit_test_layout(&catalog)
        .unwrap_err()
        .to_string()
        .contains("frozen layout exception no longer needed"));
'''.replace('\\\\"','\\"')
 s=once(s,point,addition+point)
 point='    install_frozen_layout_fixture(&workspace);\n    audit_unit_test_layout(&catalog).unwrap();\n}\n'
 replacement='''    install_frozen_layout_fixture(&workspace);
    audit_unit_test_layout(&catalog).unwrap();
    // The pinned file can remain on disk while Cargo stops enumerating its crate.
    workspace.write("Cargo.toml", "[workspace]\\nmembers = [\\"other\\"]\\nresolver = \\"2\\"\\n");
    workspace.write(
        "other/Cargo.toml",
        "[package]\\nname = \\"unrelated-layout-fixture\\"\\nversion = \\"0.0.0\\"\\nedition = \\"2021\\"\\n[package.metadata.tsc-rs]\\nrole = \\"other\\"\\n",
    );
    workspace.write("other/src/lib.rs", "");
    workspace.write("other/tests/.keep", "");
    let catalog_without_frozen_crate = WorkspaceCatalog::discover(workspace.path()).unwrap();
    assert!(audit_unit_test_layout(&catalog_without_frozen_crate)
        .unwrap_err()
        .to_string()
        .contains("frozen layout source was not scanned"));
}
'''.replace('\\\\"','\\"')
 return once(s,point,replacement)
update('crates/xtask/src/workspace_maintenance.rs',rust)
update('scripts/inline-tests-scan.py',python)
update('crates/xtask/tests/unit/workspace_maintenance/tests.rs',rusttests)
update('scripts/chain-walk.sh',lambda s:once(s,'shasum -a 256 Cargo.toml Cargo.lock; rustc --version;', 'shasum -a 256 Cargo.toml Cargo.lock scripts/inline-tests-scan.py scripts/frozen-test-layout.json; rustc --version;'))
update('scripts/frozen-test-layout.json',lambda s:once(s,'This one layout exception preserves that reference identity.', 'This one layout exception preserves that reference identity. Both gates share this descriptor; rebuild xtask after editing it because Rust includes it at compile time.'))
def pins(s):
 data=json.loads(s);name='crates/harness/src/upstream_suites/execution/project.rs';assert data[name]=='c03c49fb14243ea4a49984df4f4ae68134276558dac32f9c17ffbfd0bca398cf';new=hashlib.sha256((R/name).read_bytes()).hexdigest();assert new=='42f04317f5fb348fb0e5ed69ec5d7276b688676c915e0a539fff5cdf1582b8ef';return once(s,data[name],new)
update('scripts/recovery-command-pins.json',pins)
for path,old,new in proposals:
 assert path.read_text()==old
for path,old,new in proposals:path.write_text(new)
(out/'applied.json').write_text(json.dumps(changes,indent=2)+'\n')
print(json.dumps(changes))
