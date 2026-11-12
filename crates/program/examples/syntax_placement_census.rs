//! Read-only placement census. No checking or timing; one CSV row per input.
use std::{collections::BTreeSet, path::PathBuf};
use tsc_program::{
    load_config_program_with_no_emit_override, parse_config_root_plan, CompilerConfigHost,
    ConfigRootPlanRequest, LibraryCatalog, ProgramLoadLimits,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tsc_program::set_default_js_doc_parsing_mode(tsc_syntax::JSDocParsingMode::ParseForTypeErrors);
    let config = PathBuf::from(std::env::args().nth(1).ok_or("config required")?);
    let root = config.parent().ok_or("config parent required")?;
    let host = tsc_host::FsCompilerHost::new(root, true)?;
    let plan = parse_config_root_plan(
        &CompilerConfigHost::new(&host),
        ConfigRootPlanRequest {
            file_name: config.to_string_lossy().as_ref().into(),
            text: std::fs::read_to_string(&config)?,
            base_path: root.to_string_lossy().as_ref().into(),
        },
    )?;
    let plan = plan.with_identity_domain(tsc_types::IdentityDomain::new());
    let roots: BTreeSet<_> = plan.file_names().iter().cloned().collect();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let catalog = LibraryCatalog::typescript_7_1(
        repo.join("vendor/typescript-native/7.1.0-dev-aa814927/upstream/tsc/internal/bundled/libs"),
    );
    let program = load_config_program_with_no_emit_override(
        &host,
        &plan,
        &catalog,
        ProgramLoadLimits::new(
            100000,
            10000000,
            10000,
            128 * 1024 * 1024,
            4 * 1024 * 1024 * 1024,
        ),
    )?;
    let libraries: BTreeSet<_> = program
        .library_files()
        .iter()
        .map(|id| id.index())
        .collect();
    println!("order,kind,root_order,bytes,nodes,arrays,items,text_words,path");
    for (index, file) in program.source_files().iter().enumerate() {
        let source = match file.preparsed_syntax().take() {
            Some(parsed) => parsed.into_source(),
            None if file.path().display().ends_with(".json") => {
                tsc_syntax::parse_json_source_text_from_snapshot(
                    file.path().display().to_owned(),
                    std::sync::Arc::clone(file.snapshot()),
                )
            }
            None => {
                return Err(format!(
                    "missing retained parse: {}",
                    file.path().display().to_string_lossy()
                )
                .into())
            }
        };
        let path = file.path().display();
        let kind = if libraries.contains(&index) {
            "lib"
        } else if roots.contains(&path.to_owned()) {
            "root"
        } else {
            "discovered"
        };
        let ordinal = plan
            .file_names()
            .iter()
            .position(|p| p.as_js() == path)
            .map_or(String::new(), |i| i.to_string());
        println!(
            "{index},{kind},{ordinal},{},{},{},{},{},\"{}\"",
            source.text().len(),
            source.arena.nodes().len(),
            source.arena.node_arrays().len(),
            source.arena.array_item_count(),
            source.arena.text_word_count(),
            path.to_string_lossy().replace('"', "\"\"")
        );
    }
    Ok(())
}
