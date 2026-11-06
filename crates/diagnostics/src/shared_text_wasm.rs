//! WASM's `SharedText`: the store and its text heap are native only, so a
//! text is a plain `Arc<str>` here, with the native type's API.

use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

#[derive(Clone)]
pub struct SharedText(Arc<str>);

impl SharedText {
    pub fn new(text: &str) -> Self {
        Self(Arc::from(text))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    #[doc(hidden)]
    pub fn in_text_heap(&self) -> bool {
        true
    }
}

impl From<&str> for SharedText {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl From<String> for SharedText {
    fn from(text: String) -> Self {
        Self(Arc::from(text))
    }
}

impl From<Arc<str>> for SharedText {
    fn from(text: Arc<str>) -> Self {
        Self(text)
    }
}

impl Deref for SharedText {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for SharedText {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SharedText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.0, formatter)
    }
}

impl PartialEq for SharedText {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other) || *self.0 == *other.0
    }
}

impl Eq for SharedText {}
