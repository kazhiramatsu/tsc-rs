use std::collections::{BTreeMap, BTreeSet};

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
    // Consumed by allocate_loop_variable (B-4 loop conversion).
    #[allow(dead_code)]
    loop_temp_taken: bool,
    private_names: Vec<String>,
    private_names_reserved_in_descendants: Vec<String>,
    private_temp_ordinals: BTreeMap<String, usize>,
}

#[derive(Debug, Default)]
pub(super) struct GeneratedBindings(Vec<String>);

impl GeneratedBindings {
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
    // Consumed by allocate_source_numbered_for_node (B-4/B-5 owners).
    #[allow(dead_code)]
    node_names: BTreeMap<(u64, u64), String>,
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
                loop_temp_taken: false,
                private_names: Vec::new(),
                private_names_reserved_in_descendants: Vec::new(),
                private_temp_ordinals: BTreeMap::new(),
            }],
            current: GeneratedBindingScopeId(0),
            node_names: BTreeMap::new(),
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
            loop_temp_taken: false,
            private_names: Vec::new(),
            private_names_reserved_in_descendants: Vec::new(),
            private_temp_ordinals: BTreeMap::new(),
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

    /// Reconciles an eagerly planned ordinary temp with bindings introduced
    /// by later transforms. Preserve the planned spelling when it remains
    /// available; otherwise resume the scope-local temp sequence.
    pub(super) fn allocate_planned_temp_with_policy(
        &mut self,
        planned: String,
        reserve_in_nested_scopes: bool,
    ) -> String {
        if self.reserve_in_current(planned.clone(), true, reserve_in_nested_scopes) {
            return planned;
        }
        self.allocate_temp_with_policy(reserve_in_nested_scopes)
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

    /// tsc-port: makeTempVariableName @6.0.3
    /// tsc-hash: 9b0f57d6a9a21d2fa06328b7cef07d8715e68ca9cb53d9ae5762a323f42cc4bf
    /// tsc-span: _tsc.js:120703-120740
    ///
    /// `createLoopVariable` prefers the dedicated `_i` slot: the per-scope
    /// TempFlags bit is consumed only when `_i` is actually free (an
    /// occupied `_i` leaves the bit unset, exactly like tsc), and every
    /// other request falls through to the ordinary temp sequence, whose
    /// candidate list already skips the `_i`/`_n` ordinals.
    #[allow(dead_code)] // callers arrive with B-4 loop conversion
    pub(super) fn allocate_loop_variable(&mut self, reserve_in_nested_scopes: bool) -> String {
        if !self.scopes[self.current.0].loop_temp_taken
            && self.reserve_in_current("_i".to_owned(), true, reserve_in_nested_scopes)
        {
            self.scopes[self.current.0].loop_temp_taken = true;
            return "_i".to_owned();
        }
        self.allocate_temp_with_policy(reserve_in_nested_scopes)
    }

    /// tsc-port: generateNameCached @6.0.3
    /// tsc-hash: 47bae5357b7899375328dd535b32ca3ccd449ad5f8383d46b93963fc81849abc
    /// tsc-span: _tsc.js:120633-120637
    ///
    /// Eager equivalent of the per-node generated-name cache behind
    /// `getGeneratedNameForNode`/`getLocalName`/`getInternalName`: the
    /// first request for a node key allocates the source-derived numbered
    /// form (tsc's non-optimistic `makeUniqueName` starts at `name_1`),
    /// and every later request returns the recorded spelling unchanged.
    /// The key is the caller's stable (source, node) identity projection.
    #[allow(dead_code)] // callers arrive with the B-4/B-5 owners
    pub(super) fn allocate_source_numbered_for_node(
        &mut self,
        key: (u64, u64),
        source_name: &str,
    ) -> String {
        if let Some(existing) = self.node_names.get(&key) {
            return existing.clone();
        }
        let name = self.allocate_numbered(source_name);
        self.node_names.insert(key, name.clone());
        name
    }

    /// Allocates the generated private name used for a source-named role such
    /// as an auto-accessor backing field. Private generated names have their
    /// own collision namespace in tsc; the collision ordinal belongs to the
    /// source name and is inserted before the role suffix.
    pub(super) fn allocate_private_preferred_with_role_suffix(
        &mut self,
        preferred: &str,
        role_suffix: &str,
        locally_reserved: &BTreeSet<String>,
    ) -> String {
        let preferred = preferred.trim_start_matches('#');
        let candidate = format!("{preferred}{role_suffix}");
        if !locally_reserved.contains(&candidate)
            && self.reserve_private_in_current(candidate.clone())
        {
            return candidate;
        }
        let mut ordinal = 1usize;
        loop {
            let candidate = format!("{preferred}_{ordinal}{role_suffix}");
            if !locally_reserved.contains(&candidate)
                && self.reserve_private_in_current(candidate.clone())
            {
                return candidate;
            }
            ordinal += 1;
        }
    }

    /// Allocates a formatted private temp (`_a`, `_b`, ...) for a generated
    /// role suffix. tsc tracks a separate temp sequence for each formatted
    /// prefix/suffix pair, so these names do not consume ordinary `_a` temps.
    pub(super) fn allocate_private_temp_with_role_suffix(
        &mut self,
        role_suffix: &str,
        locally_reserved: &BTreeSet<String>,
    ) -> String {
        loop {
            let ordinal = {
                let next = self.scopes[self.current.0]
                    .private_temp_ordinals
                    .entry(role_suffix.to_owned())
                    .or_default();
                let ordinal = *next;
                *next += 1;
                ordinal
            };
            let Some(temp) = Self::temp_candidate(ordinal) else {
                continue;
            };
            let candidate = format!("{temp}{role_suffix}");
            if !locally_reserved.contains(&candidate)
                && self.reserve_private_in_current(candidate.clone())
            {
                return candidate;
            }
        }
    }

    /// Allocates a source-derived generated name whose semantic role is a
    /// suffix (`_get`, `_set`, ...). The collision ordinal belongs to the
    /// source name and is therefore inserted before that role suffix.
    pub(super) fn allocate_preferred_with_role_suffix(
        &mut self,
        preferred: &str,
        role_suffix: &str,
    ) -> String {
        let candidate = format!("{preferred}{role_suffix}");
        if self.reserve_in_current(candidate.clone(), true, true) {
            return candidate;
        }
        let mut ordinal = 1usize;
        loop {
            let candidate = format!("{preferred}_{ordinal}{role_suffix}");
            if self.reserve_in_current(candidate.clone(), true, true) {
                return candidate;
            }
            ordinal += 1;
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

    /// Assigns the source-derived numbered form in TRAVERSAL order: the
    /// next free ordinal for the base at first occurrence, ignoring any
    /// visit-time planned ordinal. This is the finalize-walk analog of
    /// upstream's print-pass numbering (`makeUniqueName` ordinals are
    /// assigned in emit traversal order), which diverges from eager visit
    /// order exactly in multi-pass compositions (H2.5h CA-2a C(iii)).
    pub(super) fn allocate_source_numbered_with_policy(
        &mut self,
        source_name: &str,
        reserve_in_nested_scopes: bool,
    ) -> String {
        let mut suffix = 1usize;
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

    /// Commits a file-level optimistic name that was planned from the parsed
    /// source identifier snapshot.
    ///
    /// TypeScript's `FileLevel` predicate intentionally ignores generated
    /// peers, so this operation must not call `reserve_in_current`: two
    /// distinct generated binding identities are allowed to carry the same
    /// spelling. The spelling is still recorded in the current scope (and,
    /// when requested, its descendants), ensuring ordinary generated names
    /// allocated afterwards avoid it just as `ReservedInNestedScopes` does in
    /// the TypeScript printer.
    pub(super) fn reserve_planned_file_level_optimistic_with_policy(
        &mut self,
        planned: String,
        reserve_in_nested_scopes: bool,
    ) -> String {
        let current = &mut self.scopes[self.current.0];
        current.names.push(planned.clone());
        if reserve_in_nested_scopes {
            current.names_reserved_in_descendants.push(planned.clone());
        }
        current.bindings.push(planned.clone());
        planned
    }

    pub(super) fn allocate_planned_preferred_with_role_suffix_with_policy(
        &mut self,
        preferred: &str,
        role_suffix: &str,
        planned: String,
        reserve_in_nested_scopes: bool,
    ) -> String {
        if self.reserve_in_current(planned.clone(), true, reserve_in_nested_scopes) {
            return planned;
        }
        let unsuffixed = format!("{preferred}{role_suffix}");
        let stem = planned.strip_suffix(role_suffix).unwrap_or(&planned);
        let prefix = format!("{preferred}_");
        let mut ordinal = if planned == unsuffixed {
            1
        } else {
            stem.strip_prefix(&prefix)
                .and_then(|suffix| suffix.parse::<usize>().ok())
                .map_or(1, |suffix| suffix + 1)
        };
        loop {
            let candidate = format!("{preferred}_{ordinal}{role_suffix}");
            if self.reserve_in_current(candidate.clone(), true, reserve_in_nested_scopes) {
                return candidate;
            }
            ordinal += 1;
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

    fn reserve_private_in_current(&mut self, candidate: String) -> bool {
        if self.current_private_scope_contains(&candidate)
            || self.ancestor_private_scope_contains(&candidate)
        {
            return false;
        }
        let current = &mut self.scopes[self.current.0];
        current.private_names.push(candidate.clone());
        // Generated private names are reserved in nested name-generation
        // scopes even when ordinary generated bindings may shadow ancestors.
        current
            .private_names_reserved_in_descendants
            .push(candidate);
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

    fn current_private_scope_contains(&self, candidate: &str) -> bool {
        self.scopes[self.current.0]
            .private_names
            .iter()
            .any(|name| name == candidate)
    }

    fn ancestor_private_scope_contains(&self, candidate: &str) -> bool {
        let mut scope = self.scopes[self.current.0].parent;
        while let Some(current) = scope {
            let current = &self.scopes[current.0];
            if current
                .private_names_reserved_in_descendants
                .iter()
                .any(|name| name == candidate)
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
