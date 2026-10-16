//! tsgo's parse cache as a Program's loader reads it: the project system's
//! compiler host answers `GetSourceFile` from the session's parse cache
//! (project/parsecache.go `Acquire`), so a Program built again parses only
//! the files no Program holds with the same text and parse options.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use tsc_syntax::{ParseOptions, SourceFile};
use tsc_types::JsStr;

/// The parses Programs hold, by file name, text and parse options.
pub trait ParseCache: Send + Sync {
    /// The parse a Program holds of `file_name` with `text`, parsed with
    /// `options` (identity bases zero).
    fn find(&self, file_name: JsStr<'_>, text: &str, options: &ParseOptions)
        -> Option<CachedParse>;
}

/// A parse from a [`ParseCache`], with what keeps it in the cache while the
/// loader plans the file's requests from it.
#[derive(Clone)]
pub struct CachedParse {
    source: Arc<SourceFile>,
    _holder: Arc<dyn Any + Send + Sync>,
}

impl CachedParse {
    pub fn new(source: Arc<SourceFile>, holder: Arc<dyn Any + Send + Sync>) -> Self {
        Self {
            source,
            _holder: holder,
        }
    }

    pub fn source(&self) -> &SourceFile {
        &self.source
    }
}

impl fmt::Debug for CachedParse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CachedParse")
            .field("file_name", &self.source.file_name)
            .finish_non_exhaustive()
    }
}

/// The [`ParseCache`] in a program's options, if any. Every handle compares
/// equal: a cache answers the parse the loader would make, so it tells no
/// programs apart.
#[derive(Clone, Default)]
pub struct ParseCacheHandle(Option<Arc<dyn ParseCache>>);

impl ParseCacheHandle {
    pub fn new(cache: Arc<dyn ParseCache>) -> Self {
        Self(Some(cache))
    }

    pub fn get(&self) -> Option<&dyn ParseCache> {
        self.0.as_deref()
    }
}

impl PartialEq for ParseCacheHandle {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for ParseCacheHandle {}

impl fmt::Debug for ParseCacheHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ParseCacheHandle")
    }
}
