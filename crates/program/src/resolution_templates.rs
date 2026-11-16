//! One consumed row template per shared fact with compatible final membership.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FinalBindingContext {
    target: Option<SourceFileId>,
    unloaded_reason: Option<UnloadedModuleReason>,
    loads_source: bool,
    no_resolve: bool,
}
impl FinalBindingContext {
    fn new(fact: &PreparedModule, binding: &ModuleBinding, sources: &[PreparedSourceFile]) -> Self {
        if fact.path.is_none() {
            return Self {
                target: None,
                unloaded_reason: None,
                loads_source: false,
                no_resolve: false,
            };
        }
        if let Some(target) = binding.target {
            debug_assert!(
                !binding.exact_path || fact.path.as_ref() == Some(sources[target.index()].path())
            );
            // A loaded target makes the exclusion fields irrelevant. Checking
            // path equality also reproduces the proven-exact shortcut.
            return Self {
                target: Some(target),
                unloaded_reason: None,
                loads_source: false,
                no_resolve: false,
            };
        }
        let reason = binding.unloaded_reason.or_else(|| {
            binding
                .no_resolve
                .then_some(UnloadedModuleReason::NoResolve)
        });
        Self {
            target: None,
            unloaded_reason: reason,
            loads_source: reason.is_none() && binding.loads_source,
            no_resolve: false,
        }
    }
    fn binding(self, source: SourceFileId) -> ModuleBinding {
        ModuleBinding {
            source,
            target: self.target,
            loads_source: self.loads_source,
            unloaded_reason: self.unloaded_reason,
            no_resolve: self.no_resolve,
            exact_path: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TemplateHandle {
    index: u32,
}
pub(crate) enum TemplateDisposition {
    Direct(Arc<PreparedModule>),
    Shared(TemplateHandle),
}
struct Input {
    fact: Arc<PreparedModule>,
    binding: ModuleBinding,
    remaining: u32,
}
pub(crate) struct TemplatePreparation {
    by_fact: rustc_hash::FxHashMap<usize, u32>,
    inputs: Vec<Input>,
    limit: usize,
}
impl TemplatePreparation {
    pub(crate) fn new(row_count: usize) -> Self {
        Self {
            by_fact: Default::default(),
            inputs: Vec::new(),
            limit: row_count / 2,
        }
    }

    pub(crate) fn consider(
        &mut self,
        fact: Arc<PreparedModule>,
        binding: ModuleBinding,
        sources: &[PreparedSourceFile],
    ) -> TemplateDisposition {
        let identity = Arc::as_ptr(&fact) as usize;
        let context = FinalBindingContext::new(&fact, &binding, sources);
        let index = if let Some(&index) = self.by_fact.get(&identity) {
            let input = &mut self.inputs[index as usize];
            if FinalBindingContext::new(&input.fact, &input.binding, sources) != context {
                return TemplateDisposition::Direct(fact);
            }
            input.remaining = input.remaining.checked_add(1).expect("module uses fit u32");
            index
        } else {
            if Arc::strong_count(&fact) == 1 || self.inputs.len() == self.limit {
                return TemplateDisposition::Direct(fact);
            }
            let index = u32::try_from(self.inputs.len()).expect("module templates fit u32");
            self.inputs.push(Input {
                fact,
                binding: context.binding(binding.source),
                remaining: 1,
            });
            self.by_fact.insert(identity, index);
            return TemplateDisposition::Shared(TemplateHandle { index });
        };
        TemplateDisposition::Shared(TemplateHandle { index })
    }
    pub(crate) fn finish(
        self,
        loaded: &mut LoadedModules,
        sources: &[PreparedSourceFile],
        maps: &PreparedPackageMaps,
    ) -> ModuleTemplates {
        let Self {
            by_fact, inputs, ..
        } = self;
        drop(by_fact);
        let mut slots = Vec::with_capacity(inputs.len());
        let mut failed = false;
        for input in inputs {
            let context = FinalBindingContext::new(&input.fact, &input.binding, sources);
            let fact = if !failed && Arc::weak_count(&input.fact) == 0 {
                match Arc::try_unwrap(input.fact) {
                    Ok(fact) => {
                        let result = fact.bind(
                            &input.binding,
                            sources,
                            maps,
                            &mut loaded.packages,
                            &mut loaded.package_indices,
                            &mut loaded.unloaded_paths,
                        );
                        let (row, error) = match result {
                            Ok(row) => (Some(row), None),
                            Err(error) => {
                                failed = true;
                                (None, Some(Box::new(error)))
                            }
                        };
                        slots.push(Slot {
                            fact: FinalFact::Ready(Box::new(TemplateRow { row, error })),
                            remaining: input.remaining,
                            context,
                        });
                        continue;
                    }
                    Err(fact) => fact,
                }
            } else {
                input.fact
            };
            slots.push(Slot {
                fact: FinalFact::Retained(Some(fact)),
                remaining: input.remaining,
                context,
            });
        }
        ModuleTemplates { slots }
    }
}
struct TemplateRow {
    row: Option<Row>,
    error: Option<Box<ResolutionError>>,
}
enum FinalFact {
    Ready(Box<TemplateRow>),
    Retained(Option<Arc<PreparedModule>>),
}
struct Slot {
    fact: FinalFact,
    remaining: u32,
    context: FinalBindingContext,
}
pub(crate) struct ModuleTemplates {
    slots: Vec<Slot>,
}
impl ModuleTemplates {
    pub(crate) fn push(
        &mut self,
        handle: TemplateHandle,
        loaded: &mut LoadedModules,
        sources: &[PreparedSourceFile],
        source: SourceFileId,
        specifier: &mut JsString,
        maps: &PreparedPackageMaps,
    ) -> Result<(), ResolutionError> {
        let slot = &mut self.slots[handle.index as usize];
        slot.remaining = slot
            .remaining
            .checked_sub(1)
            .expect("a template has an incoming row");
        match &mut slot.fact {
            FinalFact::Ready(template) => {
                if let Some(error) = template.error.take() {
                    return Err(*error);
                }
                let row = if slot.remaining == 0 {
                    template.row.take().expect("final template use")
                } else {
                    template.row.as_ref().expect("valid template").clone()
                };
                loaded.push_final_row(row, specifier, source);
                Ok(())
            }
            FinalFact::Retained(fact) => {
                let fact = if slot.remaining == 0 {
                    fact.take().expect("final retained fact use")
                } else {
                    Arc::clone(fact.as_ref().expect("retained fact"))
                };
                loaded.push_prepared(fact, specifier, slot.context.binding(source), sources, maps)
            }
        }
    }
}
impl LoadedModules {
    pub(crate) fn preassign_package(&mut self, package: &PreparedPackageRef) {
        if package.index >= self.package_indices.len() {
            self.package_indices.resize(package.index + 1, None);
        }
        if self.package_indices[package.index].is_none() {
            self.package_indices[package.index] = Some(self.packages.intern_prepared(package));
        }
    }
}
const _: () = assert!(std::mem::size_of::<Input>() <= 32);
const _: () = assert!(std::mem::size_of::<Slot>() <= 32);
const _: () = assert!(std::mem::size_of::<TemplateRow>() <= 72);
const _: () = assert!(std::mem::size_of::<FinalBindingContext>() <= 12);

#[cfg(test)]
mod tests {
    use super::*;

    fn path(name: &str) -> ProgramPath {
        crate::module_resolution::make_program_path(name, true).unwrap()
    }

    fn fact(extension: ExtensionTag) -> Arc<PreparedModule> {
        Arc::new(PreparedModule {
            row: Row {
                specifier: JsString::default(),
                target: Target::NotFound,
                rare: None,
                package_id: None,
                mode: ResolutionMode::Unspecified,
                extension,
                flags: 0,
            },
            path: Some(path("/p/target.ts")),
            package: None,
        })
    }

    fn binding(source: u32, target: Option<u32>) -> ModuleBinding {
        ModuleBinding {
            source: SourceFileId::from_raw(source),
            target: target.map(SourceFileId::from_raw),
            loads_source: true,
            unloaded_reason: None,
            no_resolve: false,
            exact_path: false,
        }
    }

    fn shared(disposition: TemplateDisposition) -> TemplateHandle {
        let TemplateDisposition::Shared(handle) = disposition else {
            panic!("a shared fact gets a template handle")
        };
        handle
    }

    #[test]
    fn a_template_reuses_membership_and_moves_each_request_specifier() {
        let sources = vec![
            PreparedSourceFile::new(path("/p/target.ts"), ""),
            PreparedSourceFile::new(path("/p/first.ts"), ""),
            PreparedSourceFile::new(path("/p/second.ts"), ""),
        ];
        let mut preparation = TemplatePreparation::new(4);
        let fact = fact(ExtensionTag::Ts);
        let first = shared(preparation.consider(Arc::clone(&fact), binding(1, Some(0)), &sources));
        let second = shared(preparation.consider(fact, binding(2, Some(0)), &sources));
        let maps = PreparedPackageMaps::new(std::iter::empty());
        let mut loaded = LoadedModules::new(
            3,
            2,
            [
                (SourceFileId::from_raw(1), 1),
                (SourceFileId::from_raw(2), 1),
            ],
        );
        let mut templates = preparation.finish(&mut loaded, &sources, &maps);
        assert_eq!(templates.slots.len(), 1);
        assert!(matches!(templates.slots[0].fact, FinalFact::Ready(_)));
        for (handle, source, text) in [(first, 1, "first"), (second, 2, "second")] {
            let mut specifier = JsString::from(text);
            templates
                .push(
                    handle,
                    &mut loaded,
                    &sources,
                    SourceFileId::from_raw(source),
                    &mut specifier,
                    &maps,
                )
                .unwrap();
            assert!(specifier.is_empty());
        }
        assert_eq!(loaded.source_ranges[1], 0..1);
        assert_eq!(loaded.source_ranges[2], 1..2);
        assert_eq!(loaded.modules[0].specifier, "first");
        assert_eq!(loaded.modules[1].specifier, "second");
        assert_eq!(
            loaded.modules[0].target,
            Target::Source(SourceFileId::from_raw(0))
        );
        assert_eq!(templates.slots[0].remaining, 0);
        let table = loaded.finish(&sources, BTreeMap::new());
        assert_eq!(
            table
                .ordered_rows()
                .map(|(source, row)| (
                    source.raw(),
                    row.specifier.as_js().to_string_lossy().into_owned()
                ))
                .collect::<Vec<_>>(),
            vec![(1, "first".to_owned()), (2, "second".to_owned())]
        );
    }

    #[test]
    fn incompatible_membership_keeps_the_original_fact_for_direct_binding() {
        let sources = vec![PreparedSourceFile::new(path("/p/target.ts"), "")];
        let mut preparation = TemplatePreparation::new(4);
        let fact = fact(ExtensionTag::Ts);
        shared(preparation.consider(Arc::clone(&fact), binding(0, Some(0)), &sources));
        let mut excluded = binding(0, None);
        excluded.no_resolve = true;
        let TemplateDisposition::Direct(retained) =
            preparation.consider(Arc::clone(&fact), excluded, &sources)
        else {
            panic!("a different final membership cannot share a template")
        };
        assert!(Arc::ptr_eq(&retained, &fact));
        assert_eq!(preparation.inputs[0].remaining, 1);
        let maps = PreparedPackageMaps::new(std::iter::empty());
        let mut loaded = LoadedModules::new(1, 1, [(SourceFileId::from_raw(0), 1)]);
        let templates = preparation.finish(&mut loaded, &sources, &maps);
        assert!(matches!(templates.slots[0].fact, FinalFact::Retained(_)));
        assert!(loaded.modules.is_empty());
    }

    #[test]
    fn preparation_defers_its_error_and_leaves_later_facts_unbound() {
        let mut preparation = TemplatePreparation::new(4);
        let invalid = fact(ExtensionTag::Js);
        let first = shared(preparation.consider(Arc::clone(&invalid), binding(0, None), &[]));
        drop(invalid);
        let later = fact(ExtensionTag::Ts);
        shared(preparation.consider(Arc::clone(&later), binding(0, None), &[]));
        drop(later);
        let maps = PreparedPackageMaps::new(std::iter::empty());
        let mut loaded = LoadedModules::new(1, 0, []);
        let mut templates = preparation.finish(&mut loaded, &[], &maps);
        assert!(matches!(templates.slots[0].fact, FinalFact::Ready(_)));
        assert!(matches!(templates.slots[1].fact, FinalFact::Retained(_)));
        let mut specifier = JsString::from("bad");
        let error = templates
            .push(
                first,
                &mut loaded,
                &[],
                SourceFileId::from_raw(0),
                &mut specifier,
                &maps,
            )
            .unwrap_err();
        assert!(format!("{error:?}").contains("unexplained-unloaded-javascript"));
        assert_eq!(specifier, "bad");
        assert!(loaded.modules.is_empty());
    }
}
