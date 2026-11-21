//! Pure-host metadata observations consumed only at ordered scope lookup.
use super::*;
/// Exact-spelling observations; only ordered lookups publish their facts.
#[derive(Default)]
pub(crate) struct ScopePreparation {
    directories: std::sync::Mutex<
        rustc_hash::FxHashMap<crate::lexical_path_cache::LexicalPath, Arc<PreparedScopeLookup>>,
    >,
    packages: std::sync::Mutex<rustc_hash::FxHashMap<Arc<JsString>, Arc<PreparedScopePackage>>>,
}

#[derive(Clone)]
pub(crate) struct PreparedFileScope {
    pub(crate) lookup: Arc<PreparedScopeLookup>,
    pub(crate) normalized: bool,
}

pub(crate) struct PreparedScopeLookup {
    directory: crate::lexical_path_cache::LexicalPath,
    canonical: Option<JsString>,
    packages: Vec<Arc<PreparedScopePackage>>,
    memo_only: bool,
}

struct PreparedScopePackage {
    query: Arc<JsString>,
    canonical: Option<JsString>,
    /// None is a present boundary whose manifest is read at the ordered visit.
    missing: Option<bool>,
}
impl PreparedScopePackage {
    fn key(&self) -> &JsString {
        self.canonical.as_ref().unwrap_or(&self.query)
    }
}

impl PreparedScopeLookup {
    fn key(&self) -> &JsString {
        self.canonical.as_ref().unwrap_or(&self.directory)
    }
}

impl ScopePreparation {
    pub(crate) fn prepare(
        &self,
        resolver: &ModuleResolver<'_>,
        file: &ProgramPath,
    ) -> Option<PreparedFileScope> {
        if !resolver.package_cache_enabled || resolver.trace.is_some() {
            return None;
        }
        let (directory, normalized) = match resolver
            .lexical_paths
            .cached_file_directory(file.display(), resolver.current_directory_text())
        {
            Some(context) => context,
            None => {
                let normalized = normalize_absolute_js_path(
                    file.display(),
                    Some(resolver.current_directory_text()),
                    true,
                )
                .ok()?;
                let unchanged = normalized.as_js() == file.display();
                (
                    crate::lexical_path_cache::LexicalPath::from_owned(
                        crate::js_path::directory_name(normalized.as_js()),
                    ),
                    unchanged,
                )
            }
        };
        let normalized = normalized && resolver.module_resolution_override.is_none();
        if let Some(lookup) = self
            .directories
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&directory)
            .cloned()
        {
            return Some(PreparedFileScope { lookup, normalized });
        }
        let key = canonical_text(
            directory.as_js(),
            resolver.path_context.use_case_sensitive_file_names(),
        );
        let memo_only = resolver.package_scope_by_directory.contains_key(&key);
        let mut packages = Vec::new();
        if !memo_only {
            for ancestor in ancestor_directories(directory.as_js()) {
                let query = join_normalized(ancestor.as_js(), "package.json");
                let cached = self
                    .packages
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(&query)
                    .cloned();
                let package = match cached {
                    Some(package) => package,
                    None => {
                        let observation = resolver.prepare_scope_package(query)?;
                        Arc::clone(
                            self.packages
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .entry(Arc::clone(&observation.query))
                                .or_insert_with(|| Arc::new(observation)),
                        )
                    }
                };
                let found = package.missing.is_none();
                packages.push(package);
                if found {
                    break;
                }
            }
        }
        let canonical = (key.as_js() != directory.as_js()).then_some(key);
        let lookup = Arc::new(PreparedScopeLookup {
            directory,
            canonical,
            packages,
            memo_only,
        });
        let lookup = Arc::clone(
            self.directories
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .entry(lookup.directory.clone())
                .or_insert(lookup),
        );
        Some(PreparedFileScope { lookup, normalized })
    }
}

impl ModuleResolver<'_> {
    fn prepare_scope_package(&self, query: JsString) -> Option<PreparedScopePackage> {
        let key = canonical_text(
            query.as_js(),
            self.path_context.use_case_sensitive_file_names(),
        );
        let known = matches!(self.package_cache.get(&key), Some(PackageCacheEntry::Found(package)) if package.metadata.package_json().display() == query.as_js());
        let missing = if known {
            None
        } else {
            let directory = crate::js_path::directory_name(query.as_js());
            let exists = self.host.directory_exists_js(directory.as_js()).ok()?;
            if exists && self.host.file_exists_js(query.as_js()).ok()? {
                None
            } else {
                Some(exists)
            }
        };
        let canonical = (key != query).then_some(key);
        Some(PreparedScopePackage {
            query: Arc::new(query),
            canonical,
            missing,
        })
    }

    pub(crate) fn package_scope_from_prepared(
        &mut self,
        lookup: &PreparedScopeLookup,
    ) -> Result<Option<Arc<PackageMetadata>>, ResolutionError> {
        if !self.package_cache_enabled || self.trace.is_some() {
            return self
                .find_nearest_package_scope(lookup.directory.as_js())
                .map(|scope| scope.map(|package| Arc::clone(&package.metadata)));
        }
        if let Some(scope) = self.memoized_package_scope(lookup.directory.as_js(), lookup.key()) {
            return Ok(scope.map(|package| Arc::clone(&package.metadata)));
        }
        if lookup.memo_only {
            return self
                .find_nearest_package_scope(lookup.directory.as_js())
                .map(|scope| scope.map(|package| Arc::clone(&package.metadata)));
        }
        let mut found = None;
        let mut complete = true;
        for observation in &lookup.packages {
            if let Some(entry) = self.package_cache.get(observation.key()) {
                if let PackageCacheEntry::Found(package) = entry {
                    found = Some(Arc::clone(package));
                    break;
                }
                if observation.missing.is_none() {
                    complete = false;
                }
                continue;
            }
            match observation.missing {
                Some(directory_exists) => {
                    self.record_package_json_probe(
                        observation.key(),
                        observation.query.as_js(),
                        false,
                    )?;
                    self.package_cache.insert(
                        observation.key().clone(),
                        PackageCacheEntry::Missing { directory_exists },
                    );
                }
                None => {
                    found = self.load_package(observation.query.as_js())?;
                    complete = found.is_some();
                    break;
                }
            }
        }
        if !complete && found.is_none() {
            return self
                .find_nearest_package_scope(lookup.directory.as_js())
                .map(|scope| scope.map(|package| Arc::clone(&package.metadata)));
        }
        self.package_scope_by_directory
            .insert(lookup.key().clone(), found.clone());
        Ok(found.map(|package| Arc::clone(&package.metadata)))
    }
}
