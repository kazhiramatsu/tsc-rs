//! Shared static reference fixture loader for program and compiler order tests.
use serde::Deserialize;
use std::path::PathBuf;
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_config_program_with_no_emit_override, parse_config_root_plan, CompilerConfigHost,
    ConfigRootPlanRequest, LibraryCatalog, PreparedProgram, ProgramLoadLimits, WorkerBudget,
};
#[derive(Deserialize)]
pub struct Fixture {
    pub name: String,
    pub files: std::collections::BTreeMap<String, String>,
    pub case_sensitive: bool,
    #[serde(default)]
    pub symlinks: std::collections::BTreeMap<String, String>,
    pub list_files: Vec<String>,
    pub explain_files: String,
    pub trace: String,
    pub check_trace: bool,
    pub diagnostics: Vec<serde_json::Value>,
    #[serde(default)]
    pub build_info: Option<serde_json::Value>,
}

pub fn load(fixture: &Fixture, workers: WorkerBudget, trace: bool) -> PreparedProgram {
    let mut config: serde_json::Value =
        serde_json::from_str(&fixture.files["tsconfig.json"]).expect("fixture config");
    config["compilerOptions"]["traceResolution"] = trace.into();
    let config = config.to_string();
    let mut host = MemoryCompilerHost::builder("/project").case_sensitive(fixture.case_sensitive);
    for (name, text) in &fixture.files {
        let text = if name == "tsconfig.json" {
            &config
        } else {
            text
        };
        host = host.file(format!("/project/{name}"), text.as_bytes().to_vec());
    }
    for (name, target) in &fixture.symlinks {
        host = host.realpath(format!("/project/{name}"), format!("/project/{target}"));
    }
    let host = host.build().expect("fixture host");
    let plan = parse_config_root_plan(
        &CompilerConfigHost::new(&host),
        ConfigRootPlanRequest {
            file_name: "/project/tsconfig.json".into(),
            text: config,
            base_path: "/project".into(),
        },
    )
    .expect("parse fixture config");
    load_config_program_with_no_emit_override(
        &host,
        &plan,
        &LibraryCatalog::typescript_7_1(PathBuf::from("/project/lib")),
        ProgramLoadLimits::new(128, 1024, 64, 1 << 20, 1 << 24).with_workers(workers),
    )
    .expect("load fixture")
}

pub fn fixture(name: &str) -> Fixture {
    let fixtures: Vec<Fixture> =
        serde_json::from_str(include_str!("../fixtures/loader-order.json"))
            .expect("fixture corpus");
    fixtures
        .into_iter()
        .find(|f| f.name == name)
        .expect("named fixture")
}
