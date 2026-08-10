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
    names_reserved_in_descendants: Vec<String>,
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
                names_reserved_in_descendants: Vec::new(),
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
            names_reserved_in_descendants: Vec::new(),
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
            if self.reserve_in_current(candidate.clone(), true, false) {
                return candidate;
            }
        }
    }

    pub(super) fn allocate_temp_with_policy(&mut self, reserve_in_nested_scopes: bool) -> String {
        loop {
            let ordinal = self.scopes[self.current.0].next_temp_ordinal;
            self.scopes[self.current.0].next_temp_ordinal += 1;
            let Some(candidate) = Self::temp_candidate(ordinal) else {
                continue;
            };
            if self.reserve_in_current(candidate.clone(), true, reserve_in_nested_scopes) {
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
            if self.reserve_in_current(candidate.clone(), false, false) {
                return candidate;
            }
        }
    }

    pub(super) fn allocate_preferred(&mut self, preferred: String) -> String {
        if self.reserve_in_current(preferred.clone(), true, false) {
            return preferred;
        }
        let mut suffix = 1usize;
        loop {
            let candidate = format!("{preferred}_{suffix}");
            if self.reserve_in_current(candidate.clone(), true, false) {
                return candidate;
            }
            suffix += 1;
        }
    }

    pub(super) fn allocate_local_preferred_with_policy(
        &mut self,
        preferred: String,
        reserve_in_nested_scopes: bool,
    ) -> String {
        if self.reserve_in_current(preferred.clone(), false, reserve_in_nested_scopes) {
            return preferred;
        }
        let mut suffix = 1usize;
        loop {
            let candidate = format!("{preferred}_{suffix}");
            if self.reserve_in_current(candidate.clone(), false, reserve_in_nested_scopes) {
                return candidate;
            }
            suffix += 1;
        }
    }

    pub(super) fn allocate_numbered(&mut self, source_name: &str) -> String {
        let mut suffix = 1usize;
        loop {
            let candidate = format!("{source_name}_{suffix}");
            if self.reserve_in_current(candidate.clone(), true, false) {
                return candidate;
            }
            suffix += 1;
        }
    }

    /// Allocates the numbered form used when a generated declaration derives
    /// its identity from a parsed node (`name_1`, `name_2`, ...). Unlike a
    /// preferred name collision, the ordinal belongs to the source name and
    /// must therefore advance as a unit rather than produce `name_1_1`.
    pub(super) fn allocate_local_numbered(&mut self, source_name: &str) -> String {
        let mut suffix = 1usize;
        loop {
            let candidate = format!("{source_name}_{suffix}");
            if self.reserve_in_current(candidate.clone(), false, false) {
                return candidate;
            }
            suffix += 1;
        }
    }

    /// Reconciles an eagerly planned source-derived name with bindings added
    /// by later transforms. The planned ordinal is retained when available;
    /// on collision the same source-name sequence advances (`e_1` -> `e_2`)
    /// instead of treating the full spelling as a new preferred base.
    pub(super) fn allocate_source_planned_numbered(
        &mut self,
        source_name: &str,
        planned: String,
    ) -> String {
        self.allocate_source_planned_numbered_with_policy(source_name, planned, false)
    }

    pub(super) fn allocate_source_planned_numbered_with_policy(
        &mut self,
        source_name: &str,
        planned: String,
        reserve_in_nested_scopes: bool,
    ) -> String {
        if self.reserve_in_source(planned.clone()) {
            if reserve_in_nested_scopes {
                self.scopes[0]
                    .names_reserved_in_descendants
                    .push(planned.clone());
            }
            return planned;
        }
        let prefix = format!("{source_name}_");
        let mut suffix = planned
            .strip_prefix(&prefix)
            .and_then(|suffix| suffix.parse::<usize>().ok())
            .map_or(1, |suffix| suffix + 1);
        loop {
            let candidate = format!("{source_name}_{suffix}");
            if self.reserve_in_source(candidate.clone()) {
                if reserve_in_nested_scopes {
                    self.scopes[0]
                        .names_reserved_in_descendants
                        .push(candidate.clone());
                }
                return candidate;
            }
            suffix += 1;
        }
    }

    /// Reconciles a scoped optimistic name such as `_super` with bindings
    /// introduced by another target pass. Unlike source-derived numbered
    /// bindings, preferred names are owned by the current function scope and
    /// may therefore be reused by sibling functions.
    pub(super) fn allocate_planned_preferred_with_policy(
        &mut self,
        preferred: &str,
        planned: String,
        reserve_in_nested_scopes: bool,
    ) -> String {
        if self.reserve_in_current(planned.clone(), true, reserve_in_nested_scopes) {
            return planned;
        }
        let prefix = format!("{preferred}_");
        let mut suffix = planned
            .strip_prefix(&prefix)
            .and_then(|suffix| suffix.parse::<usize>().ok())
            .map_or(1, |suffix| suffix + 1);
        loop {
            let candidate = format!("{preferred}_{suffix}");
            if self.reserve_in_current(candidate.clone(), true, reserve_in_nested_scopes) {
                return candidate;
            }
            suffix += 1;
        }
    }

    fn reserve_in_current(
        &mut self,
        candidate: String,
        binding: bool,
        reserve_in_nested_scopes: bool,
    ) -> bool {
        if self.reserved_source_names.contains(&candidate)
            || self.current_scope_contains(&candidate)
            || self.ancestor_scope_contains(
                &candidate,
                self.ancestor_policy == AncestorBindingPolicy::Reserve,
            )
        {
            return false;
        }
        self.scopes[self.current.0].names.push(candidate.clone());
        if reserve_in_nested_scopes {
            self.scopes[self.current.0]
                .names_reserved_in_descendants
                .push(candidate.clone());
        }
        if binding {
            self.scopes[self.current.0].bindings.push(candidate);
        }
        true
    }

    fn reserve_in_source(&mut self, candidate: String) -> bool {
        if self.reserved_source_names.contains(&candidate)
            || self.scopes[0].names.iter().any(|name| name == &candidate)
        {
            return false;
        }
        self.scopes[0].names.push(candidate);
        true
    }

    fn current_scope_contains(&self, candidate: &str) -> bool {
        self.scopes[self.current.0]
            .names
            .iter()
            .any(|name| name == candidate)
    }

    fn ancestor_scope_contains(&self, candidate: &str, include_all_names: bool) -> bool {
        let mut scope = self.scopes[self.current.0].parent;
        while let Some(current) = scope {
            let current = &self.scopes[current.0];
            if current
                .names_reserved_in_descendants
                .iter()
                .any(|name| name == candidate)
                || include_all_names && current.names.iter().any(|name| name == candidate)
            {
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

#[cfg(test)]
#[path = "../../tests/unit/generated_bindings/tests.rs"]
mod tests;
