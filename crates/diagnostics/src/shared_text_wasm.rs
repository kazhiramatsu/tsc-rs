//! WASM's `SharedText`: the store and its text heap are native only, so a
//! owned text uses `Arc<str>` and embedded text uses a static view.

use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

#[derive(Clone)]
enum Text {
    Owned(Arc<str>),
    Static(&'static str),
}

#[derive(Clone)]
pub struct SharedText(Text);

pub struct StaticText<const N: usize> {
    text: &'static str,
}

impl<const N: usize> StaticText<N> {
    pub const fn new(text: &'static str) -> Self {
        assert!(
            text.len() == N,
            "static text length differs from its storage"
        );
        Self { text }
    }
    pub const fn shared(&'static self) -> SharedText {
        SharedText(Text::Static(self.text))
    }
}

pub enum TextRead {
    Text(SharedText),
    Bytes(Vec<u8>),
}

pub fn read_text_file(path: &std::path::Path) -> std::io::Result<TextRead> {
    let bytes = std::fs::read(path)?;
    if !bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        if let Ok(text) = std::str::from_utf8(&bytes) {
            return Ok(TextRead::Text(SharedText::new(text)));
        }
    }
    Ok(TextRead::Bytes(bytes))
}

impl SharedText {
    pub fn new(text: &str) -> Self {
        Self(Text::Owned(Arc::from(text)))
    }

    pub fn as_str(&self) -> &str {
        match &self.0 {
            Text::Owned(text) => text,
            Text::Static(text) => text,
        }
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        self.as_str().as_ptr() == other.as_str().as_ptr()
            && self.as_str().len() == other.as_str().len()
    }

    #[doc(hidden)]
    pub fn in_text_heap(&self) -> bool {
        matches!(self.0, Text::Owned(_))
    }
}

impl From<&str> for SharedText {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl From<String> for SharedText {
    fn from(text: String) -> Self {
        Self(Text::Owned(Arc::from(text)))
    }
}

impl From<Arc<str>> for SharedText {
    fn from(text: Arc<str>) -> Self {
        Self(Text::Owned(text))
    }
}

impl Deref for SharedText {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for SharedText {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for SharedText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), formatter)
    }
}

impl PartialEq for SharedText {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other) || self.as_str() == other.as_str()
    }
}

impl Eq for SharedText {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_and_owned_texts_keep_the_native_text_contract() {
        static TEXT: StaticText<4> = StaticText::new("😀");
        let borrowed = TEXT.shared();
        let clone = borrowed.clone();
        assert!(borrowed.ptr_eq(&clone));
        assert!(!borrowed.in_text_heap());
        let owned = SharedText::new("😀");
        assert!(owned.in_text_heap());
        assert!(!borrowed.ptr_eq(&owned));
        assert_eq!(borrowed, owned);
        assert_eq!(SharedText::from(String::from("😀")), borrowed);
        assert_eq!(SharedText::from(Arc::<str>::from("😀")), borrowed);
        assert_eq!(SharedText::from("😀"), borrowed);
    }
}

#[cfg(test)]
#[test]
fn portable_file_contents_preserve_bom_and_invalid_input() {
    let path = std::env::temp_dir().join(format!("tsc-text-portable-{}", std::process::id()));
    for (bytes, shared) in [
        (b"valid".as_slice(), true),
        (b"\xef\xbb\xbftext".as_slice(), false),
        (b"bad\xff".as_slice(), false),
    ] {
        std::fs::write(&path, bytes).unwrap();
        match read_text_file(&path).unwrap() {
            TextRead::Text(text) => {
                assert!(shared);
                assert_eq!(text.as_bytes(), bytes);
            }
            TextRead::Bytes(original) => {
                assert!(!shared);
                assert_eq!(original, bytes);
            }
        }
    }
    std::fs::remove_file(path).unwrap();
}
