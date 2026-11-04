//! Compile-time loader measurement. Counts explicit owned constructions at
//! the instrumented sites, not allocator traffic from the host or checker.
//! Inline JsStrings and empty vectors count as values, but not heap buffers.
use tsc_diagnostics::{JsStr, JsString};

#[cfg(feature = "loader-counters")]
mod enabled {
    use super::*;
    use std::collections::{BTreeMap, HashSet};
    use std::sync::Mutex;

    type Key = (Option<Vec<u8>>, Vec<u8>, &'static str, bool);

    #[derive(Default)]
    pub(super) struct Counters {
        pub calls: BTreeMap<&'static str, u64>,
        pub keys: HashSet<Key>,
        pub unique: BTreeMap<&'static str, u64>,
        pub values: BTreeMap<(&'static str, &'static str), (u64, u64, u64)>,
        pub maxima: BTreeMap<&'static str, usize>,
        pub plans: BTreeMap<JsString, [usize; 4]>,
    }

    pub(super) fn with<T>(f: impl FnOnce(&mut Counters) -> T) -> T {
        static COUNTERS: std::sync::OnceLock<Mutex<Counters>> = std::sync::OnceLock::new();
        f(&mut COUNTERS.get_or_init(Mutex::default).lock().unwrap())
    }

    pub(super) fn path(op: &'static str, text: JsStr<'_>, base: Option<JsStr<'_>>, nul: bool) {
        with(|s| {
            *s.calls.entry(op).or_default() += 1;
            if s.keys.insert((
                base.map(|b| b.as_bytes().to_vec()),
                text.as_bytes().to_vec(),
                op,
                nul,
            )) {
                *s.unique.entry(op).or_default() += 1;
            }
        });
    }
}

#[inline]
pub(crate) fn path(op: &'static str, text: JsStr<'_>, base: Option<JsStr<'_>>, nul: bool) {
    #[cfg(feature = "loader-counters")]
    enabled::path(op, text, base, nul);
    #[cfg(not(feature = "loader-counters"))]
    let _ = (op, text, base, nul);
}

/// An explicit owned construction: value count, nonempty heap buffer count,
/// and requested bytes. Reallocations, capacity rounding and measurement
/// bookkeeping are excluded. Keep site names stable across the A/B builds.
#[inline]
pub fn owned(site: &'static str, kind: &'static str, bytes: usize, heap: bool) {
    #[cfg(feature = "loader-counters")]
    enabled::with(|s| {
        let row = s.values.entry((site, kind)).or_default();
        row.0 += 1;
        row.1 += u64::from(heap);
        row.2 += if heap { bytes as u64 } else { 0 };
    });
    #[cfg(not(feature = "loader-counters"))]
    let _ = (site, kind, bytes, heap);
}

#[inline]
pub(crate) fn js_owned(site: &'static str, text: JsStr<'_>) -> JsString {
    js(site, text);
    text.to_owned()
}

#[inline]
pub(crate) fn js(site: &'static str, text: JsStr<'_>) {
    owned(
        site,
        "JsString",
        text.as_bytes().len(),
        text.as_bytes().len() > 15,
    );
}

/// A clone preserves heap storage even when a previously longer string was
/// truncated into the inline-size range. Inspect its actual storage.
#[cfg(feature = "loader-counters")]
pub(crate) fn js_clone(site: &'static str, text: &JsString) {
    owned(
        site,
        "JsString",
        text.as_bytes().len(),
        text.heap_bytes() != 0 && !text.is_empty(),
    );
}

#[cfg(feature = "loader-counters")]
pub(crate) fn plan(file: JsStr<'_>, values: [usize; 4]) {
    enabled::with(|s| {
        let maxima = s.plans.entry(file.to_owned()).or_default();
        for (maximum, value) in maxima.iter_mut().zip(values) {
            *maximum = (*maximum).max(value);
        }
    });
}

#[inline]
pub(crate) fn js_capacity(site: &'static str, bytes: usize) -> JsString {
    owned(site, "JsString", bytes, bytes > 15);
    JsString::with_capacity(bytes)
}

#[inline]
#[cfg(feature = "loader-counters")]
pub(crate) fn vector<T>(site: &'static str, len: usize) {
    owned(
        site,
        "Vec",
        len * std::mem::size_of::<T>(),
        len != 0 && std::mem::size_of::<T>() != 0,
    );
}

#[inline]
#[cfg(feature = "loader-counters")]
pub(crate) fn maximum(name: &'static str, value: usize) {
    #[cfg(feature = "loader-counters")]
    enabled::with(|s| {
        let row = s.maxima.entry(name).or_default();
        *row = (*row).max(value);
    });
    #[cfg(not(feature = "loader-counters"))]
    let _ = (name, value);
}

#[inline]
pub fn diagnostic_clones(site: &'static str, diagnostics: &[tsc_diagnostics::Diagnostic]) {
    #[cfg(feature = "loader-counters")]
    {
        use tsc_diagnostics::{MessageChain, RelatedInfo, Repopulate};
        fn related(site: &'static str, values: &[RelatedInfo]) {
            vector::<RelatedInfo>(site, values.len());
            for value in values {
                if let Some(name) = &value.file_name {
                    js_clone(site, name);
                }
                message(site, &value.message);
            }
        }
        fn message(site: &'static str, value: &MessageChain) {
            js_clone(site, &value.text);
            vector::<JsString>(site, value.args.len());
            for arg in &value.args {
                js_clone(site, arg);
            }
            vector::<MessageChain>(site, value.next.len());
            for next in &value.next {
                message(site, next);
            }
            related(site, &value.related);
            if let Some(Repopulate::ModuleNotFound {
                module_reference,
                package_name,
                ..
            }) = &value.repopulate
            {
                js_clone(site, module_reference);
                if let Some(name) = package_name {
                    js_clone(site, name);
                }
            }
        }
        for diagnostic in diagnostics {
            if let Some(name) = &diagnostic.file_name {
                js_clone(site, name);
            }
            if let Some(source) = &diagnostic.source {
                owned(site, "String", source.len(), !source.is_empty());
            }
            message(site, &diagnostic.message);
            related(site, &diagnostic.related);
        }
    }
    #[cfg(not(feature = "loader-counters"))]
    let _ = (site, diagnostics);
}

#[inline]
pub(crate) fn cache(
    op: &'static str,
    hit: bool,
    text: JsStr<'_>,
    base: Option<JsStr<'_>>,
    reject_nul: bool,
) {
    #[cfg(feature = "loader-counters")]
    {
        enabled::path(op, text, base, reject_nul);
        enabled::with(|s| {
            *s.calls
                .entry(if hit { "cache.hit" } else { "cache.miss" })
                .or_default() += 1;
            *s.calls
                .entry(match (op, hit) {
                    ("lookup.absolute", true) => "cache.absolute.hit",
                    ("lookup.absolute", false) => "cache.absolute.miss",
                    (_, true) => "cache.directory.hit",
                    (_, false) => "cache.directory.miss",
                })
                .or_default() += 1;
        });
    }
    #[cfg(not(feature = "loader-counters"))]
    let _ = (op, hit, text, base, reject_nul);
}

/// Reset at the CLI run boundary. Only measurement builds call this.
#[cfg(feature = "loader-counters")]
pub fn reset() {
    enabled::with(|s| *s = enabled::Counters::default());
}

/// Deterministic stderr report; measurement keys retain exact WTF-8 bytes
/// and distinguish no base, an empty base, operation and validation policy.
#[cfg(feature = "loader-counters")]
pub fn report() -> String {
    use std::fmt::Write;
    enabled::with(|s| {
        let mut out = String::from("loader-counters v1 (explicit owned constructions; excludes growth and instrumentation)\n");
        for (op, calls) in &s.calls {
            let unique = s.unique.get(op).copied().unwrap_or_default();
            writeln!(out, "loader.calls.{op}={calls} unique={unique}").unwrap();
        }
        // Present even at A3-0, before the cache exists.
        for name in ["cache.hit", "cache.miss"] {
            if !s.calls.contains_key(name) {
                writeln!(out, "loader.calls.{name}=0 unique=0").unwrap();
            }
        }
        for ((site, kind), (values, heaps, bytes)) in &s.values {
            writeln!(
                out,
                "loader.owned.{site}.{kind}={values} heap={heaps} bytes={bytes}"
            )
            .unwrap();
        }
        for (name, value) in &s.maxima {
            writeln!(out, "loader.high_water.{name}={value}").unwrap();
        }
        for (file, values) in &s.plans {
            writeln!(out, "loader.plan.{file:?}=requests:{} occurrences:{} observed:{} vector_capacity_bytes:{}", values[0], values[1], values[2], values[3]).unwrap();
        }
        out
    })
}
