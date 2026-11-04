//! Resolver-owned pure spelling only. No host result or failure is retained.
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::hash::BuildHasher;
use std::ops::Deref;
use std::sync::Arc;

use rustc_hash::FxBuildHasher;
use tsc_diagnostics::{JsStr, JsString};

use crate::module_resolution::normalize_absolute_js_path;
use crate::ResolutionError;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct LexicalPath(Arc<JsString>);

impl LexicalPath {
    pub(crate) fn into_owned(self) -> JsString {
        match Arc::try_unwrap(self.0) {
            Ok(text) => text,
            Err(text) => crate::loader_counters::js_owned("cache.output", text.as_js()),
        }
    }

    pub(crate) fn with_trailing_separator(mut self, trailing: bool) -> Self {
        if trailing && !self.ends_with("/") {
            #[cfg(feature = "loader-counters")]
            if Arc::strong_count(&self.0) != 1 {
                crate::loader_counters::js_clone("cache.detached_value", &self.0);
                crate::loader_counters::owned(
                    "cache.detached",
                    "Arc<JsString>",
                    std::mem::size_of::<JsString>() + 2 * std::mem::size_of::<usize>(),
                    true,
                );
            }
            Arc::make_mut(&mut self.0).push('/');
        }
        self
    }
}

impl Deref for LexicalPath {
    type Target = JsString;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'p> From<&'p LexicalPath> for JsStr<'p> {
    fn from(path: &'p LexicalPath) -> Self {
        path.as_js()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Operation {
    Absolute,
    Directory,
}

struct DirectoryLexicalContext<S> {
    operations: BTreeMap<(Operation, bool), HashMap<JsString, LexicalPath, S>>,
}

impl<S> Default for DirectoryLexicalContext<S> {
    fn default() -> Self {
        Self {
            operations: BTreeMap::new(),
        }
    }
}

struct Contexts<S> {
    no_base: DirectoryLexicalContext<S>,
    bases: HashMap<JsString, DirectoryLexicalContext<S>, S>,
    #[cfg(any(test, feature = "loader-counters"))]
    entries: usize,
    #[cfg(feature = "loader-counters")]
    text_bytes: usize,
}

impl<S: Default> Default for Contexts<S> {
    fn default() -> Self {
        Self {
            no_base: DirectoryLexicalContext::default(),
            bases: HashMap::with_hasher(S::default()),
            #[cfg(any(test, feature = "loader-counters"))]
            entries: 0,
            #[cfg(feature = "loader-counters")]
            text_bytes: 0,
        }
    }
}

#[derive(Default)]
pub(crate) struct PurePathCache<S = FxBuildHasher> {
    contexts: RefCell<Contexts<S>>,
}

/// Cheap owned handles; no map borrow can escape into mutable resolution or
/// a host callback. Display spellings remain separate from canonical keys.
pub(crate) struct FileResolutionContext {
    pub file: LexicalPath,
    pub directory: LexicalPath,
}

impl<S: BuildHasher + Default> PurePathCache<S> {
    fn get_or_insert(
        &self,
        op: Operation,
        text: JsStr<'_>,
        base: Option<JsStr<'_>>,
        reject_nul: bool,
        calculate: impl FnOnce() -> Result<JsString, ResolutionError>,
    ) -> Result<LexicalPath, ResolutionError> {
        let hit = {
            let contexts = self.contexts.borrow();
            let context = match base {
                Some(base) => contexts.bases.get(base.as_bytes()),
                None => Some(&contexts.no_base),
            };
            context
                .and_then(|context| context.operations.get(&(op, reject_nul)))
                .and_then(|map| map.get(text.as_bytes()))
                .cloned()
        };
        if let Some(hit) = hit {
            crate::loader_counters::cache(
                if op == Operation::Absolute {
                    "lookup.absolute"
                } else {
                    "lookup.directory"
                },
                true,
                text,
                base,
                reject_nul,
            );
            return Ok(hit);
        }
        crate::loader_counters::cache(
            if op == Operation::Absolute {
                "lookup.absolute"
            } else {
                "lookup.directory"
            },
            false,
            text,
            base,
            reject_nul,
        );
        // Run at the original call site with the original inputs. In
        // particular, errors are returned here and are never memoized.
        let value = calculate()?;
        #[cfg(feature = "loader-counters")]
        let value_bytes = value.as_bytes().len();
        crate::loader_counters::owned(
            "cache.value",
            "Arc<JsString>",
            std::mem::size_of::<JsString>() + 2 * std::mem::size_of::<usize>(),
            true,
        );
        let value = LexicalPath(Arc::new(value));
        let mut contexts = self.contexts.borrow_mut();
        #[cfg(feature = "loader-counters")]
        let mut new_base_bytes = 0;
        let context = match base {
            None => &mut contexts.no_base,
            Some(base) => {
                if !contexts.bases.contains_key(base.as_bytes()) {
                    #[cfg(feature = "loader-counters")]
                    {
                        new_base_bytes = base.as_bytes().len();
                    }
                    contexts.bases.insert(
                        crate::loader_counters::js_owned("cache.base", base),
                        DirectoryLexicalContext::default(),
                    );
                }
                contexts
                    .bases
                    .get_mut(base.as_bytes())
                    .expect("base inserted")
            }
        };
        context
            .operations
            .entry((op, reject_nul))
            .or_insert_with(|| HashMap::with_hasher(S::default()))
            .insert(
                crate::loader_counters::js_owned("cache.operand", text),
                value.clone(),
            );
        #[cfg(any(test, feature = "loader-counters"))]
        {
            contexts.entries += 1;
        }
        #[cfg(feature = "loader-counters")]
        {
            contexts.text_bytes += new_base_bytes + text.as_bytes().len() + value_bytes;
            crate::loader_counters::maximum("cache.entries_per_resolver", contexts.entries);
            crate::loader_counters::maximum("cache.text_bytes_per_resolver", contexts.text_bytes);
        }
        Ok(value)
    }

    pub(crate) fn normalize(
        &self,
        text: JsStr<'_>,
        base: Option<JsStr<'_>>,
        reject_nul: bool,
    ) -> Result<LexicalPath, ResolutionError> {
        self.get_or_insert(Operation::Absolute, text, base, reject_nul, || {
            normalize_absolute_js_path(text, base, reject_nul)
        })
    }

    pub(crate) fn normalize_owned(
        &self,
        text: JsStr<'_>,
        base: Option<JsStr<'_>>,
        reject_nul: bool,
    ) -> Result<JsString, ResolutionError> {
        self.normalize(text, base, reject_nul)
            .map(LexicalPath::into_owned)
    }

    pub(crate) fn directory(&self, text: JsStr<'_>) -> LexicalPath {
        self.get_or_insert(Operation::Directory, text, None, false, || {
            Ok(crate::js_path::directory_name(text))
        })
        .expect("directory_name is infallible")
    }

    pub(crate) fn file_context(
        &self,
        text: JsStr<'_>,
        base: JsStr<'_>,
    ) -> Result<FileResolutionContext, ResolutionError> {
        let file = self.normalize(text, Some(base), true)?;
        let directory = self.directory(file.as_js());
        Ok(FileResolutionContext { file, directory })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CompilerOptions, ModuleResolver, ResolutionMode};
    use std::path::{Path, PathBuf};
    use tsc_host::{CompilerHost, HostError, HostErrorKind, HostOperation, MemoryCompilerHost};
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../host/tests/support/scalar_query_bridge.rs"
    ));

    use std::hash::{BuildHasherDefault, Hasher};

    #[derive(Default)]
    struct Collision;
    impl Hasher for Collision {
        fn finish(&self) -> u64 {
            0
        }
        fn write(&mut self, _: &[u8]) {}
    }

    #[test]
    fn collisions_preserve_exact_bases_operands_operations_and_nul_policy() {
        let cache = PurePathCache::<BuildHasherDefault<Collision>>::default();
        let low = JsString::from_code_units(&[b'x' as u16, 0xdc00]);
        let high = JsString::from_code_units(&[b'x' as u16, 0xd800]);
        for base in [
            "/work/A",
            "/work/a",
            "C:/work",
            "//server/share",
            "file:///work/",
        ] {
            for text in [
                "a/../b".into(),
                "b/".into(),
                low.as_js(),
                high.as_js(),
                "a\\b".into(),
                "b\0".into(),
            ] {
                for reject_nul in [false, true] {
                    for _ in 0..3 {
                        assert_eq!(
                            cache
                                .normalize(text, Some(base.into()), reject_nul)
                                .map(LexicalPath::into_owned),
                            normalize_absolute_js_path(text, Some(base.into()), reject_nul)
                        );
                    }
                }
            }
        }
        let a = cache.normalize("/work/leaf".into(), None, true).unwrap();
        let b = cache
            .normalize("/work/leaf".into(), Some("".into()), true)
            .unwrap();
        assert_eq!(a, b);
        assert!(
            !Arc::ptr_eq(&a.0, &b.0),
            "no-base and empty base are separate keys"
        );
        assert_eq!(cache.directory(a.as_js()).as_js(), "/work");
        assert_eq!(
            cache.normalize(a.as_js(), None, false).unwrap().as_js(),
            "/work/leaf"
        );
        assert!(
            !Arc::ptr_eq(&a.0, &cache.normalize(a.as_js(), None, false).unwrap().0),
            "validation policies remain separate even on a successful NUL-free value"
        );
        let again = cache.normalize("/work/leaf".into(), None, true).unwrap();
        assert!(Arc::ptr_eq(&a.0, &again.0));
        assert_eq!(
            a.clone().with_trailing_separator(true).as_js(),
            "/work/leaf/"
        );
        assert_eq!(
            cache
                .normalize("/work/leaf".into(), None, true)
                .unwrap()
                .as_js(),
            "/work/leaf",
            "trailing restoration cannot mutate the cached pure value"
        );
    }

    #[test]
    fn errors_are_retried_without_retaining_entries_or_borrows() {
        let cache = PurePathCache::<FxBuildHasher>::default();
        for (text, base) in [
            ("", Some("/work")),
            ("a", None),
            ("a", Some("relative")),
            ("a\0", Some("/work")),
            ("a", Some("/work\0")),
        ] {
            for _ in 0..3 {
                assert_eq!(
                    cache
                        .normalize(text.into(), base.map(JsStr::from), true)
                        .map(LexicalPath::into_owned),
                    normalize_absolute_js_path(text.into(), base.map(JsStr::from), true)
                );
                assert_eq!(cache.contexts.borrow().entries, 0);
            }
        }
        let context = cache
            .file_context("./sub/../main.ts".into(), "/work".into())
            .unwrap();
        assert_eq!(context.file.as_js(), "/work/main.ts");
        assert_eq!(context.directory.as_js(), "/work");
        let repeated = cache
            .file_context("./sub/../main.ts".into(), "/work".into())
            .unwrap();
        assert!(Arc::ptr_eq(&context.file.0, &repeated.file.0));
        assert!(Arc::ptr_eq(&context.directory.0, &repeated.directory.0));
    }
    /// Pure lexical hits must not change even the repeated host observations of
    /// traced resolution (whose existing package/result caches are bypassed).
    struct LexicalObservationHost {
        inner: MemoryCompilerHost,
        events: RefCell<Vec<(&'static str, PathBuf)>>,
        denied: RefCell<Option<(&'static str, PathBuf)>>,
    }
    impl LexicalObservationHost {
        fn event(&self, kind: &'static str, path: &Path) -> Result<(), HostError> {
            self.events.borrow_mut().push((kind, path.to_path_buf()));
            if self
                .denied
                .borrow()
                .as_ref()
                .is_some_and(|(k, p)| *k == kind && p == path)
            {
                let operation = match kind {
                    "read" => HostOperation::ReadFile,
                    "file" => HostOperation::FileExists,
                    "realpath" => HostOperation::Realpath,
                    _ => HostOperation::DirectoryExists,
                };
                return Err(HostError::new(
                    HostErrorKind::PermissionDenied,
                    operation,
                    Some(path.to_path_buf()),
                    "warm lexical cache must observe this failure",
                ));
            }
            Ok(())
        }
    }
    impl CompilerHost for LexicalObservationHost {
        scalar_host_query_bridge!();
        fn current_directory(&self) -> Result<PathBuf, HostError> {
            self.event("current_directory", Path::new(""))?;
            self.inner.current_directory()
        }
        fn use_case_sensitive_file_names(&self) -> bool {
            self.events
                .borrow_mut()
                .push(("case_sensitive", PathBuf::new()));
            self.inner.use_case_sensitive_file_names()
        }
        fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
            self.event("read", path)?;
            self.inner.read_file(path)
        }
        fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
            self.event("file", path)?;
            self.inner.file_exists(path)
        }
        fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
            self.event("directory", path)?;
            self.inner.directory_exists(path)
        }
        fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
            self.event("read_directory", path)?;
            self.inner.read_directory(path)
        }
        fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
            self.event("realpath", path)?;
            self.inner.realpath(path)
        }
    }

    fn observation_host() -> LexicalObservationHost {
        LexicalObservationHost {
            inner: MemoryCompilerHost::builder("/work")
                .file("/work/src/main.ts", b"export {};".to_vec())
                .file("/work/src/value.ts", b"export {};".to_vec())
                .file(
                    "/work/node_modules/pkg/package.json",
                    br#"{"name":"pkg","version":"1.0.0","types":"index.d.ts"}"#.to_vec(),
                )
                .file("/work/node_modules/pkg/index.d.ts", b"export {};".to_vec())
                .file(
                    "/work/node_modules/cfg/package.json",
                    br#"{"name":"cfg","tsconfig":"base.json"}"#.to_vec(),
                )
                .file("/work/node_modules/cfg/base.json", b"{}".to_vec())
                .build()
                .unwrap(),
            events: RefCell::new(Vec::new()),
            denied: RefCell::new(None),
        }
    }

    #[test]
    fn repeated_hits_match_uncached_lexical_work_with_identical_host_cache_history() {
        let host = observation_host();
        let other = observation_host();
        let options = CompilerOptions {
            module: Some(199),
            module_resolution: Some(99),
            trace_resolution: Some(true),
            ..CompilerOptions::default()
        };
        let mut cached = ModuleResolver::new(&host, &options).unwrap();
        let mut oracle = ModuleResolver::new(&other, &options).unwrap();
        for specifier in ["./value", "pkg", "./missing"] {
            for _ in 0..4 {
                host.events.borrow_mut().clear();
                other.events.borrow_mut().clear();
                // Test-only control: preserve every pre-existing host/package
                // cache and its trace history; clear only pure lexical strings.
                *oracle.lexical_paths().contexts.borrow_mut() = Contexts::default();
                let actual = cached
                    .resolve_with_facts("/work/src/main.ts", specifier, ResolutionMode::CommonJs)
                    .unwrap();
                let expected = oracle
                    .resolve_with_facts("/work/src/main.ts", specifier, ResolutionMode::CommonJs)
                    .unwrap();
                assert_eq!(
                    actual, expected,
                    "paths, diagnostics and exact raw trace for {specifier}"
                );
                assert_eq!(
                    *host.events.borrow(),
                    *other.events.borrow(),
                    "ordered host calls for {specifier}"
                );
                assert!(!host.events.borrow().is_empty());
            }
        }
        for (kind, path, specifier) in [
            ("file", "/work/src/value.ts", "./value"),
            ("realpath", "/work/node_modules/pkg/index.d.ts", "pkg"),
        ] {
            *host.denied.borrow_mut() = Some((kind, PathBuf::from(path)));
            *other.denied.borrow_mut() = Some((kind, PathBuf::from(path)));
            for _ in 0..3 {
                host.events.borrow_mut().clear();
                other.events.borrow_mut().clear();
                *oracle.lexical_paths().contexts.borrow_mut() = Contexts::default();
                let actual = cached.resolve_with_facts(
                    "/work/src/main.ts",
                    specifier,
                    ResolutionMode::CommonJs,
                );
                let expected = oracle.resolve_with_facts(
                    "/work/src/main.ts",
                    specifier,
                    ResolutionMode::CommonJs,
                );
                assert!(
                    matches!(actual, Err(ResolutionError::Host(_))),
                    "host {kind} failure must still be observed: {actual:?}"
                );
                assert_eq!(actual, expected);
                assert_eq!(*host.events.borrow(), *other.events.borrow());
            }
            *host.denied.borrow_mut() = None;
            *other.denied.borrow_mut() = None;
            assert_eq!(
                cached
                    .resolve_with_facts("/work/src/main.ts", specifier, ResolutionMode::CommonJs)
                    .unwrap(),
                oracle
                    .resolve_with_facts("/work/src/main.ts", specifier, ResolutionMode::CommonJs)
                    .unwrap(),
                "request stack restored after {kind}"
            );
        }
        // Config extends deliberately bypasses the old package cache. Repeated
        // lexical hits must retain every read and failure in that region too.
        for _ in 0..4 {
            host.events.borrow_mut().clear();
            other.events.borrow_mut().clear();
            *oracle.lexical_paths().contexts.borrow_mut() = Contexts::default();
            assert_eq!(
                cached
                    .resolve_json_config("/work/src/tsconfig.json", "cfg")
                    .unwrap(),
                oracle
                    .resolve_json_config("/work/src/tsconfig.json", "cfg")
                    .unwrap()
            );
            assert_eq!(cached.take_trace(), oracle.take_trace());
            assert_eq!(*host.events.borrow(), *other.events.borrow());
            assert!(host
                .events
                .borrow()
                .contains(&("read", PathBuf::from("/work/node_modules/cfg/package.json"))));
        }
        *host.denied.borrow_mut() =
            Some(("read", PathBuf::from("/work/node_modules/cfg/package.json")));
        *other.denied.borrow_mut() =
            Some(("read", PathBuf::from("/work/node_modules/cfg/package.json")));
        for _ in 0..3 {
            host.events.borrow_mut().clear();
            other.events.borrow_mut().clear();
            *oracle.lexical_paths().contexts.borrow_mut() = Contexts::default();
            let actual = cached.resolve_json_config("/work/src/tsconfig.json", "cfg");
            let expected = oracle.resolve_json_config("/work/src/tsconfig.json", "cfg");
            assert!(matches!(actual, Err(ResolutionError::Host(_))));
            assert_eq!(actual, expected);
            assert_eq!(cached.take_trace(), oracle.take_trace());
            assert_eq!(*host.events.borrow(), *other.events.borrow());
        }
        *host.denied.borrow_mut() = None;
        *other.denied.borrow_mut() = None;
        assert_eq!(
            cached
                .resolve_json_config("/work/src/tsconfig.json", "cfg")
                .unwrap(),
            oracle
                .resolve_json_config("/work/src/tsconfig.json", "cfg")
                .unwrap()
        );
        assert_eq!(cached.take_trace(), oracle.take_trace());
    }
}
