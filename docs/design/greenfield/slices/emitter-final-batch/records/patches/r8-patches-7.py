#!/usr/bin/env python3
"""r8 part 7 (EF2-DETACHED-COMMENT follow-up): the source-file statement loop moved the pending
detached prefix into the printer carry at the first synthesized statement but only nested nodes
consulted the carry; a later top-level statement at nodePos (the retained declaration behind a
hoisted `exports.x = x`) re-scanned from position 0 and printed the detached header twice
(emitter lib test hoisted_exports_leave_detached_comments_for_the_function_declaration)."""
W = "/Users/hiramatsu/dev/tsc-rs-emitter-final/"
def patch(path, pairs):
    s = open(path).read()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:90])
        s = s.replace(old, new)
    open(path, "w").write(s)
patch(W + "crates/emitter/src/printer.rs", [
("""            if detached_resume.is_none() {
                if let Some(resume) = pending_detached_comments.resume.take() {
                    self.carried_source_detached.set(Some(resume));
                }
            }
            let carried_resume = self.carried_container_owned_prefix(transformation, emitted)?;
""", """            let detached_resume = match detached_resume {
                Some(resume) => Some(resume),
                None => {
                    if let Some(resume) = pending_detached_comments.resume.take() {
                        self.carried_source_detached.set(Some(resume));
                    }
                    // hasDetachedComments(pos) holds for the FIRST node whose
                    // leading walk starts at nodePos, at any depth: a later
                    // top-level statement (the retained declaration behind a
                    // hoisted `exports.x = x`) consumes the carried prefix too.
                    self.take_carried_source_detached_for_node(transformation, emitted)?
                }
            };
            let carried_resume = self.carried_container_owned_prefix(transformation, emitted)?;
"""),
])
print("r8 part 7 applied")
