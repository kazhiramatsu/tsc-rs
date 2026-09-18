#!/usr/bin/env python3
"""r8 part 2 (EF2-DETACHED-COMMENT): tsc's detachedCommentsInfo is consumed by the FIRST node at
its nodePos at any nesting depth (forEachLeadingCommentToEmit → hasDetachedComments); the port's
statement-list-local PendingDetachedComments left a synthesized first statement's prefix to be
re-emitted by a nested original node. Carry the unconsumed source-level resume on the printer."""
def patch(path, pairs):
    s = open(path).read()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:100])
        s = s.replace(old, new)
    open(path, "w").write(s)

W = "/Users/hiramatsu/dev/tsc-rs-emitter-final/"
patch(W + "crates/emitter/src/printer.rs", [
("""    carried_generated_names: Option<CarriedGeneratedNames>,
""", """    carried_generated_names: Option<CarriedGeneratedNames>,
    /// The source file's detached-comment resume when its first statement
    /// could not consume it (synthesized start): tsc keeps
    /// `detachedCommentsInfo` on its stack until the first node at that
    /// position, at any depth, resumes from the detached end.
    carried_source_detached: std::cell::Cell<Option<CommentResume>>,
"""),
("""        carried_generated_names: None,
""", """        carried_generated_names: None,
        carried_source_detached: std::cell::Cell::new(None),
"""),
("""        let mut pending_detached_comments = PendingDetachedComments::default();
        let skipped_prologues = if mode == SourceFileEmitMode::Bundle {
""", """        let mut pending_detached_comments = PendingDetachedComments::default();
        self.carried_source_detached.set(None);
        let skipped_prologues = if mode == SourceFileEmitMode::Bundle {
"""),
("""            let detached_resume = self.take_detached_comment_resume_for_node(
                transformation,
                &mut pending_detached_comments,
                emitted,
            )?;
            let carried_resume = self.carried_container_owned_prefix(transformation, emitted)?;
""", """            let detached_resume = self.take_detached_comment_resume_for_node(
                transformation,
                &mut pending_detached_comments,
                emitted,
            )?;
            if detached_resume.is_none() {
                if let Some(resume) = pending_detached_comments.resume.take() {
                    self.carried_source_detached.set(Some(resume));
                }
            }
            let carried_resume = self.carried_container_owned_prefix(transformation, emitted)?;
"""),
("""    fn emit_leading_comments_for_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_leading_comments_for_node_worker(
            transformation,
            node,
            LeadingCommentContext::Normal,
            None,
            writer,
        )
    }
""", """    fn emit_leading_comments_for_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let resume = self.take_carried_source_detached_for_node(transformation, node)?;
        self.emit_leading_comments_for_node_worker(
            transformation,
            node,
            LeadingCommentContext::Normal,
            resume,
            writer,
        )
    }

    /// `hasDetachedComments(pos)` for a nested node: the source file's
    /// unconsumed detached prefix resumes at the first node whose leading
    /// walk starts at its nodePos (then it is gone, like tsc's stack pop).
    fn take_carried_source_detached_for_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<Option<CommentResume>, PrinterError> {
        let Some(carried) = self.carried_source_detached.get() else {
            return Ok(None);
        };
        let owner = self.expression_comment_phase_owner_for_node(transformation, node)?;
        if self.comments_disabled()
            || owner.flags.intersects(EmitFlags::NO_LEADING_COMMENTS)
            || owner.kind == SyntaxKind::JsxText
            || !owner.range.range().has_nonempty_extent()
        {
            return Ok(None);
        }
        let Some(start) = owner.range.range().start() else {
            return Ok(None);
        };
        if carried.owner_start() != CommentCursor::new(owner.range.source(), start) {
            return Ok(None);
        }
        self.carried_source_detached.set(None);
        Ok(Some(carried))
    }
"""),
])
print("r8 part 2 applied")
