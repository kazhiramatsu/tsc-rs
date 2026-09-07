//! Non-bundle declaration-map recording and artifact construction.
//!
//! The declaration lane shares the JavaScript map generator, path workers and
//! UTF-16 writer positions. Its map options explicitly omit both inline flags.

use std::path::Path;

use tsc_diagnostics::Diagnostic;
use tsc_types::CompilerOptions;

use crate::{
    source_map_recording_inputs_for, source_mapping_url, EmitArtifact, EmitContractViolation,
    EmitFailure, EmitTextMetadata, MapLaneInputs, NewLineKind, PrintedText, SourceMapObservation,
    SourceMapRecordingInputs,
};

/// The exact declaration map-options projection in emitDeclarationFileOrBundle.
/// tsc-port: emitDeclarationFileOrBundle @6.0.3
/// tsc-hash: 8275307ffb4a07e3c7d8b7a5d7f2acf16bfe01c5f746285165c54dc225904434
/// tsc-span: _tsc.js:116640-116715
fn map_options(options: &CompilerOptions) -> CompilerOptions {
    CompilerOptions {
        source_map: options.declaration_map,
        source_root: options.source_root.clone(),
        map_root: options.map_root.clone(),
        ..CompilerOptions::default()
    }
}

pub fn declaration_map_recording_inputs_for(
    lane: &MapLaneInputs,
    options: &CompilerOptions,
    declaration_path: &Path,
    source_path: &Path,
) -> SourceMapRecordingInputs {
    source_map_recording_inputs_for(lane, &map_options(options), declaration_path, source_path)
}

/// Map callback precedes declaration callback; emitted-file listing reverses
/// that pair. The executor retains responsibility for sink failures and lists.
pub struct DeclarationMapEmit {
    pub map: EmitArtifact,
    pub declaration: EmitArtifact,
    pub observation: SourceMapObservation,
}

/// Complete the shared printSourceFileOrBundle map branch after declaration
/// printing, preserving map bytes, URL position and source association.
/// tsc-port: printSourceFileOrBundle @6.0.3
/// tsc-hash: 46e5c19d92e497190c703090e7d95bbd22e310323b69dde9c43c0eac2fa6979b
/// tsc-span: _tsc.js:116744-116804
#[allow(clippy::too_many_arguments)]
pub fn finish_declaration_map(
    lane: &MapLaneInputs,
    options: &CompilerOptions,
    declaration_path: &Path,
    map_path: &Path,
    source_path: &Path,
    printed: &PrintedText,
    diagnostics: Vec<Diagnostic>,
    new_line: NewLineKind,
) -> Result<DeclarationMapEmit, EmitFailure> {
    let mut generator = printed.source_map().cloned().ok_or(EmitFailure::Contract(
        EmitContractViolation::SourceMapRecordingUnavailable,
    ))?;
    let map_json = generator.to_json_string();
    let observation = SourceMapObservation::new(
        generator
            .raw_sources()
            .iter()
            .map(|name| name.as_ref().into())
            .collect(),
        map_json.clone().into_boxed_str(),
    );
    let url = source_mapping_url(
        lane,
        &map_options(options),
        &map_json,
        declaration_path,
        Some(map_path),
        source_path,
    )?;
    let mut text = printed.text().to_owned();
    let mut url_position = printed.end().position();
    if printed.end().column() != 0 {
        text.push_str(new_line.text());
        url_position = url_position
            .checked_add(new_line.text().len() as u32)
            .ok_or(EmitFailure::Contract(
                EmitContractViolation::SourceMapRecordingUnavailable,
            ))?;
    }
    text.push_str("//# sourceMappingURL=");
    text.push_str(&url);
    Ok(DeclarationMapEmit {
        map: EmitArtifact::declaration_map(map_path, map_json, Some(vec![source_path.into()])),
        declaration: EmitArtifact::declaration(
            declaration_path,
            text,
            options.emit_bom == Some(true),
            Some(vec![source_path.into()]),
            EmitTextMetadata::new(diagnostics, Some(url_position)),
        ),
        observation,
    })
}
