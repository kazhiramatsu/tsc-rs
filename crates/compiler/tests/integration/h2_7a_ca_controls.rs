//! Declaration-family options pass the emit request validation.

use std::path::Path;

use tsc_checker::CompilerOptions;
use tsc_emitter::{EmitHost, SourceFileId};

struct ControlHost {
    options: CompilerOptions,
    sources: [SourceFileId; 1],
}

impl EmitHost for ControlHost {
    fn compiler_options(&self) -> &CompilerOptions {
        &self.options
    }

    fn current_directory(&self) -> tsc_diagnostics::JsStr<'_> {
        (Path::new("/control"))
            .to_str()
            .expect("scalar mock host directory")
            .into()
    }

    fn common_source_directory(&self) -> tsc_diagnostics::JsStr<'_> {
        (Path::new("/control"))
            .to_str()
            .expect("scalar mock host directory")
            .into()
    }

    fn config_file_path(&self) -> Option<tsc_diagnostics::JsStr<'_>> {
        None
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn source_file_ids(&self) -> &[SourceFileId] {
        &self.sources
    }

    fn source_file(&self, id: SourceFileId) -> Option<tsc_emitter::EmitSource<'_>> {
        (id == self.sources[0]).then(|| {
            let path = Path::new("/control/input.ts");
            tsc_emitter::EmitSource::new(
                id,
                path.to_str().expect("scalar control source").into(),
                path.to_str().expect("scalar control source").into(),
                true,
                None,
                None,
            )
        })
    }
}

fn control_host(options: CompilerOptions) -> ControlHost {
    ControlHost {
        options,
        sources: [SourceFileId::from_raw(0)],
    }
}

#[test]
fn declaration_family_options_remain_typed_refusals() {
    // The unchanged declarationMap-only input now reports the ordinary
    // TS5069 prerequisite diagnostic and emits JavaScript (E m3 original
    // corpus comparison); it is no longer an emit-request refusal.
    let declaration_map = control_host(CompilerOptions {
        declaration_map: Some(true),
        ..CompilerOptions::default()
    });
    assert_eq!(
        tsc_emitter::validate_bootstrap_emit_request(&declaration_map),
        Ok(()),
    );
    let declaration_only = control_host(CompilerOptions {
        emit_declaration_only: Some(true),
        ..CompilerOptions::default()
    });
    assert_eq!(
        tsc_emitter::validate_bootstrap_emit_request(&declaration_only),
        Ok(())
    );
    let combined = control_host(CompilerOptions {
        declaration: Some(true),
        emit_declaration_only: Some(true),
        ..CompilerOptions::default()
    });
    assert_eq!(
        tsc_emitter::validate_bootstrap_emit_request(&combined),
        Ok(())
    );
    let strip_internal = control_host(CompilerOptions {
        strip_internal: Some(true),
        ..CompilerOptions::default()
    });
    assert_eq!(
        tsc_emitter::validate_bootstrap_emit_request(&strip_internal),
        Ok(()),
    );
    let bundle = control_host(CompilerOptions {
        out_file: Some("/control/bundle.js".to_owned().into()),
        ..CompilerOptions::default()
    });
    assert_eq!(
        tsc_emitter::validate_bootstrap_emit_request(&bundle),
        Ok(())
    );

    let host = control_host(CompilerOptions {
        declaration: Some(true),
        ..CompilerOptions::default()
    });
    assert_eq!(tsc_emitter::validate_bootstrap_emit_request(&host), Ok(()));
}
