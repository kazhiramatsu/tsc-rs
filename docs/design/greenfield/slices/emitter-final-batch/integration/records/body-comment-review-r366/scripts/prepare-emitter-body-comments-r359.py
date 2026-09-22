from pathlib import Path
import json,hashlib,subprocess,difflib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-leading-binding-prototype');O=Path('/tmp/emitter-body-comments-r359');O.mkdir(exist_ok=False);p='crates/emitter/src/printer.rs';old=(Path('/tmp/emitter-system-eof-final-r351')/p).read_text();s=old
needle='''                let relocated_statement_list_comments = transformation
                    .arena()
                    .metadata(node)
                    .and_then(crate::EmitMetadata::relocated_statement_list_comments);'''
replacement='''                let function_body_range = if let Some(array) = array.filter(|_| function_body) {
                    let record = transformation.arena().node_array(array)?;
                    let source = transformation.arena().source(array.source())?.syntax();
                    match SourceRange::from_raw(record.pos, record.end, source.positions())? {
                        SourceRange::Original(range) => Some(range),
                        SourceRange::Synthesized => None,
                    }
                } else {
                    None
                };
'''+needle
assert s.count(needle)==1;s=s.replace(needle,replacement)
s=s.replace('''                let detached_body_prefix = if function_body
                    && !statements.is_empty()
                    && !transformation''','''                let detached_body_prefix = if function_body
                    && !transformation''')
a='''                    if let Some(relocated) = relocated_statement_list_comments {''';b='''                    if let Some(relocated) = relocated_statement_list_comments
                        .filter(|_| function_body_range.is_none())
                    {''';assert s.count(a)==1;s=s.replace(a,b)
a='''                // A relocated module body shares the original prefix boundary
                // only as a resume seed. Its outer SourceFile list retained the
                // parsed range and already emitted the prefix.
                let body_owned_detached_prefix = relocated_statement_list_comments
                    .is_none()
                    .then_some(detached_body_prefix)
                    .flatten();'''
b='''                // A ranged body owns its array's detached prefix even when
                // the outer SourceFile already emitted the same comment.
                // Only a synthesized relocated list uses that prefix solely
                // as a resume seed for its retained statements.
                let body_owned_detached_prefix = (function_body_range.is_some()
                    || relocated_statement_list_comments.is_none())
                    .then_some(detached_body_prefix)
                    .flatten();
                let mut pending_detached_comments =
                    PendingDetachedComments::from_prefix(detached_body_prefix);''';assert s.count(a)==1;s=s.replace(a,b)
a='''                if statements.is_empty() {
                    let emitted_comments = self.emit_empty_block_comments('''
b='''                if body_owned_detached_prefix.is_some() {
                    writer.increase_indent();
                    self.emit_detached_comment_prefix(
                        transformation,
                        body_owned_detached_prefix,
                        writer,
                    )?;
                    writer.decrease_indent();
                }
                if statements.is_empty() {
                    let emitted_comments = if function_body_range.is_some() {
                        // Function-body lists have no bracket comment lane.
                        // Their range owns a separate detached head and tail.
                        false
                    } else {
                        self.emit_empty_block_comments(''';assert s.count(a)==1;s=s.replace(a,b)
a='''                        function_body,
                        writer,
                    )?;
                    if !emitted_comments && multi_line {''';b='''                        function_body,
                        writer,
                    )?
                    };
                    if !emitted_comments && multi_line {''';assert s.count(a)==1;s=s.replace(a,b)
a='''                    self.emit_detached_comment_prefix(
                        transformation,
                        body_owned_detached_prefix,
                        writer,
                    )?;
                    let mut pending_detached_comments =
                        PendingDetachedComments::from_prefix(detached_body_prefix);
''';assert s.count(a)==1;s=s.replace(a,'')
a='''                    self.emit_comments_before_close_brace(
                        transformation,
                        node,
                        statement_list_end,
                        writer,
                    )?;
                    writer.decrease_indent();
                }
                // Close brace maps'''
b='''                    if !function_body {
                        self.emit_comments_before_close_brace(
                            transformation,
                            node,
                            statement_list_end,
                            writer,
                        )?;
                    }
                    writer.decrease_indent();
                }
                if let Some(range) = function_body_range {
                    writer.increase_indent();
                    self.emit_function_body_trailing_comments(
                        transformation,
                        node,
                        range.end(),
                        &mut pending_detached_comments,
                        writer,
                    )?;
                    writer.decrease_indent();
                }
                // Close brace maps''';assert s.count(a)==1;s=s.replace(a,b)
a='''    /// A relocated module body does not own an ordinary block statement-list
    /// prefix. Its prefix was emitted by the outer SourceFile, whose detached
    /// comment owner is the first parsed statement rather than the current
    /// (possibly synthesized) NodeArray boundary. Recreate that exact owner so
    /// `PendingDetachedComments` can resume the first retained statement and
    /// cannot emit the SourceFile-owned prefix a second time.'''
b='''    /// A relocated module body whose current list is synthesized uses the
    /// SourceFile-owned prefix only as a resume seed. Its owner is the first
    /// parsed statement, rather than the synthetic array boundary. A current
    /// list with a real range instead owns its own detached prefix, even when
    /// upstream deliberately emits the SourceFile prefix a second time.''';assert s.count(a)==1;s=s.replace(a,b)
marker='''    /// Emits the leading comments owned by a close-brace token. `tsc` anchors'''
helper='''    /// The tail of `emitBodyWithDetachedComments`, separate from the
    /// close-brace token's comment lane. Its owner is the function body's
    /// statement-array end, including a real EOF with no source brace.
    fn emit_function_body_trailing_comments(
        &self,
        transformation: &TransformationResult<'_>,
        body: TransformNode,
        end: SourceBytePosition,
        pending: &mut PendingDetachedComments,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.comments_disabled()
            || transformation.arena().metadata(body).is_some_and(|metadata| {
                metadata.flags().intersects(EmitFlags::NO_TRAILING_COMMENTS)
            })
        {
            return Ok(());
        }
        // When an empty list's end equals its detached-head owner, upstream
        // resumes at the last comment's end, not after its trailing whitespace.
        let position = pending
            .take_for(CommentCursor::new(body.source(), end))
            .map_or(end, |resume| resume.next().position());
        let source = transformation.arena().source(body.source())?.syntax();
        let before = writer.text_position();
        emit_source_leading_comments_of_position(
            source.text(),
            position.value() as usize,
            &BTreeSet::new(),
            self.options.only_print_js_doc_style,
            writer,
        );
        if writer.text_position() != before && !writer.is_at_start_of_line() {
            writer.write_line(false);
        }
        Ok(())
    }

''';assert s.count(marker)==1;s=s.replace(marker,helper+marker)
out=O/'printer.rs';out.write_text(s);subprocess.run(['rustfmt','--edition','2021','--config','skip_children=true',str(out)],check=True);s=out.read_text()
(O/'changes.patch').write_text(''.join(difflib.unified_diff(old.splitlines(True),s.splitlines(True),fromfile='a/'+p,tofile='b/'+p)))
report={'scope':'DRAFT range-owned body head/tail peractual183/184, based351typedguard. Originalrelocatedmetadata retained forcontainer-resume ownership; onlyprefixselectionusesrealrange. Emptyrealfunctionbody list skipsoldbracketpickup because official353emptyexecute hasonehead+onetail and upstreamlistformat noBracketsMask. Tailresume useslastcommentend, notwhitespace. NoNestedrouting remainsunchanged pendingactual185 witnessedflagreview.','base_sha256':hashlib.sha256(old.encode()).hexdigest(),'draft_sha256':hashlib.sha256(s.encode()).hexdigest(),'applied':False}
(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
