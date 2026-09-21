from pathlib import Path
import re,json
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');changed=[]
for package,expected in [('compiler',30),('harness',2)]:
 root=r/f'crates/{package}/tests/contracts.rs';s=root.read_text();modules=re.findall(r'#\[path = "(integration/[^"\n]+\.rs)"\]',s);count=0
 for module in modules:
  p=root.parent/module;t=p.read_text();old=t
  for name,parent,member in [('path','host','ScalarTestPath as _'),('json','program','observe as scalar_json')]:
   block=f'#[path = "../../../{parent}/tests/support/scalar_{name}.rs"]\nmod utf16_scalar_{name};\nuse utf16_scalar_{name}::{member};'
   if block in t:
    assert t.count(block)==1
    t=t.replace(block,f'use crate::utf16_scalar_{name}::{member};')
  if old!=t:
   p.write_text(t);changed.append(str(p.relative_to(r)));count+=1
 assert count==expected,(package,count)
 assert 'mod utf16_scalar' not in s
 prefix='// Share each unchanged scalar fixture observer once per test crate.\n#[path = "../../program/tests/support/scalar_json.rs"]\nmod utf16_scalar_json;\n#[path = "../../host/tests/support/scalar_path.rs"]\nmod utf16_scalar_path;\n\n'
 root.write_text(prefix+s);changed.append(str(root.relative_to(r)))
# source_map_band_probe also compiles under the standalone map projection root;
# that root already owns its one scalar_path declaration. Its bytes stay intact.
p=r/'crates/compiler/tests/h2_6a_map_option_projection.rs'
assert '#[path = "../../host/tests/support/scalar_path.rs"]\nmod utf16_scalar_path;' in p.read_text()
assert '#[path = "../../../program/tests/support/scalar_json.rs"]\nmod utf16_scalar_json;' in (r/'crates/compiler/tests/integration/h2_8a_decorator_super.rs').read_text()
Path('/tmp/emitter-shared-test-modules-r241-paths.json').write_text(json.dumps(sorted(changed),indent=2)+'\n')
print('Centralized helper declarations in',len(changed),'files')
