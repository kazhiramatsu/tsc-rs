from pathlib import Path
import json,hashlib,difflib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-leading-binding-prototype');O=Path('/tmp/emitter-system-eof-review-r345');O.mkdir(exist_ok=False)
p=R/'crates/emitter/src/printer.rs';old=p.read_text();s=old
s=s.replace('''        let end = token_start.checked_add(spelling.len())?;''','''        // Source slice ends use bytes; map columns use UTF-16 units.
        let end = token_start.checked_add(spelling.len())?;''')
s=s.replace('''self.token_map_range_at(
                                transformation,
                                cursor_source,
                                position.value(),
                                writer,''','''self.token_map_range_spanning(
                                transformation,
                                cursor_source,
                                position.value(),
                                ":",
                                writer,''')
for owner in ['node.source()', 'source']:
 for anchor,spelling in [('start','{'),('end','}')]:
  s=s.replace(f'self.token_map_range_at(transformation, {owner}, {anchor}, writer)?',f'self.token_map_range_spanning(transformation, {owner}, {anchor}, "{spelling}", writer)?')
a=s.index('    /// A one-token default range anchored at a raw source position:')
b=s.index('    fn token_map_range_spanning(',a)
s=s[:a]+'''    /// A token default anchored at a raw source position. The Before
    /// side skips trivia; the After side follows the token's spelling.
    /// At EOF that continuation is a map column, not a source byte range.
'''+s[b:]
assert 'token_map_range_at' not in s
(O/'printer.rs').write_text(s)
(O/'review-cleanup.patch').write_text(''.join(difflib.unified_diff(old.splitlines(True),s.splitlines(True),fromfile='a/crates/emitter/src/printer.rs',tofile='b/crates/emitter/src/printer.rs')))
manifest={'scope':'Behavior-neutral spelling provenance cleanup accepted from actual Opus181. DRAFT ONLY: do not apply while342 runner freezes product bytes. Existing colon/open/close punctuation each remains one UTF8 byte and one UTF16 unit. No default construction/dispatch semantics change.','old_sha256':hashlib.sha256(old.encode()).hexdigest(),'new_sha256':hashlib.sha256(s.encode()).hexdigest(),'applied':False}
(O/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps(manifest))
