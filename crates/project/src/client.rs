//! tsgo `project.Client` (client.go): what the session asks of the editor;
//! and tsgo `logging.Logger`, where it logs.

use tsc_compiler::locale::Locale;
use tsc_lsproto::FileSystemWatcher;

/// tsgo `Client`, as far as the session uses it so far.
pub trait Client: Send + Sync {
    /// tsgo `WatchFiles`: the editor watches `watchers` under `id`.
    fn watch_files(&self, id: &str, watchers: Vec<FileSystemWatcher>) -> Result<(), String>;
    /// tsgo `UnwatchFiles`.
    fn unwatch_files(&self, id: &str) -> Result<(), String>;
    /// tsgo `RefreshInlayHints`: the editor asks for its inlay hints again.
    fn refresh_inlay_hints(&self) -> Result<(), String>;
    /// tsgo `RefreshCodeLens`: the editor asks for its code lenses again.
    fn refresh_code_lens(&self) -> Result<(), String>;
    /// tsgo `SetLocale`: the user's locale (`auto` is the editor's).
    fn set_locale(&self, locale: &str);
    /// tsgo `GetLocale`.
    fn locale(&self) -> Locale;
}

/// tsgo `logging.Logger`, as far as the session logs so far.
pub trait Logger: Send + Sync {
    /// tsgo `Log`.
    fn log(&self, message: &str);
}
