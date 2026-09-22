#!/usr/bin/env python3
"""r7 part 5: EF4-FILE-THIS-CAPTURE — the transform-time `this` substitute carries the `this`
token's ContainsES2015 | ContainsLexicalThis flags and the ES2015 pass applies visitThisKeyword's
hierarchy facts to an identifier whose original is the `this` token."""
def patch(path, pairs):
    s = open(path).read()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:100])
        s = s.replace(old, new)
    open(path, "w").write(s)

W = "/Users/hiramatsu/dev/tsc-rs-emitter-final/"
patch(W + "crates/emitter/src/builtins/class_fields/downlevel.rs", [
("""    /// The transform flag a retained `this` token would carry.
    fn add_lexical_this_flag(&mut self, node: TransformNode) -> Result<(), TransformError> {
        let flags = self.context.arena().transform_flags(node);
        self.context
            .arena_mut()?
            .set_transform_flags(node, flags | TransformFlags::CONTAINS_LEXICAL_THIS);
        Ok(())
    }
""", """    /// The transform flags a retained `this` token carries
    /// (`ContainsES2015 | ContainsLexicalThis`, createToken): the ES2015 pass
    /// visits the substitute and applies `visitThisKeyword`'s hierarchy facts
    /// to it because its original is the `this` token.
    fn add_lexical_this_flag(&mut self, node: TransformNode) -> Result<(), TransformError> {
        let flags = self.context.arena().transform_flags(node);
        self.context.arena_mut()?.set_transform_flags(
            node,
            flags | TransformFlags::CONTAINS_ES2015 | TransformFlags::CONTAINS_LEXICAL_THIS,
        );
        Ok(())
    }
"""),
])
patch(W + "crates/emitter/src/builtins/es2015.rs", [
("""    fn visit_identifier(&mut self, node: TransformNode) -> Result<TransformNode, TransformError> {
        if self.converted_loop_state.is_some() {
            let is_arguments = {
""", """    fn visit_identifier(&mut self, node: TransformNode) -> Result<TransformNode, TransformError> {
        if self.is_static_this_substitute(node)? {
            // Class-fields substitutes a static initializer's `this` while
            // visiting, where tsc keeps the token until print-time
            // substitution; the retained token would reach visitThisKeyword,
            // so its hierarchy facts (`var _this = this` capture) apply here.
            self.note_lexical_this_use();
        }
        if self.converted_loop_state.is_some() {
            let is_arguments = {
"""),
("""    fn visit_this_keyword(&mut self, node: TransformNode) -> Result<TransformNode, TransformError> {
        self.print_state.hierarchy_facts = self
            .print_state
            .hierarchy_facts
            .union(HierarchyFacts::LEXICAL_THIS);
        if self
            .print_state
            .hierarchy_facts
            .intersects(HierarchyFacts::ARROW_FUNCTION)
            && !self
                .print_state
                .hierarchy_facts
                .intersects(HierarchyFacts::STATIC_INITIALIZER)
        {
            self.print_state.hierarchy_facts = self
                .print_state
                .hierarchy_facts
                .union(HierarchyFacts::CAPTURED_LEXICAL_THIS);
        }
        if self.converted_loop_state.is_some() {
""", """    /// An identifier standing in for a static initializer's `this`
    /// (class-fields' transform-time substitute keeps the token as its
    /// original and its transform flags).
    fn is_static_this_substitute(&self, node: TransformNode) -> Result<bool, TransformError> {
        if !self
            .transform_flags(node)
            .contains(TransformFlags::CONTAINS_LEXICAL_THIS)
        {
            return Ok(false);
        }
        let original = self.context.arena().get_original_node(node);
        Ok(original != node
            && self.context.arena().node(original)?.kind == SyntaxKind::ThisKeyword)
    }

    /// The hierarchy-fact half of `visitThisKeyword` (_tsc.js:105055-105068).
    fn note_lexical_this_use(&mut self) {
        self.print_state.hierarchy_facts = self
            .print_state
            .hierarchy_facts
            .union(HierarchyFacts::LEXICAL_THIS);
        if self
            .print_state
            .hierarchy_facts
            .intersects(HierarchyFacts::ARROW_FUNCTION)
            && !self
                .print_state
                .hierarchy_facts
                .intersects(HierarchyFacts::STATIC_INITIALIZER)
        {
            self.print_state.hierarchy_facts = self
                .print_state
                .hierarchy_facts
                .union(HierarchyFacts::CAPTURED_LEXICAL_THIS);
        }
        if self.converted_loop_state.is_some()
            && self
                .print_state
                .hierarchy_facts
                .intersects(HierarchyFacts::ARROW_FUNCTION)
        {
            self.loop_state_mut().contains_lexical_this = true;
        }
    }

    fn visit_this_keyword(&mut self, node: TransformNode) -> Result<TransformNode, TransformError> {
        self.print_state.hierarchy_facts = self
            .print_state
            .hierarchy_facts
            .union(HierarchyFacts::LEXICAL_THIS);
        if self
            .print_state
            .hierarchy_facts
            .intersects(HierarchyFacts::ARROW_FUNCTION)
            && !self
                .print_state
                .hierarchy_facts
                .intersects(HierarchyFacts::STATIC_INITIALIZER)
        {
            self.print_state.hierarchy_facts = self
                .print_state
                .hierarchy_facts
                .union(HierarchyFacts::CAPTURED_LEXICAL_THIS);
        }
        if self.converted_loop_state.is_some() {
"""),
])
print("r7 part 5 applied")
