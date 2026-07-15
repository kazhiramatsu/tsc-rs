//! M4 5.5d: the spelling-suggestion core (getSpellingSuggestion L951 +
//! levenshteinWithMax L976) and its symbol-facing wrappers. This is
//! the risk-#1 FP boundary: a plain 2339/2304 where tsc says
//! 2551/2552 — or vice versa — is a set-compare FP, so the arithmetic
//! (substitution 2, case-substitution 0.1, insert/delete 1, cutoff
//! `bestDistance - 0.1`) transcribes verbatim over UTF-16 code units
//! (tsc indexes JS strings; `s1[i-1].toLowerCase()` lowercases one
//! code unit — surrogate halves pass through unchanged).

use tsrs2_binder::unescape_leading_underscores;
use tsrs2_syntax::NodeId;
use tsrs2_types::{SymbolFlags, SymbolId, TypeId};

use crate::state::{CheckResult2, CheckerState};

/// One UTF-16 code unit, lowercased the way `String.prototype
/// .toLowerCase` treats a single-unit string: BMP scalars take the
/// full Unicode mapping (possibly multi-char), lone surrogates are
/// identity.
fn lowercase_unit(unit: u16) -> Vec<u16> {
    match char::from_u32(u32::from(unit)) {
        Some(scalar) => scalar
            .to_lowercase()
            .flat_map(|lowered| {
                let mut buffer = [0u16; 2];
                lowered.encode_utf16(&mut buffer).to_vec()
            })
            .collect(),
        None => vec![unit],
    }
}

/// tsc-port: levenshteinWithMax @6.0.3
/// tsc-hash: eb79af510122ea8b6f14324bdd579dcdfeba2df1e60995fb199e415688f583df
/// tsc-span: _tsc.js:976-1016
fn levenshtein_with_max(s1: &[u16], s2: &[u16], max: f64) -> Option<f64> {
    let mut previous: Vec<f64> = (0..=s2.len()).map(|index| index as f64).collect();
    let mut current: Vec<f64> = vec![0.0; s2.len() + 1];
    let big = max + 0.01;
    for i in 1..=s1.len() {
        let c1 = s1[i - 1];
        let min_j = (if i as f64 > max { i as f64 - max } else { 1.0 }).ceil() as usize;
        let max_j = (if s2.len() as f64 > max + i as f64 {
            max + i as f64
        } else {
            s2.len() as f64
        })
        .floor() as usize;
        current[0] = i as f64;
        let mut col_min = i as f64;
        for slot in current.iter_mut().take(min_j).skip(1) {
            *slot = big;
        }
        for j in min_j..=max_j {
            let substitution_distance = if lowercase_unit(s1[i - 1]) == lowercase_unit(s2[j - 1]) {
                previous[j - 1] + 0.1
            } else {
                previous[j - 1] + 2.0
            };
            let dist = if c1 == s2[j - 1] {
                previous[j - 1]
            } else {
                (previous[j] + 1.0)
                    .min(current[j - 1] + 1.0)
                    .min(substitution_distance)
            };
            current[j] = dist;
            col_min = col_min.min(dist);
        }
        for slot in current.iter_mut().take(s2.len() + 1).skip(max_j + 1) {
            *slot = big;
        }
        if col_min > max {
            return None;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    let res = previous[s2.len()];
    (res <= max).then_some(res)
}

/// tsc-port: getSpellingSuggestion @6.0.3
/// tsc-hash: 37b9cd417fd83af45f9fa8584ae1a3aa05e3f7ac3764438bb0627a7d61591ab6
/// tsc-span: _tsc.js:951-975
///
/// Generic over candidate handles, with tsc's getName callback shape;
/// candidates run IN ORDER and only a STRICTLY better distance
/// replaces the best (earlier candidates win ties).
pub(crate) fn get_spelling_suggestion<C: Copy, S>(
    state: &mut S,
    name: &str,
    candidates: &[C],
    mut get_name: impl FnMut(&mut S, C) -> Option<String>,
) -> Option<C> {
    let name_units: Vec<u16> = name.encode_utf16().collect();
    let maximum_length_difference = 2.0_f64.max((name_units.len() as f64 * 0.34).floor());
    let mut best_distance = (name_units.len() as f64 * 0.4).floor() + 1.0;
    let mut best_candidate = None;
    for &candidate in candidates {
        let Some(candidate_name) = get_name(state, candidate) else {
            continue;
        };
        let candidate_units: Vec<u16> = candidate_name.encode_utf16().collect();
        if (candidate_units.len() as f64 - name_units.len() as f64).abs()
            <= maximum_length_difference
        {
            if candidate_name == name {
                continue;
            }
            if candidate_units.len() < 3 && candidate_name.to_lowercase() != name.to_lowercase() {
                continue;
            }
            let Some(distance) =
                levenshtein_with_max(&name_units, &candidate_units, best_distance - 0.1)
            else {
                continue;
            };
            debug_assert!(distance < best_distance);
            best_distance = distance;
            best_candidate = Some(candidate);
        }
    }
    best_candidate
}

impl<'a> CheckerState<'a> {
    /// tsc-port: getSpellingSuggestionForName @6.0.3
    /// tsc-hash: 5d18415ea6940193a787d4c21a9f08139c1dd6e1990bb3b8afa131811a617bc0
    /// tsc-span: _tsc.js:75579-75597
    ///
    /// The `"`-prefixed (quoted) names are rejected; the Alias-chase
    /// arm (tryResolveAlias + meaning re-test) is LIVE (M4 5.8d). An
    /// Unsupported unwind inside the chase demotes to no-suggestion
    /// (tsc cannot fail here; the suggestion band is the only consumer
    /// and a missing suggestion picks the plain message flavor).
    pub(crate) fn get_spelling_suggestion_for_name(
        &mut self,
        name: &str,
        symbols: &[SymbolId],
        meaning: SymbolFlags,
    ) -> Option<SymbolId> {
        get_spelling_suggestion(self, name, symbols, |state, candidate| {
            let candidate_name =
                unescape_leading_underscores(&state.binder.symbol(candidate).escaped_name)
                    .to_owned();
            if candidate_name.starts_with('"') {
                return None;
            }
            let flags = state.binder.symbol(candidate).flags;
            if flags.intersects(meaning) {
                return Some(candidate_name);
            }
            if flags.intersects(SymbolFlags::ALIAS) {
                if let Ok(Some(target)) = state.try_resolve_alias(candidate) {
                    if state.binder.symbol(target).flags.intersects(meaning) {
                        return Some(candidate_name);
                    }
                }
            }
            None
        })
    }

    /// tsc-port: getSuggestedSymbolForNonexistentModule @6.0.3
    /// tsc-hash: 34b420727eaeb8830d4d7e53508765d37bd46e83f85a56de9bc541b5e219ee65
    /// tsc-span: _tsc.js:75551-75553
    pub(crate) fn get_suggested_symbol_for_nonexistent_module(
        &mut self,
        name: NodeId,
        target_module: SymbolId,
    ) -> CheckResult2<Option<SymbolId>> {
        let Some(name_text) = self.identifier_text_of(name).map(str::to_owned) else {
            return Ok(None);
        };
        let exports = self.get_exports_of_module(target_module)?;
        let candidates: Vec<SymbolId> = exports.values().copied().collect();
        Ok(self.get_spelling_suggestion_for_name(
            &name_text,
            &candidates,
            SymbolFlags::MODULE_MEMBER,
        ))
    }

    /// tsc-port: getSuggestedSymbolForNonexistentClassMember @6.0.3
    /// tsc-hash: 8e78ef31290dfaec3cc2ea2e32f9c1036bfcb5a576110ce076f2c56720b0825b
    /// tsc-span: _tsc.js:75498-75500
    pub(crate) fn get_suggested_symbol_for_nonexistent_class_member(
        &mut self,
        name: &str,
        base_type: TypeId,
    ) -> CheckResult2<Option<SymbolId>> {
        let properties = self.get_properties_of_type(base_type)?;
        Ok(self.get_spelling_suggestion_for_name(name, &properties, SymbolFlags::CLASS_MEMBER))
    }

    /// tsc-port: getSuggestedSymbolForNonexistentProperty @6.0.3
    /// tsc-hash: 340ae10ba18f958d1611de0ef44f287f188beac9ac63f810afef4b80d64e29b2
    /// tsc-span: _tsc.js:75501-75517
    ///
    /// The property-side suggester — NOT suggestion-budget gated
    /// (oracle-pinned: 2551 fires freely in noLib while the name side
    /// is exhausted). The node-flavored caller filters candidates by
    /// completion validity (accessibility probe without reporting).
    pub(crate) fn get_suggested_symbol_for_nonexistent_property(
        &mut self,
        name_node: Option<tsrs2_syntax::NodeId>,
        name: &str,
        containing_type: TypeId,
    ) -> CheckResult2<Option<SymbolId>> {
        let mut props = self.get_properties_of_type(containing_type)?;
        if let Some(node) = name_node {
            if let Some(parent) = self.parent_of(node) {
                if self.kind_of(parent) == tsrs2_syntax::SyntaxKind::PropertyAccessExpression {
                    let mut filtered = Vec::with_capacity(props.len());
                    for prop in props {
                        if self.is_valid_property_access_for_completions(
                            parent,
                            containing_type,
                            prop,
                        )? {
                            filtered.push(prop);
                        }
                    }
                    props = filtered;
                }
            }
        }
        Ok(self.get_spelling_suggestion_for_name(name, &props, SymbolFlags::VALUE))
    }

    /// tsc-port: getSuggestionForNonexistentProperty @6.0.3
    /// tsc-hash: c9959d21e71cc27baff354dd7caf6abad4d7150f9c4afa48983f5e3f05fd014b
    /// tsc-span: _tsc.js:75518-75521
    pub(crate) fn get_suggestion_for_nonexistent_property(
        &mut self,
        name_node: Option<tsrs2_syntax::NodeId>,
        name: &str,
        containing_type: TypeId,
    ) -> CheckResult2<Option<String>> {
        let suggestion =
            self.get_suggested_symbol_for_nonexistent_property(name_node, name, containing_type)?;
        Ok(suggestion.map(|symbol| {
            unescape_leading_underscores(&self.binder.symbol(symbol).escaped_name).to_owned()
        }))
    }
}

impl<'a> CheckerState<'a> {
    /// tsc-port: getSuggestionForNonexistentIndexSignature @6.0.3
    /// tsc-hash: 5f46ebd76c10591a3095949e4813f3fd454475f5313bc0b22c35b27f8d7dfa81
    /// tsc-span: _tsc.js:75554-75578
    ///
    /// The 7052 "did you mean to call `o.get`" probe: a get/set method
    /// (by write-position) with ≥1 required parameter whose first
    /// parameter accepts the key type.
    pub(crate) fn get_suggestion_for_nonexistent_index_signature(
        &mut self,
        object_type: TypeId,
        expr: tsrs2_syntax::NodeId,
        keyed_type: TypeId,
    ) -> CheckResult2<Option<String>> {
        let source = self.binder.source_of_node(expr);
        let suggested_method = if tsrs2_binder::node_util::is_assignment_target(source, expr) {
            "set"
        } else {
            "get"
        };
        let has_prop = {
            let prop = self.get_property_of_object_type(object_type, suggested_method)?;
            match prop {
                Some(prop) => {
                    let prop_type = self.get_type_of_symbol(prop)?;
                    match self.get_single_call_signature(prop_type)? {
                        Some(signature) => {
                            self.get_min_argument_count(signature)? >= 1 && {
                                let first = self.get_type_at_position(signature, 0)?;
                                self.is_type_assignable_to(keyed_type, first)?
                            }
                        }
                        None => false,
                    }
                }
                None => false,
            }
        };
        if !has_prop {
            return Ok(None);
        }
        let receiver = match self.data_of(expr) {
            tsrs2_syntax::NodeData::ElementAccessExpression(data) => data.expression,
            _ => None,
        };
        // tryGetPropertyAccessOrIdentifierToString: dotted entity text.
        let base = receiver.and_then(|receiver| self.entity_name_to_string(receiver).ok());
        Ok(Some(match base {
            Some(base) => format!("{base}.{suggested_method}"),
            None => suggested_method.to_owned(),
        }))
    }
}
