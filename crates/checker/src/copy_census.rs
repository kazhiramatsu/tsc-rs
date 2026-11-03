//! Observation wrappers for B-2. The default expansion is exactly the
//! original expression: even argument/length evaluation is feature gated.

macro_rules! copy_census {
    ($site:literal, $group:ident, $value:expr) => {{
        #[cfg(feature = "perf-counters")]
        {
            let value = $value;
            static SITE: tsc_types::perf::list_copies::Site =
                tsc_types::perf::list_copies::Site::new(
                    tsc_types::perf::list_copies::Group::$group,
                    tsc_types::census_names!($site),
                );
            $crate::copy_census::Copied::note(&value, &SITE);
            value
        }
        #[cfg(not(feature = "perf-counters"))]
        {
            $value
        }
    }};
}
pub(crate) use copy_census;

#[cfg(feature = "perf-counters")]
mod enabled {
    use std::mem::size_of;
    use tsc_types::perf::list_copies::Site;

    pub(crate) trait Copied {
        fn note(&self, site: &'static Site);
    }

    impl<T> Copied for Vec<T> {
        fn note(&self, site: &'static Site) {
            site.slice(self);
        }
    }
    impl<T> Copied for Box<[T]> {
        fn note(&self, site: &'static Site) {
            site.slice(self);
        }
    }
    impl<T: Copied> Copied for Option<T> {
        fn note(&self, site: &'static Site) {
            // None performs no list copy; Some(empty) is a length-zero copy.
            if let Some(value) = self {
                value.note(site);
            }
        }
    }
    impl Copied for tsc_types::TypeListCopy {
        fn note(&self, site: &'static Site) {
            site.slice::<tsc_types::TypeId>(self);
        }
    }
    impl Copied for tsc_binder::Declarations {
        fn note(&self, site: &'static Site) {
            site.slice::<tsc_syntax::NodeId>(self);
        }
    }
    impl Copied for tsc_binder::Symbol {
        fn note(&self, site: &'static Site) {
            self.declarations.note(site);
        }
    }
    impl Copied for tsc_binder::SymbolTable {
        fn note(&self, site: &'static Site) {
            site.note(
                self.len(),
                self.len(),
                self.len() * size_of::<(tsc_types::EscapedName, tsc_binder::SymbolId)>(),
            );
        }
    }
    impl<T: Copy> Copied for crate::state::SharedList<T> {
        fn note(&self, site: &'static Site) {
            site.note(self.len(), 0, 0);
        }
    }
    impl Copied for crate::member_table::MemberTable {
        fn note(&self, site: &'static Site) {
            // Logical member IDs only; hash-index allocation/control bytes
            // belong to the table representation, outside this list census.
            site.note(
                self.len(),
                self.len(),
                self.len() * size_of::<tsc_binder::SymbolId>(),
            );
        }
    }
    impl Copied for tsc_types::TupleTargetData {
        fn note(&self, site: &'static Site) {
            // An aggregate expression is one call. Its length is the sum
            // of its copied list fields, rather than counting the same
            // source expression once per field.
            let labels = self.labeled_element_declarations.as_deref().unwrap_or(&[]);
            let len = self.element_flags.len() + labels.len();
            let bytes = std::mem::size_of_val(&*self.element_flags) + std::mem::size_of_val(labels);
            site.note(len, len, bytes);
        }
    }
    impl Copied for tsc_types::ConditionalRootData {
        fn note(&self, site: &'static Site) {
            let len = self.infer_type_parameters.len()
                + self.outer_type_parameters.as_deref().map_or(0, <[_]>::len)
                + self.alias_type_arguments.as_deref().map_or(0, <[_]>::len);
            site.note(len, len, len * size_of::<tsc_types::TypeId>());
        }
    }
    impl Copied for tsc_types::TemplateLiteralData {
        fn note(&self, site: &'static Site) {
            self.texts.note(site);
        }
    }
    impl Copied for crate::state::Signature {
        fn note(&self, site: &'static Site) {
            let types = self.type_parameters.as_deref().unwrap_or(&[]);
            let composites = self.composite_signatures.as_deref().unwrap_or(&[]);
            let len = types.len() + self.parameters.len() + composites.len();
            let bytes = std::mem::size_of_val(types)
                + std::mem::size_of_val(self.parameters.as_slice())
                + std::mem::size_of_val(composites);
            site.note(len, len, bytes);
        }
    }
}
#[cfg(feature = "perf-counters")]
pub(crate) use enabled::Copied;

#[cfg(test)]
mod tests {
    #[test]
    fn observes_without_repeating_or_changing_the_copy() {
        let mut evaluations = 0;
        let input = [1_u32, 2, 3];
        let result = copy_census!("types.test.once", Types, {
            evaluations += 1;
            input.to_vec()
        });
        assert_eq!(evaluations, 1);
        assert_eq!(result, input);
    }

    #[test]
    #[cfg(feature = "perf-counters")]
    fn aggregate_clone_is_one_call_and_counts_all_list_fields() {
        use tsc_types::{ConditionalRootData, TypeId};
        let ty = TypeId::new(0);
        let source = ConditionalRootData {
            node: 0,
            check_type: ty,
            extends_type: ty,
            is_distributive: false,
            infer_type_parameters: vec![ty; 2].into_boxed_slice(),
            outer_type_parameters: Some(vec![ty; 3].into_boxed_slice()),
            alias_symbol: None,
            alias_type_arguments: Some(vec![ty; 4].into_boxed_slice()),
        };
        let copy = copy_census!("sidecars.test.aggregate", Sidecars, source.clone());
        assert_eq!(copy, source);
        let rows = tsc_types::perf::snapshot();
        assert!(rows.contains(&("b2.site.sidecars.test.aggregate.calls", Some(1))));
        assert!(rows.contains(&("b2.site.sidecars.test.aggregate.elements", Some(9))));
        assert!(rows.contains(&("b2.site.sidecars.test.aggregate.bytes", Some(36))));
        assert!(rows.contains(&("b2.site.sidecars.test.aggregate.len_9_32", Some(1))));
    }
}
