use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct GeneratedBindingScopeId(usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GeneratedBindingOwner {
    Source,
    FunctionBody,
    StaticEvaluation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AncestorBindingPolicy {
    Reserve,
    AllowShadow,
}

#[derive(Debug)]
struct GeneratedBindingScope {
    owner: GeneratedBindingOwner,
    parent: Option<GeneratedBindingScopeId>,
    names: Vec<String>,
    bindings: Vec<String>,
    next_temp_ordinal: usize,
}

#[derive(Debug, Default)]
pub(super) struct GeneratedBindings(Vec<String>);

impl GeneratedBindings {
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(super) fn names(&self) -> &[String] {
        &self.0
    }
}

/// Owns generated names by the runtime scope in which their declarations and
/// initializations execute. Parsed identifiers are reserved for the whole
/// source, while generated names may be reused by sibling function scopes.
/// An active ancestor's generated bindings remain reserved in descendants.
///
/// This is shared by class lowering and the target-transform ladder. It
/// deliberately models scope ownership and allocation policy without owning
/// any syntax insertion; each transformer materializes declarations at the
/// boundary whose runtime semantics it controls.
#[derive(Debug)]
pub(super) struct GeneratedBindingScopes {
    reserved_source_names: BTreeSet<String>,
    ancestor_policy: AncestorBindingPolicy,
    scopes: Vec<GeneratedBindingScope>,
    current: GeneratedBindingScopeId,
}

impl GeneratedBindingScopes {
    pub(super) fn new(
        reserved_source_names: BTreeSet<String>,
        ancestor_policy: AncestorBindingPolicy,
    ) -> Self {
        Self {
            reserved_source_names,
            ancestor_policy,
            scopes: vec![GeneratedBindingScope {
                owner: GeneratedBindingOwner::Source,
                parent: None,
                names: Vec::new(),
                bindings: Vec::new(),
                next_temp_ordinal: 0,
            }],
            current: GeneratedBindingScopeId(0),
        }
    }

    pub(super) fn enter(
        &mut self,
        owner: GeneratedBindingOwner,
    ) -> (GeneratedBindingScopeId, GeneratedBindingScopeId) {
        let previous = self.current;
        let scope = GeneratedBindingScopeId(self.scopes.len());
        self.scopes.push(GeneratedBindingScope {
            owner,
            parent: Some(previous),
            names: Vec::new(),
            bindings: Vec::new(),
            next_temp_ordinal: 0,
        });
        self.current = scope;
        (previous, scope)
    }

    pub(super) fn exit(
        &mut self,
        previous: GeneratedBindingScopeId,
        completed: GeneratedBindingScopeId,
    ) -> GeneratedBindings {
        debug_assert_eq!(self.current, completed);
        debug_assert_ne!(
            self.scopes[completed.0].owner,
            GeneratedBindingOwner::Source
        );
        self.current = previous;
        GeneratedBindings(std::mem::take(&mut self.scopes[completed.0].bindings))
    }

    pub(super) fn source_bindings(&mut self) -> GeneratedBindings {
        debug_assert_eq!(self.current, GeneratedBindingScopeId(0));
        GeneratedBindings(std::mem::take(&mut self.scopes[0].bindings))
    }

    pub(super) fn allocate_temp(&mut self) -> String {
        loop {
            let ordinal = self.scopes[self.current.0].next_temp_ordinal;
            self.scopes[self.current.0].next_temp_ordinal += 1;
            let Some(candidate) = Self::temp_candidate(ordinal) else {
                continue;
            };
            if self.reserve_in_current(candidate.clone(), true) {
                return candidate;
            }
        }
    }

    pub(super) fn allocate_local_temp(&mut self) -> String {
        loop {
            let ordinal = self.scopes[self.current.0].next_temp_ordinal;
            self.scopes[self.current.0].next_temp_ordinal += 1;
            let Some(candidate) = Self::temp_candidate(ordinal) else {
                continue;
            };
            if self.reserve_in_current(candidate.clone(), false) {
                return candidate;
            }
        }
    }

    pub(super) fn allocate_preferred(&mut self, preferred: String) -> String {
        if self.reserve_in_current(preferred.clone(), true) {
            return preferred;
        }
        let mut suffix = 1usize;
        loop {
            let candidate = format!("{preferred}_{suffix}");
            if self.reserve_in_current(candidate.clone(), true) {
                return candidate;
            }
            suffix += 1;
        }
    }

    fn reserve_in_current(&mut self, candidate: String, binding: bool) -> bool {
        if self.reserved_source_names.contains(&candidate)
            || self.ancestor_policy == AncestorBindingPolicy::Reserve
                && self.active_scope_contains(&candidate)
        {
            return false;
        }
        self.scopes[self.current.0].names.push(candidate.clone());
        if binding {
            self.scopes[self.current.0].bindings.push(candidate);
        }
        true
    }

    fn active_scope_contains(&self, candidate: &str) -> bool {
        let mut scope = Some(self.current);
        while let Some(current) = scope {
            let current = &self.scopes[current.0];
            if current.names.iter().any(|name| name == candidate) {
                return true;
            }
            scope = current.parent;
        }
        false
    }

    /// tsc reserves `_i` and `_n` for its loop-variable name classes. The
    /// target ladder does not allocate loop variables yet, but ordinary temp
    /// ordinals must still leave those slots untouched so later names remain
    /// byte-for-byte compatible with the shared TypeScript name generator.
    fn temp_candidate(ordinal: usize) -> Option<String> {
        if matches!(ordinal, 8 | 13) {
            return None;
        }
        Some(if ordinal < 26 {
            format!("_{}", char::from(b'a' + ordinal as u8))
        } else {
            format!("_{}", ordinal - 26)
        })
    }
}
