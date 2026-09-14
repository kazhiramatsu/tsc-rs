// Legacy fixture hosts observe native scalar paths. Fail explicitly if a
// non-scalar query reaches this adapter; do not replace it or report a miss.
// Call the existing methods so injected faults and probe order stay observable.
macro_rules! scalar_host_query_bridge {
    () => {
        fn current_directory_js(&self) -> Result<tsc_diagnostics::JsString, tsc_host::HostError> {
            self.current_directory().map(|path| path.to_str().expect("scalar test host directory").into())
        }
        fn read_file_js(&self, path: tsc_diagnostics::JsStr<'_>) -> Result<Option<Vec<u8>>, tsc_host::HostError> {
            self.read_file(std::path::Path::new(path.as_str().expect("scalar test host query")))
        }
        fn file_exists_js(&self, path: tsc_diagnostics::JsStr<'_>) -> Result<bool, tsc_host::HostError> {
            self.file_exists(std::path::Path::new(path.as_str().expect("scalar test host query")))
        }
        fn directory_exists_js(&self, path: tsc_diagnostics::JsStr<'_>) -> Result<bool, tsc_host::HostError> {
            self.directory_exists(std::path::Path::new(path.as_str().expect("scalar test host query")))
        }
        fn read_directory_js(&self, path: tsc_diagnostics::JsStr<'_>) -> Result<Vec<tsc_diagnostics::JsString>, tsc_host::HostError> {
            self.read_directory(std::path::Path::new(path.as_str().expect("scalar test host query")))
                .map(|paths| paths.into_iter().map(|path| path.to_str().expect("scalar test host entry").into()).collect())
        }
        fn get_directories_js(&self, path: tsc_diagnostics::JsStr<'_>) -> Result<Vec<tsc_diagnostics::JsString>, tsc_host::HostError> {
            self.get_directories(std::path::Path::new(path.as_str().expect("scalar test host query")))
                .map(|paths| paths.into_iter().map(|path| path.to_str().expect("scalar test host entry").into()).collect())
        }
        fn realpath_js(&self, path: tsc_diagnostics::JsStr<'_>) -> Result<Option<tsc_diagnostics::JsString>, tsc_host::HostError> {
            self.realpath(std::path::Path::new(path.as_str().expect("scalar test host query")))
                .map(|path| path.map(|path| path.to_str().expect("scalar test host realpath").into()))
        }
    };
}
