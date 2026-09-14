// Preserve existing native Path comparisons at the scalar fixture observer.
// This deliberately supplies no implicit conversion or replacement fallback.
pub trait ScalarTestPath<'a> {
    fn scalar_test_path(self) -> &'a std::path::Path;
}
impl<'a> ScalarTestPath<'a> for tsc_diagnostics::JsStr<'a> {
    fn scalar_test_path(self) -> &'a std::path::Path {
        std::path::Path::new(self.as_str().expect("scalar legacy path observation"))
    }
}

impl<'a> ScalarTestPath<'a> for &'a tsc_diagnostics::JsString {
    fn scalar_test_path(self) -> &'a std::path::Path {
        self.as_js().scalar_test_path()
    }
}
