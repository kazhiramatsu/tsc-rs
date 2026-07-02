//! typeToString parity: byte-identical type display.

use super::Checker;
use crate::ast::{KeywordTypeKind, MappedModifier, MappedTypeNode, PropName, Span, TypeNode};
use crate::types::{TypeId, TypeKind};

impl<'a> Checker<'a> {
    fn node_source_text(&self, span: Span, file: usize) -> String {
        let text = &self.files[file].1.text;
        text.get(span.start as usize..span.end as usize)
            .unwrap_or("?")
            .to_string()
    }

    pub(crate) fn display_prop_name_for_error(&self, name: &PropName) -> String {
        match name {
            PropName::Ident(i) => i.name.clone(),
            PropName::String { span, value } => self
                .source_text_for_current_file(*span)
                .unwrap_or_else(|| format!("\"{}\"", escape_string_for_display(value))),
            PropName::Number { text, .. } => text.clone(),
            PropName::Computed { span, .. } => {
                self.source_text_for_current_file(*span).unwrap_or_default()
            }
        }
    }

    fn source_text_for_current_file(&self, span: Span) -> Option<String> {
        let text = &self.files[self.current_file].1.text;
        text.get(span.start as usize..span.end as usize)
            .map(str::to_string)
    }

    pub fn display_type(&mut self, t: TypeId) -> String {
        self.display_type_depth(t, 0)
    }

    fn display_type_depth(&mut self, t: TypeId, depth: usize) -> String {
        if depth > 8 {
            return "...".to_string();
        }
        // boolean intrinsic (false | true) prints as 'boolean'
        if t == self.types.boolean {
            return "boolean".to_string();
        }
        // alias display (type X = ...) wins for structured types
        if let Some((sym, args)) = self.types.alias(t).cloned() {
            let name = self.symbol(sym).name.clone();
            if args.is_empty() {
                return name;
            }
            let args_s: Vec<String> = args
                .iter()
                .map(|&a| self.display_type_depth(a, depth + 1))
                .collect();
            return format!("{}<{}>", name, args_s.join(", "));
        }
        match self.types.kind(t).clone() {
            TypeKind::Any => "any".into(),
            TypeKind::Unknown => "unknown".into(),
            TypeKind::Error => "error".into(),
            TypeKind::Undefined => "undefined".into(),
            TypeKind::Null => "null".into(),
            TypeKind::String => "string".into(),
            TypeKind::Number => "number".into(),
            TypeKind::Bigint => "bigint".into(),
            TypeKind::EsSymbol => "symbol".into(),
            TypeKind::Void => "void".into(),
            TypeKind::Never => "never".into(),
            TypeKind::NonPrimitive => "object".into(),
            TypeKind::StrLit(s) => format!("\"{}\"", s.display_escaped()),
            TypeKind::NumLit(bits) => crate::js_num::to_js_string(f64::from_bits(bits)),
            TypeKind::BigIntLit(text) => text,
            TypeKind::BoolLit(b) => {
                if b {
                    "true".into()
                } else {
                    "false".into()
                }
            }
            TypeKind::TypeParam(sym) => self.symbol(sym).name.clone(),
            TypeKind::Iface(sym) => self.generic_name_with_params(sym),
            TypeKind::MappedIface(sym, _) => self.generic_name_with_params(sym),
            TypeKind::ClassStatics(sym) => format!("typeof {}", self.symbol(sym).name),
            TypeKind::MappedClassStatics(sym, _) => format!("typeof {}", self.symbol(sym).name),
            TypeKind::ReadonlyArray(e) => {
                let inner = self.display_type_depth(e, depth + 1);
                let needs_parens = self.element_needs_parens(e);
                if needs_parens {
                    format!("readonly ({})[]", inner)
                } else {
                    format!("readonly {}[]", inner)
                }
            }
            TypeKind::Ref(sym, args) => {
                // Array<T> prints as T[]
                if Some(sym) == self.array_symbol() && args.len() == 1 {
                    let inner = self.display_type_depth(args[0], depth + 1);
                    if self.element_needs_parens(args[0]) {
                        return format!("({})[]", inner);
                    }
                    return format!("{}[]", inner);
                }
                let name = self.symbol(sym).name.clone();
                let args_s: Vec<String> = args
                    .iter()
                    .map(|&a| self.display_type_depth(a, depth + 1))
                    .collect();
                format!("{}<{}>", name, args_s.join(", "))
            }
            TypeKind::Tuple(elems) => {
                let parts: Vec<String> = elems
                    .iter()
                    .map(|e| {
                        let inner = self.display_type_depth(e.ty, depth + 1);
                        if e.rest {
                            format!("...{}[]", inner)
                        } else if e.optional {
                            format!("{}?", inner)
                        } else {
                            inner
                        }
                    })
                    .collect();
                format!("[{}]", parts.join(", "))
            }
            TypeKind::EnumType(sym) => self.symbol(sym).name.clone(),
            TypeKind::EnumObject(sym) | TypeKind::NamespaceObj(sym) => {
                format!("typeof {}", self.symbol(sym).name)
            }
            TypeKind::Keyof(inner) => {
                format!("keyof {}", self.display_type_depth(inner, depth + 1))
            }
            TypeKind::TemplateLit(parts) => {
                let mut s = String::from("`");
                for p in parts {
                    match p {
                        crate::types::TplPart::Str(text) => s.push_str(&text),
                        crate::types::TplPart::Ty(t2) => {
                            s.push_str("${");
                            s.push_str(&self.display_type_depth(t2, depth + 1));
                            s.push('}');
                        }
                    }
                }
                s.push('`');
                s
            }
            TypeKind::DeferredCond(key, _) => {
                // render the source text of the conditional node (fixtures use
                // canonical spacing, matching tsc's reconstruction)
                match self.deferred.deferred_conds.get(&key) {
                    Some(&(node, _, file)) => self.node_source_text(node.span, file),
                    None => "?".to_string(),
                }
            }
            TypeKind::DeferredMapped(key, _) => match self.deferred.deferred_mappeds.get(&key) {
                Some(&(node, _, file)) => self.display_mapped_type_node(node, file),
                None => "?".to_string(),
            },
            TypeKind::IndexedAccess(obj, idx) => format!(
                "{}[{}]",
                self.display_type_depth(obj, depth + 1),
                self.display_type_depth(idx, depth + 1)
            ),
            TypeKind::ReadonlyTuple(elems) => {
                let parts: Vec<String> = elems
                    .iter()
                    .map(|e| {
                        let inner = self.display_type_depth(e.ty, depth + 1);
                        if e.rest {
                            format!("...{}[]", inner)
                        } else if e.optional {
                            format!("{}?", inner)
                        } else {
                            inner
                        }
                    })
                    .collect();
                format!("readonly [{}]", parts.join(", "))
            }
            TypeKind::EnumMember(msym) => {
                let parent = self.symbol(msym).parent;
                let mname = self.symbol(msym).name.clone();
                match parent {
                    Some(p) => format!("{}.{}", self.symbol(p).name, mname),
                    None => mname,
                }
            }
            TypeKind::Union(members) => self.display_union(&members, depth),
            TypeKind::Intersection(members) => self.display_intersection(&members, depth),
            TypeKind::DeferredObj(_) => {
                let Some(sid) = self.shape_of_type(t) else {
                    return "{}".to_string();
                };
                let anon = self.types.alloc(TypeKind::Anon(sid));
                self.display_type_depth(anon, depth)
            }
            TypeKind::Anon(shape_id) => {
                let shape = self.types.shape(shape_id).clone();
                // pure function type
                if shape.props.is_empty()
                    && shape.index_infos.is_empty()
                    && shape.call_sigs.len() == 1
                    && shape.ctor_sigs.is_empty()
                {
                    return self.display_signature(shape.call_sigs[0], depth, false);
                }
                // pure constructor type
                if shape.props.is_empty()
                    && shape.index_infos.is_empty()
                    && shape.call_sigs.is_empty()
                    && shape.ctor_sigs.len() == 1
                {
                    return self.display_signature(shape.ctor_sigs[0], depth, true);
                }
                if shape.props.is_empty()
                    && shape.call_sigs.is_empty()
                    && shape.ctor_sigs.is_empty()
                    && shape.index_infos.is_empty()
                {
                    return "{}".to_string();
                }
                let mut parts: Vec<String> = Vec::new();
                for &sig in &shape.call_sigs {
                    parts.push(self.display_call_sig_member(sig, depth, false));
                }
                for &sig in &shape.ctor_sigs {
                    parts.push(format!(
                        "new {}",
                        self.display_call_sig_member(sig, depth, false)
                    ));
                }
                for info in &shape.index_infos {
                    let k = self.display_type_depth(info.key, depth + 1);
                    let v = self.display_type_depth(info.value, depth + 1);
                    parts.push(format!("[x: {}]: {}", k, v));
                }
                for p in &shape.props {
                    let ty = self.display_type_depth(p.ty, depth + 1);
                    if p.is_method {
                        // method display: reuse the signature form
                        if let TypeKind::Anon(ms) = self.types.kind(p.ty).clone() {
                            let mshape = self.types.shape(ms).clone();
                            if mshape.call_sigs.len() == 1 && mshape.props.is_empty() {
                                parts.push(format!(
                                    "{}{}{}",
                                    display_prop_name(&p.name),
                                    if p.optional { "?" } else { "" },
                                    self.display_call_sig_member(mshape.call_sigs[0], depth, true)
                                ));
                                continue;
                            }
                        }
                    }
                    parts.push(format!(
                        "{}{}{}: {}",
                        if p.readonly { "readonly " } else { "" },
                        display_prop_name(&p.name),
                        if p.optional { "?" } else { "" },
                        ty
                    ));
                }
                let mut s = String::from("{ ");
                for p in &parts {
                    s.push_str(p);
                    s.push_str("; ");
                }
                s.push('}');
                s
            }
        }
    }

    fn element_needs_parens(&self, t: TypeId) -> bool {
        if t == self.types.boolean {
            return false;
        }
        match self.types.kind(t) {
            TypeKind::Union(_) => true,
            TypeKind::Intersection(_) => true,
            TypeKind::Anon(s) => {
                let shape = self.types.shape(*s);
                shape.props.is_empty() && (shape.call_sigs.len() == 1 || shape.ctor_sigs.len() == 1)
            }
            _ => false,
        }
    }

    fn display_mapped_type_node(&self, node: &MappedTypeNode, file: usize) -> String {
        let mut out = String::from("{ ");
        match node.readonly_mod {
            Some(MappedModifier::Add) => out.push_str("readonly "),
            Some(MappedModifier::Remove) => out.push_str("-readonly "),
            None => {}
        }
        out.push('[');
        out.push_str(&node.key.name);
        out.push_str(" in ");
        out.push_str(&self.type_node_source_text(&node.constraint, file));
        if let Some(name_type) = &node.name_type {
            out.push_str(" as ");
            out.push_str(&self.type_node_source_text(name_type, file));
        }
        out.push(']');
        match node.optional_mod {
            Some(MappedModifier::Add) => out.push('?'),
            Some(MappedModifier::Remove) => out.push_str("-?"),
            None => {}
        }
        if let Some(value) = &node.value {
            out.push_str(": ");
            out.push_str(&self.display_mapped_value_type_node(
                value,
                matches!(node.optional_mod, Some(MappedModifier::Add)),
                file,
            ));
        }
        out.push_str("; }");
        out
    }

    fn display_mapped_value_type_node(
        &self,
        node: &TypeNode,
        optional: bool,
        file: usize,
    ) -> String {
        if optional
            && self.options.strict_null_checks()
            && !self.options.exact_optional_property_types
        {
            return self.display_type_node_with_optional_undefined(node, file);
        }
        self.type_node_source_text(node, file)
    }

    fn display_type_node_with_optional_undefined(&self, node: &TypeNode, file: usize) -> String {
        match node {
            TypeNode::Paren { inner, .. } => {
                self.display_type_node_with_optional_undefined(inner, file)
            }
            TypeNode::Union { members, .. } => {
                let mut parts = Vec::new();
                let mut has_undefined = false;
                for member in members {
                    if self.type_node_is_never(member) {
                        continue;
                    }
                    if self.type_node_is_undefined(member) {
                        has_undefined = true;
                        continue;
                    }
                    parts.push(self.display_type_node_as_union_member(member, file));
                }
                if !has_undefined {
                    parts.push("undefined".to_string());
                }
                if parts.is_empty() {
                    "undefined".to_string()
                } else {
                    parts.join(" | ")
                }
            }
            _ if self.type_node_is_never(node) || self.type_node_is_undefined(node) => {
                "undefined".to_string()
            }
            _ => {
                let raw = self.type_node_source_text(node, file);
                if self.type_node_needs_union_parens(node) {
                    format!("({}) | undefined", raw)
                } else {
                    format!("{} | undefined", raw)
                }
            }
        }
    }

    fn display_type_node_as_union_member(&self, node: &TypeNode, file: usize) -> String {
        match node {
            TypeNode::Paren { inner, .. } => self.display_type_node_as_union_member(inner, file),
            _ => {
                let raw = self.type_node_source_text(node, file);
                if self.type_node_needs_union_parens(node) {
                    format!("({})", raw)
                } else {
                    raw
                }
            }
        }
    }

    fn type_node_is_undefined(&self, node: &TypeNode) -> bool {
        matches!(node, TypeNode::Keyword(KeywordTypeKind::Undefined, _))
    }

    fn type_node_is_never(&self, node: &TypeNode) -> bool {
        matches!(node, TypeNode::Keyword(KeywordTypeKind::Never, _))
    }

    fn type_node_needs_union_parens(&self, node: &TypeNode) -> bool {
        matches!(
            node,
            TypeNode::Function(_)
                | TypeNode::Ctor(_)
                | TypeNode::Intersection { .. }
                | TypeNode::Conditional(_)
        )
    }

    fn type_node_source_text(&self, node: &TypeNode, file: usize) -> String {
        self.node_source_text(node.span(), file)
    }

    fn display_intersection(&mut self, members: &[TypeId], depth: usize) -> String {
        // intersections keep declaration order; union/function operands are
        // parenthesized so `(A | B) & C` reads unambiguously.
        let parts: Vec<String> = members
            .iter()
            .map(|&m| {
                let s = self.display_type_depth(m, depth + 1);
                if matches!(self.types.kind(m), TypeKind::Union(_)) {
                    format!("({})", s)
                } else {
                    s
                }
            })
            .collect();
        parts.join(" & ")
    }

    fn display_union(&mut self, members: &[TypeId], depth: usize) -> String {
        // formatUnionTypes: null/undefined move to the end (null then undefined);
        // adjacent false,true collapse to boolean.
        let mut main: Vec<TypeId> = Vec::new();
        let mut has_null = false;
        let mut has_undefined = false;
        let mut i = 0;
        while i < members.len() {
            let m = members[i];
            match self.types.kind(m) {
                TypeKind::Null => has_null = true,
                TypeKind::Undefined => has_undefined = true,
                TypeKind::BoolLit(false)
                    if i + 1 < members.len()
                        && matches!(self.types.kind(members[i + 1]), TypeKind::BoolLit(true)) =>
                {
                    main.push(self.types.boolean);
                    i += 1;
                }
                _ => main.push(m),
            }
            i += 1;
        }
        if has_null {
            main.push(self.types.null);
        }
        if has_undefined {
            main.push(self.types.undefined);
        }
        let has_number = main
            .iter()
            .any(|&m| matches!(self.types.kind(m), TypeKind::Number));
        let has_string = main
            .iter()
            .any(|&m| matches!(self.types.kind(m), TypeKind::String));
        if has_number || has_string {
            let filtered: Vec<TypeId> = main
                .iter()
                .copied()
                .filter(|&m| !self.enum_subsumed_by_display_primitive(m, has_number, has_string))
                .collect();
            if !filtered.is_empty() {
                main = filtered;
            }
        }
        let parts: Vec<String> = main
            .iter()
            .map(|&m| {
                let s = self.display_type_depth(m, depth + 1);
                if self.union_member_needs_parens(m) {
                    format!("({})", s)
                } else {
                    s
                }
            })
            .collect();
        parts.join(" | ")
    }

    fn enum_subsumed_by_display_primitive(
        &mut self,
        t: TypeId,
        has_number: bool,
        has_string: bool,
    ) -> bool {
        match self.types.kind(t) {
            TypeKind::EnumType(_) | TypeKind::EnumMember(_) => {
                let (numeric, string) = self.enum_member_kinds_of(t);
                (has_number && numeric && !string) || (has_string && string && !numeric)
            }
            _ => false,
        }
    }

    fn union_member_needs_parens(&self, t: TypeId) -> bool {
        match self.types.kind(t) {
            TypeKind::Anon(s) => {
                let shape = self.types.shape(*s);
                shape.props.is_empty()
                    && shape.index_infos.is_empty()
                    && (shape.call_sigs.len() == 1 || shape.ctor_sigs.len() == 1)
                    && (shape.call_sigs.len() + shape.ctor_sigs.len() == 1)
            }
            _ => false,
        }
    }

    /// `(x: string) => void` (`is_ctor` adds `new `)
    pub(crate) fn display_signature(
        &mut self,
        sig: crate::types::SigId,
        depth: usize,
        is_ctor: bool,
    ) -> String {
        let tps = self.display_sig_type_params(sig);
        let params = self.display_params(sig, depth);
        let ret = {
            let r = self.sig_return(sig);
            self.display_type_depth(r, depth + 1)
        };
        if is_ctor {
            format!("new {}({}) => {}", tps, params, ret)
        } else {
            format!("{}({}) => {}", tps, params, ret)
        }
    }

    /// `(x: string): void` (member form)
    fn display_call_sig_member(
        &mut self,
        sig: crate::types::SigId,
        depth: usize,
        member_form: bool,
    ) -> String {
        let tps = self.display_sig_type_params(sig);
        let params = self.display_params(sig, depth);
        let r = self.sig_return(sig);
        let ret = self.display_type_depth(r, depth + 1);
        if member_form {
            format!("{}({}): {}", tps, params, ret)
        } else {
            format!("{}({}): {}", tps, params, ret)
        }
    }

    pub(crate) fn display_sig_type_params(&self, sig: crate::types::SigId) -> String {
        let params = self.types.sig(sig).type_params.clone();
        if params.is_empty() {
            return String::new();
        }
        let names: Vec<String> = params
            .iter()
            .map(|&p| self.symbol(p).name.clone())
            .collect();
        format!("<{}>", names.join(", "))
    }

    fn display_params(&mut self, sig: crate::types::SigId, depth: usize) -> String {
        let s = self.types.sig(sig).clone();
        let mut parts: Vec<String> = Vec::new();
        for p in &s.params {
            let ty = self.display_type_depth(p.ty, depth + 1);
            parts.push(format!(
                "{}{}: {}",
                p.name,
                if p.optional { "?" } else { "" },
                ty
            ));
        }
        if let Some(rest) = s.rest {
            let ty = self.display_type_depth(rest, depth + 1);
            let name = s.rest_name.as_deref().unwrap_or("args");
            parts.push(format!("...{}: {}[]", name, ty));
        }
        parts.join(", ")
    }
}

fn display_prop_name(name: &str) -> String {
    let valid_ident = !name.is_empty()
        && name.chars().enumerate().all(|(i, c)| {
            c == '_' || c == '$' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit())
        });
    if valid_ident {
        name.to_string()
    } else if name.chars().all(|c| c.is_ascii_digit()) {
        name.to_string()
    } else {
        format!("\"{}\"", escape_string_for_display(name))
    }
}

fn escape_string_for_display(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}
