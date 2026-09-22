from pathlib import Path
import json,hashlib,difflib
O=Path('/tmp/emitter-body-final-r361');O.mkdir(exist_ok=False)
p=Path('/tmp/emitter-body-comments-r359/printer.rs');s=p.read_text();original=s
old='''        let enters_nested_comment_suppression = node_flags
            .intersects(EmitFlags::NO_NESTED_COMMENTS)
'''
new='''        // Function bodies apply their own NoNestedComments inside
        // emitBodyWithDetachedComments, after their detached head and before
        // their tail. Keep inherited suppression and the map/notification
        // pipeline intact.
        let function_body_comments = transformation.arena().node(node)?.kind == SyntaxKind::Block
            && self.is_function_body_block(transformation, node)?;
        let enters_nested_comment_suppression = !function_body_comments
            && node_flags.intersects(EmitFlags::NO_NESTED_COMMENTS)
'''
assert s.count(old)==1;s=s.replace(old,new)
old='''        let expression_context = if node_flags.intersects(EmitFlags::NO_NESTED_COMMENTS) {
            expression_context.with_nested_comments_suppressed()
'''
new='''        let expression_context = if !function_body_comments
            && node_flags.intersects(EmitFlags::NO_NESTED_COMMENTS)
        {
            expression_context.with_nested_comments_suppressed()
'''
assert s.count(old)==1;s=s.replace(old,new)
start=s.index('                if statements.is_empty() {',s.index('let body_owned_detached_prefix ='))
end=s.index('                if let Some(range) = function_body_range {',start)
body=s[start:end]
head='''                // tsc's emitBodyWithDetachedComments suppresses only the
                // list callback. A failed callback deliberately leaves the
                // shared printer suppressed for the next print operation.
                let enters_body_comment_suppression = function_body
                    && transformation.arena().metadata(node).is_some_and(|metadata| {
                        metadata.flags().intersects(EmitFlags::NO_NESTED_COMMENTS)
                    })
                    && !expression_context.nested_comments_suppressed()
                    && !self.comments_disabled();
                if enters_body_comment_suppression {
                    self.comments_disabled_after_failure = true;
                }
                let body_result = (|| -> Result<(), PrinterError> {
                    let expression_context = if enters_body_comment_suppression {
                        expression_context.with_nested_comments_suppressed()
                    } else {
                        expression_context
                    };
'''
tail='''                    Ok(())
                })();
                if body_result.is_ok() && enters_body_comment_suppression {
                    self.comments_disabled_after_failure = false;
                }
                body_result?;
'''
s=s[:start]+head+''.join('    '+line if line.strip() else line for line in body.splitlines(keepends=True))+tail+s[end:]
(O/'printer.rs').write_text(s)
(O/'changes-from-r359.patch').write_text(''.join(difflib.unified_diff(original.splitlines(True),s.splitlines(True),fromfile='r359/printer.rs',tofile='r361/printer.rs')))
(O/'manifest.json').write_text(json.dumps({'status':'draft; actual185 body callback extent; based351+359; not applied','sha256':hashlib.sha256(s.encode()).hexdigest()},indent=2)+'\n')
print(O)
