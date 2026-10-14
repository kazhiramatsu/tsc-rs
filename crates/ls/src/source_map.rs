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

use crate::language_service::Host;
use crate::lsconv::{Converters, TextScript};

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
            let script = TextScript {
                file_name: start.file_name.clone(),
                text: &text,
            };
            return Location {
                uri: DocumentUri(file_name_to_document_uri(&start.file_name)),
                range: self.converters.to_lsp_range(&script, start.pos..end),
            };
        }
        let script = TextScript {
            file_name: file_name.clone().into_owned(),
            text: source.text(),
        };
        Location {
            uri: DocumentUri(file_name_to_document_uri(&file_name)),
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
    fn document_position_mapper(&self, file_name: &str) -> Option<Rc<DocumentPositionMapper>> {
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
