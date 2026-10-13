//! tsgo `ls.LanguageService` (languageservice.go): one program of a
//! snapshot with the snapshot's position conversions, the user's
//! preferences and the client's capabilities.

use tsc_compiler::LiveProgram;
use tsc_diagnostics::{JsStr, MessageCatalog};
use tsc_emitter::source_mapper::SourceMapperHost;
use tsc_lsproto::{DocumentUri, ResolvedClientCapabilities};
use tsc_program::uri::uri_to_file_name;

use crate::lsconv::{Converters, TextScript};
use crate::lsutil::UserPreferences;

/// Why a language service request failed: the program could not answer
/// (a module the checker could not load).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LsError(pub String);

impl std::fmt::Display for LsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// tsgo `ls.Host`: what the language service reads beyond its program —
/// the snapshot's files (a declaration's source map and its sources).
pub trait Host: SourceMapperHost {}

/// tsgo `LanguageService`.
pub struct LanguageService<'a> {
    pub(crate) program: &'a mut LiveProgram,
    pub(crate) host: &'a dyn Host,
    pub(crate) converters: &'a Converters,
    preferences: &'a UserPreferences,
    pub(crate) capabilities: &'a ResolvedClientCapabilities,
    /// The messages of the request's locale.
    pub(crate) catalog: Option<&'a dyn MessageCatalog>,
}

impl<'a> LanguageService<'a> {
    /// tsgo `NewLanguageService`.
    pub fn new(
        program: &'a mut LiveProgram,
        host: &'a dyn Host,
        converters: &'a Converters,
        preferences: &'a UserPreferences,
        capabilities: &'a ResolvedClientCapabilities,
        catalog: Option<&'a dyn MessageCatalog>,
    ) -> Self {
        Self {
            program,
            host,
            converters,
            preferences,
            capabilities,
            catalog,
        }
    }

    /// tsgo `UserPreferences`.
    pub fn user_preferences(&self) -> &UserPreferences {
        self.preferences
    }

    /// tsgo `getProgramAndFile`: the document's file in the program, which
    /// must have it (tsgo panics, and the server answers the panic).
    pub(crate) fn file(&self, uri: &DocumentUri) -> usize {
        let file_name = uri_to_file_name(uri.as_str()).unwrap_or_default();
        self.program
            .file_index(JsStr::from_str(&file_name))
            .unwrap_or_else(|| panic!("file not found: {file_name}"))
    }

    /// A program file's name and text, for converting its positions.
    pub(crate) fn diagnostic_file(&self, file_name: &str) -> Option<TextScript<'_>> {
        let index = self.program.file_index(JsStr::from_str(file_name))?;
        let document = self.program.document(index)?;
        Some(TextScript {
            file_name: file_name.to_owned(),
            text: document.source().text(),
        })
    }
}
