//! tsgo `ls/source_map.go`: a program file's range as a protocol location,
//! following the declaration maps of declaration files to their sources
//! (a content mapper's projections come with the content mapper's language
//! service).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ops::Range;
use std::rc::Rc;

use tsc_emitter::source_mapper::{
    get_document_position_mapper, DocumentPosition, DocumentPositionMapper,
};
use tsc_lsproto::{DocumentUri, Location};
use tsc_program::uri::file_name_to_document_uri;
use tsc_syntax::SourceFile;

use tsc_types::JsStr;

use crate::language_service::Host;
use crate::lsconv::{Converters, Script, TextScript};
use crate::request::Request;
use crate::utilities::canonical_path;

/// The source maps one request reads (tsgo `documentPositionMappers`).
pub(crate) struct SourceMaps<'h> {
    host: &'h dyn Host,
    converters: &'h Converters,
    mappers: RefCell<BTreeMap<String, Option<Rc<DocumentPositionMapper>>>>,
}

impl<'h> SourceMaps<'h> {
    pub(crate) fn new(host: &'h dyn Host, converters: &'h Converters) -> Self {
        Self {
            host,
            converters,
            mappers: RefCell::new(BTreeMap::new()),
        }
    }

    /// tsgo `sourceFileRangeToLSPLocation` (and its `ForFeature` form) of a
    /// file without a content mapper: tsgo `getMappedLocation`, a
    /// declaration file's range at its source when its map has one.
    pub(crate) fn location(&self, source: &SourceFile, range: Range<usize>) -> Location {
        let file_name = source.file_name.to_string_lossy();
        if let Some(start) = self.try_get_source_position(&file_name, range.start) {
            // An end that does not map, maps to another source (an outFile
            // declaration's multi-source map) or before the start (mappings
            // that are not monotonic) is approximated.
            let end = match self.try_get_source_position(&file_name, range.end) {
                Some(end) if end.file_name == start.file_name && end.pos >= start.pos => end.pos,
                _ => start.pos + range.len(),
            };
            let text = self.host.read_file(&start.file_name).unwrap_or_default();
            let script = TextScript::new(start.file_name.clone(), &text);
            return Location {
                uri: DocumentUri(file_name_to_document_uri(&start.file_name)),
                range: self.converters.to_lsp_range(&script, start.pos..end),
            };
        }
        // tsgo `ToLSPLocation`: a content mapper's output's range in its
        // original file.
        let script = crate::utilities::script_of(source);
        Location {
            uri: DocumentUri(file_name_to_document_uri(script.original_file_name())),
            range: self.converters.to_lsp_range(&script, range),
        }
    }

    /// tsgo `tryGetSourcePosition`: a declaration file's position in the
    /// source its maps lead to, when that source exists.
    pub(crate) fn try_get_source_position(
        &self,
        file_name: &str,
        position: usize,
    ) -> Option<DocumentPosition> {
        let position = self.try_get_source_position_worker(file_name, position)?;
        self.host.read_file(&position.file_name)?;
        Some(position)
    }

    fn try_get_source_position_worker(
        &self,
        file_name: &str,
        position: usize,
    ) -> Option<DocumentPosition> {
        if !tsc_program::is_declaration_file_name(file_name) {
            return None;
        }
        let mapper = self.document_position_mapper(file_name)?;
        let document_position = mapper.source_position(position)?;
        Some(
            self.try_get_source_position_worker(
                &document_position.file_name,
                document_position.pos,
            )
            .unwrap_or(document_position),
        )
    }

    /// tsgo `GetDocumentPositionMapper`, once per file.
    pub(crate) fn document_position_mapper(
        &self,
        file_name: &str,
    ) -> Option<Rc<DocumentPositionMapper>> {
        if let Some(mapper) = self.mappers.borrow().get(file_name) {
            return mapper.clone();
        }
        let mapper = get_document_position_mapper(self.host, file_name).map(Rc::new);
        self.mappers
            .borrow_mut()
            .insert(file_name.to_owned(), mapper.clone());
        mapper
    }
}

impl Request<'_, '_> {
    /// tsgo `IsSourceFromProjectReference`: whether the program reads the
    /// file, a referenced project's source, in place of its output.
    pub(crate) fn is_source_from_project_reference(&self, file_name: &str) -> bool {
        canonical_path(self.prepared, file_name)
            .is_some_and(|path| self.prepared.is_source_from_project_reference(&path))
    }

    /// tsgo `tryGetGeneratedPosition`: a program source's position in the
    /// declaration file the program emits for it, through that file's
    /// declaration map, when the declaration file exists.
    pub(crate) fn try_get_generated_position(
        &self,
        file_name: &str,
        position: usize,
    ) -> Option<DocumentPosition> {
        let position = self.try_get_generated_position_worker(file_name, position)?;
        self.host.read_file(&position.file_name)?;
        Some(position)
    }

    fn try_get_generated_position_worker(
        &self,
        file_name: &str,
        position: usize,
    ) -> Option<DocumentPosition> {
        if tsc_program::is_declaration_file_name(file_name) {
            return None;
        }
        let path = canonical_path(self.prepared, file_name)?;
        self.prepared.source_id(&path)?;
        // A referenced project's source (not its output) has no generated
        // position.
        if self.prepared.is_source_from_project_reference(&path) {
            return None;
        }
        let declaration_file_name = tsc_program::output_declaration_file_name(
            JsStr::from_str(file_name),
            self.prepared.compiler_options(),
            self.prepared.common_source_directory().as_js(),
            self.prepared.current_directory().display(),
            self.prepared.path_context().use_case_sensitive_file_names(),
        );
        let mapper = self
            .source_maps
            .document_position_mapper(&declaration_file_name.to_string_lossy())?;
        let document_position = mapper.generated_position(file_name, position)?;
        Some(
            self.try_get_generated_position_worker(
                &document_position.file_name,
                document_position.pos,
            )
            .unwrap_or(document_position),
        )
    }
}
