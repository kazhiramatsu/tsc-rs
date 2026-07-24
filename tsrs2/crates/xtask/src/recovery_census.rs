//! Phase 9.7a: reproducible census of the parse-recovery overload curtain.
//!
//! This is deliberately measurement-only. It starts from conformance's
//! reached F2 boundary evidence, then compares the declaration tree and
//! binder declaration order with the vendored tsc oracle.

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tsrs2_checker::{check_program_with_libs_at, InputFile, PartialCheck};
use tsrs2_syntax::{NodeId, SourceFile, SyntaxKind};

const F2_REASON: &str = "overload band over a parse-recovery tree (declaration boundaries diverge)";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    cases: Vec<ManifestCase>,
    #[serde(default)]
    shapes: Vec<ManifestShape>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(deny_unknown_fields)]
struct ManifestCase {
    fixture: String,
    #[serde(rename = "matrixKey")]
    matrix_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestShape {
    id: String,
    fingerprint: String,
    fixture: String,
    #[serde(default, rename = "matrixKey")]
    matrix_key: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct RecoveryDump {
    files: Vec<FileDump>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct FileDump {
    name: String,
    #[serde(rename = "parseDiagnostics")]
    parse_diagnostics: Vec<ParseDiagnostic>,
    declarations: Vec<DeclarationEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct ParseDiagnostic {
    code: u32,
    start: u32,
    length: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct DeclarationEntry {
    declaration: DeclarationRef,
    symbol: Option<SymbolRef>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct SymbolRef {
    #[serde(rename = "escapedName")]
    escaped_name: String,
    declarations: Vec<DeclarationRef>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct DeclarationRef {
    kind: u16,
    pos: u32,
    end: u32,
    missing: bool,
    #[serde(rename = "parentKind")]
    parent_kind: Option<u16>,
    name: Option<NameRef>,
    body: Option<NodeRef>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct NameRef {
    kind: u16,
    pos: u32,
    end: u32,
    missing: bool,
    text: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct NodeRef {
    kind: u16,
    pos: u32,
    end: u32,
    missing: bool,
}

#[derive(Debug, Serialize)]
struct CensusReport {
    schema: u32,
    reason: &'static str,
    cases: Vec<CaseReport>,
    unique_shape_fingerprints: Vec<String>,
    summary: CensusSummary,
}

#[derive(Debug, Serialize)]
struct CensusSummary {
    selected_cases: usize,
    partial_ranges: usize,
    exact_regions: usize,
    differing_regions: usize,
    unique_shapes: usize,
}

#[derive(Debug, Serialize)]
struct CaseReport {
    fixture: String,
    #[serde(rename = "matrixKey")]
    matrix_key: String,
    regions: Vec<RegionReport>,
}

#[derive(Debug, Serialize)]
struct RegionReport {
    file: String,
    start: u32,
    length: u32,
    fingerprint: String,
    exact: bool,
    tsrs: RegionDump,
    oracle: RegionDump,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct RegionDump {
    parse_boundaries: Vec<BoundaryDiagnostic>,
    groups: Vec<GroupDump>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct BoundaryDiagnostic {
    code: u32,
    relative_start: i64,
    length: u32,
    relation: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct GroupDump {
    escaped_name: String,
    declarations: Vec<RegionDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct RegionDeclaration {
    kind: u16,
    parent_kind: Option<u16>,
    relative_pos: i64,
    relative_end: i64,
    missing: bool,
    name: Option<RegionName>,
    body: Option<RegionNode>,
    parse_boundaries: Vec<BoundaryDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct RegionName {
    kind: u16,
    relative_pos: i64,
    relative_end: i64,
    missing: bool,
    text: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct RegionNode {
    kind: u16,
    relative_pos: i64,
    relative_end: i64,
    missing: bool,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
struct ShapeDump {
    parse_boundaries: Vec<&'static str>,
    groups: Vec<ShapeGroup>,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
struct ShapeGroup {
    name_class: &'static str,
    declarations: Vec<ShapeDeclaration>,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
struct ShapeDeclaration {
    kind: u16,
    parent_kind: Option<u16>,
    name_kind: Option<u16>,
    name_missing: Option<bool>,
    body_kind: Option<u16>,
    body_missing: Option<bool>,
    parse_boundaries: Vec<&'static str>,
}

pub fn run(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let workspace = super::find_tsrs2_root()?;
    let mut manifest_path = workspace.join("pins/recovery.json");
    let mut out_path = workspace.join("target/recovery-census.json");
    let mut check = false;
    let mut probe = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--manifest" => {
                manifest_path = PathBuf::from(args.next().ok_or("missing value after --manifest")?)
            }
            "--out" => out_path = PathBuf::from(args.next().ok_or("missing value after --out")?),
            "--check" => check = true,
            "--probe" => {
                probe = Some(PathBuf::from(
                    args.next().ok_or("missing value after --probe")?,
                ))
            }
            other => return Err(format!("unexpected recovery-census argument: {other}").into()),
        }
    }
    if let Some(probe) = probe {
        return probe_fixture(&workspace, &probe);
    }
    if !manifest_path.is_absolute() {
        manifest_path = workspace.join(manifest_path);
    }
    if !out_path.is_absolute() {
        out_path = workspace.join(out_path);
    }
    let manifest: Manifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    if manifest.schema != 1 {
        return Err(format!("unsupported recovery census schema {}", manifest.schema).into());
    }

    let conformance_out = workspace.join("target/recovery-census-conformance.json");
    let summary = tsrs2_conformance::run_conformance(&tsrs2_conformance::ConformanceOptions {
        workspace: workspace.clone(),
        limit: None,
        files: Vec::new(),
        out_json: conformance_out,
        band: tsrs2_conformance::DiagnosticBand::TwoXxx,
    })?;
    run_census(&workspace, &manifest, &out_path, check, &summary)
}

pub fn check_with_summary(
    workspace: &Path,
    summary: &tsrs2_conformance::ConformanceSummary,
) -> Result<(), Box<dyn Error>> {
    if summary.band != "2xxx" {
        return Err(format!(
            "recovery census requires a 2xxx conformance summary, got {}",
            summary.band
        )
        .into());
    }
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(workspace.join("pins/recovery.json"))?)?;
    if manifest.schema != 1 {
        return Err(format!("unsupported recovery census schema {}", manifest.schema).into());
    }
    run_census(
        workspace,
        &manifest,
        &workspace.join("target/recovery-census.json"),
        true,
        summary,
    )
}

fn run_census(
    workspace: &Path,
    manifest: &Manifest,
    out_path: &Path,
    check: bool,
    summary: &tsrs2_conformance::ConformanceSummary,
) -> Result<(), Box<dyn Error>> {
    let observed_cases = selected_cases(summary);
    let expected_cases = manifest.cases.iter().cloned().collect::<BTreeSet<_>>();
    if expected_cases.len() != manifest.cases.len() {
        return Err("recovery census manifest contains duplicate cases".into());
    }
    if check && observed_cases != expected_cases {
        let added = observed_cases
            .difference(&expected_cases)
            .collect::<Vec<_>>();
        let stale = expected_cases
            .difference(&observed_cases)
            .collect::<Vec<_>>();
        return Err(format!(
            "recovery census case manifest drift: added={added:?}, stale={stale:?}"
        )
        .into());
    }

    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");
    let temp_root =
        std::env::temp_dir().join(format!("tsrs2-recovery-census-{}", std::process::id()));
    if temp_root.exists() {
        fs::remove_dir_all(&temp_root)?;
    }
    fs::create_dir_all(&temp_root)?;
    let mut oracle = RecoveryOracle::spawn(workspace)?;
    let mut reports = Vec::new();
    let mut shape_fingerprints = BTreeSet::new();
    for (case_index, case) in observed_cases.iter().enumerate() {
        let fixture = workspace.join("ts-tests/tests/cases").join(&case.fixture);
        let programs = tsrs2_harness::expand_fixture_file(&fixture, &vendor_lib_dir)?;
        let program = programs
            .iter()
            .find(|program| program.matrix_key == case.matrix_key)
            .ok_or_else(|| {
                format!(
                    "selected recovery case has no expanded program: {} [{}]",
                    case.fixture, case.matrix_key
                )
            })?;
        let out_dir = temp_root.join(case_index.to_string());
        let paths = tsrs2_harness::write_program_jsons(std::slice::from_ref(program), &out_dir)?;
        let oracle_dump = oracle.dump(&paths[0])?;
        let rust_dump = rust_dump(program)?;
        let partials = f2_partial_ranges(program, &vendor_lib_dir)?;
        if check && partials.is_empty() {
            return Err(format!(
                "selected recovery case no longer reaches the F2 boundary: {} [{}]",
                case.fixture, case.matrix_key
            )
            .into());
        }
        let mut regions = Vec::new();
        for partial in partials {
            let rust_file = find_file(&rust_dump, &partial.file_name)?;
            let oracle_file = find_file(&oracle_dump, &partial.file_name)?;
            let rust_region = region_dump(rust_file, partial.start, partial.length);
            let oracle_region = region_dump(oracle_file, partial.start, partial.length);
            let fingerprint = shape_fingerprint(&rust_region, &oracle_region)?;
            shape_fingerprints.insert(fingerprint.clone());
            regions.push(RegionReport {
                file: partial.file_name,
                start: partial.start,
                length: partial.length,
                fingerprint,
                exact: rust_region == oracle_region,
                tsrs: rust_region,
                oracle: oracle_region,
            });
        }
        reports.push(CaseReport {
            fixture: case.fixture.clone(),
            matrix_key: case.matrix_key.clone(),
            regions,
        });
    }
    let partial_ranges = reports.iter().map(|case| case.regions.len()).sum::<usize>();
    let exact_regions = reports
        .iter()
        .flat_map(|case| &case.regions)
        .filter(|region| region.exact)
        .count();
    let report = CensusReport {
        schema: 1,
        reason: F2_REASON,
        cases: reports,
        unique_shape_fingerprints: shape_fingerprints.iter().cloned().collect(),
        summary: CensusSummary {
            selected_cases: observed_cases.len(),
            partial_ranges,
            exact_regions,
            differing_regions: partial_ranges - exact_regions,
            unique_shapes: shape_fingerprints.len(),
        },
    };
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(out_path, serde_json::to_vec_pretty(&report)?)?;

    if check && !manifest.shapes.is_empty() {
        if report.summary.differing_regions != 0 {
            return Err(format!(
                "recovery census found {} non-exact declaration regions",
                report.summary.differing_regions
            )
            .into());
        }
        let expected = manifest
            .shapes
            .iter()
            .map(|shape| shape.fingerprint.clone())
            .collect::<BTreeSet<_>>();
        let ids = manifest
            .shapes
            .iter()
            .map(|shape| shape.id.as_str())
            .collect::<BTreeSet<_>>();
        if expected.len() != manifest.shapes.len() || ids.len() != manifest.shapes.len() {
            return Err("recovery shape ids and fingerprints must be unique".into());
        }
        if expected != shape_fingerprints {
            return Err(format!(
                "recovery shape manifest drift: observed={shape_fingerprints:?}, expected={expected:?}"
            )
            .into());
        }
        for shape in &manifest.shapes {
            if shape.id.is_empty() || shape.fixture.is_empty() {
                return Err("recovery shape entries require id and fixture".into());
            }
            let fixture = workspace.join(&shape.fixture);
            if !fixture.is_file() {
                return Err(format!(
                    "minimal recovery fixture for {} is missing: {}",
                    shape.id, shape.fixture
                )
                .into());
            }
            let programs = tsrs2_harness::expand_fixture_file(&fixture, &vendor_lib_dir)?;
            let program = programs
                .iter()
                .find(|program| program.matrix_key == shape.matrix_key)
                .ok_or_else(|| {
                    format!(
                        "minimal recovery fixture {} has no matrix case [{}]",
                        shape.fixture, shape.matrix_key
                    )
                })?;
            let out_dir = temp_root.join(format!("shape-{}", shape.id));
            let paths =
                tsrs2_harness::write_program_jsons(std::slice::from_ref(program), &out_dir)?;
            let oracle_dump = oracle.dump(&paths[0])?;
            let rust_dump = rust_dump(program)?;
            let partials = f2_partial_ranges(program, &vendor_lib_dir)?;
            let mut reproduced = false;
            for partial in partials {
                let rust_region = region_dump(
                    find_file(&rust_dump, &partial.file_name)?,
                    partial.start,
                    partial.length,
                );
                let oracle_region = region_dump(
                    find_file(&oracle_dump, &partial.file_name)?,
                    partial.start,
                    partial.length,
                );
                if rust_region != oracle_region {
                    return Err(format!(
                        "minimal recovery fixture {} is not exact at {}:{}+{}",
                        shape.id, partial.file_name, partial.start, partial.length
                    )
                    .into());
                }
                let fingerprint = shape_fingerprint(&rust_region, &oracle_region)?;
                reproduced |= fingerprint == shape.fingerprint;
            }
            if !reproduced {
                return Err(format!(
                    "minimal recovery fixture {} did not reproduce shape {}",
                    shape.id, shape.fingerprint
                )
                .into());
            }
        }
    }
    fs::remove_dir_all(&temp_root)?;
    println!(
        "recovery census: cases={} partial-ranges={} exact={} differing={} shapes={}",
        report.summary.selected_cases,
        report.summary.partial_ranges,
        report.summary.exact_regions,
        report.summary.differing_regions,
        report.summary.unique_shapes,
    );
    println!("report: {}", out_path.display());
    Ok(())
}

fn probe_fixture(workspace: &Path, fixture: &Path) -> Result<(), Box<dyn Error>> {
    let fixture = if fixture.is_absolute() {
        fixture.to_owned()
    } else {
        workspace.join(fixture)
    };
    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");
    let programs = tsrs2_harness::expand_fixture_file(&fixture, &vendor_lib_dir)?;
    let temp_root =
        std::env::temp_dir().join(format!("tsrs2-recovery-probe-{}", std::process::id()));
    if temp_root.exists() {
        fs::remove_dir_all(&temp_root)?;
    }
    fs::create_dir_all(&temp_root)?;
    let mut oracle = RecoveryOracle::spawn(workspace)?;
    for (index, program) in programs.iter().enumerate() {
        let out_dir = temp_root.join(index.to_string());
        let paths = tsrs2_harness::write_program_jsons(std::slice::from_ref(program), &out_dir)?;
        let oracle_dump = oracle.dump(&paths[0])?;
        let rust_dump = rust_dump(program)?;
        for partial in f2_partial_ranges(program, &vendor_lib_dir)? {
            let rust_region = region_dump(
                find_file(&rust_dump, &partial.file_name)?,
                partial.start,
                partial.length,
            );
            let oracle_region = region_dump(
                find_file(&oracle_dump, &partial.file_name)?,
                partial.start,
                partial.length,
            );
            println!(
                "{} [{}] {}:{}+{} fingerprint={} exact={}",
                fixture.display(),
                program.matrix_key,
                partial.file_name,
                partial.start,
                partial.length,
                shape_fingerprint(&rust_region, &oracle_region)?,
                rust_region == oracle_region,
            );
            println!("{}", serde_json::to_string(&shape_dump(&rust_region))?);
        }
    }
    fs::remove_dir_all(&temp_root)?;
    Ok(())
}

fn selected_cases(summary: &tsrs2_conformance::ConformanceSummary) -> BTreeSet<ManifestCase> {
    summary
        .mismatches
        .iter()
        .filter(|entry| {
            entry.fn_partial_boundary_audit.iter().any(|audit| {
                audit
                    .reasons
                    .iter()
                    .any(|reason| reason.as_str() == F2_REASON)
            })
        })
        .map(|entry| ManifestCase {
            fixture: entry.fixture.clone(),
            matrix_key: entry.matrix_key.clone(),
        })
        .collect()
}

fn f2_partial_ranges(
    program: &tsrs2_harness::ProgramJson,
    vendor_lib_dir: &Path,
) -> Result<Vec<PartialCheck>, Box<dyn Error>> {
    let files = program
        .files
        .iter()
        .map(|file| {
            Ok(InputFile {
                name: file.name.clone(),
                text: String::from_utf8(BASE64.decode(&file.text_b64)?)?,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let libs = program
        .libs
        .iter()
        .map(|name| {
            Ok(InputFile {
                name: name.clone(),
                text: fs::read_to_string(vendor_lib_dir.join(name))?,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let result = check_program_with_libs_at(
        &libs,
        &files,
        &tsrs2_conformance::compiler_options_from_program(program),
        &program.cwd,
    );
    let mut partials = result
        .partial_checks
        .into_iter()
        .filter(|partial| partial.reason == F2_REASON)
        .collect::<Vec<_>>();
    partials.sort_by(|left, right| {
        (&left.file_name, left.start, left.length).cmp(&(
            &right.file_name,
            right.start,
            right.length,
        ))
    });
    partials.dedup();
    Ok(partials)
}

fn rust_dump(program: &tsrs2_harness::ProgramJson) -> Result<RecoveryDump, Box<dyn Error>> {
    let options = tsrs2_conformance::compiler_options_from_program(program);
    let mut files = Vec::new();
    for file in &program.files {
        let is_js = [".js", ".jsx", ".mjs", ".cjs"]
            .iter()
            .any(|extension| file.name.ends_with(extension));
        if file.name.ends_with(".json") || (is_js && !options.allow_js) {
            continue;
        }
        let text = String::from_utf8(BASE64.decode(&file.text_b64)?)?;
        let source = tsrs2_syntax::parse_source_file(
            file.name.clone(),
            text,
            tsrs2_syntax::ParseOptions {
                language_variant: if file.name.ends_with(".tsx") || is_js {
                    tsrs2_syntax::LanguageVariant::Jsx
                } else {
                    tsrs2_syntax::LanguageVariant::Standard
                },
                javascript_file: is_js,
                ..tsrs2_syntax::ParseOptions::default()
            },
            None,
        );
        let binder = tsrs2_binder::bind_source_file(&source, &options);
        files.push(rust_file_dump(&source, &binder));
    }
    Ok(RecoveryDump { files })
}

fn rust_file_dump(source: &SourceFile, binder: &tsrs2_binder::Binder<'_>) -> FileDump {
    let map = tsrs2_diags::compute_line_map(&source.text);
    let to_utf16 =
        |pos: u32| -> u32 { map.byte_to_utf16.get(pos as usize).copied().unwrap_or(pos) };
    let mut entries = Vec::new();
    let mut stack = vec![source.root];
    while let Some(id) = stack.pop() {
        let node = source.arena.node(id);
        if is_census_kind(node.kind) {
            let symbol = binder.node_symbol.get(&id).copied().map(|symbol| {
                let symbol = binder.symbols.symbol(symbol);
                SymbolRef {
                    escaped_name: symbol.escaped_name.clone(),
                    declarations: symbol
                        .declarations
                        .iter()
                        .map(|&declaration| declaration_ref(source, declaration, &to_utf16))
                        .collect(),
                }
            });
            entries.push(DeclarationEntry {
                declaration: declaration_ref(source, id, &to_utf16),
                symbol,
            });
        }
        let mut children = Vec::new();
        tsrs2_syntax::for_each_child(&source.arena, node, |child| {
            children.push(child);
            false
        });
        stack.extend(children.into_iter().rev());
    }
    FileDump {
        name: source.file_name.clone(),
        parse_diagnostics: source
            .parse_diagnostics
            .iter()
            .map(|diagnostic| ParseDiagnostic {
                code: diagnostic.code(),
                start: diagnostic.start.unwrap_or(0),
                length: diagnostic.length.unwrap_or(0),
            })
            .collect(),
        declarations: entries,
    }
}

fn is_census_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ClassDeclaration
            | SyntaxKind::ClassExpression
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature
            | SyntaxKind::Constructor
    )
}

fn declaration_ref(
    source: &SourceFile,
    id: NodeId,
    to_utf16: &impl Fn(u32) -> u32,
) -> DeclarationRef {
    let node = source.arena.node(id);
    let name = tsrs2_binder::node_util::get_name_of_declaration(source, id).map(|name| {
        let node = source.arena.node(name);
        NameRef {
            kind: node.kind as u16,
            pos: to_utf16(node.pos),
            end: to_utf16(node.end),
            missing: tsrs2_binder::node_util::node_is_missing(source, Some(name)),
            text: tsrs2_binder::node_util::get_text_of_identifier_or_literal(source, name),
        }
    });
    let body = tsrs2_binder::node_util::body_of(source, id).map(|body| {
        let node = source.arena.node(body);
        NodeRef {
            kind: node.kind as u16,
            pos: to_utf16(node.pos),
            end: to_utf16(node.end),
            missing: tsrs2_binder::node_util::node_is_missing(source, Some(body)),
        }
    });
    DeclarationRef {
        kind: node.kind as u16,
        pos: to_utf16(node.pos),
        end: to_utf16(node.end),
        missing: tsrs2_binder::node_util::node_is_missing(source, Some(id)),
        parent_kind: node
            .parent
            .map(|parent| source.arena.node(parent).kind as u16),
        name,
        body,
    }
}

fn find_file<'a>(dump: &'a RecoveryDump, name: &str) -> Result<&'a FileDump, Box<dyn Error>> {
    dump.files
        .iter()
        .find(|file| file.name == name || file.name.ends_with(&format!("/{name}")))
        .ok_or_else(|| format!("recovery dump is missing file {name}").into())
}

fn region_dump(file: &FileDump, start: u32, length: u32) -> RegionDump {
    let end = start.saturating_add(length.max(1));
    let parse_boundaries = file
        .parse_diagnostics
        .iter()
        .map(|diagnostic| BoundaryDiagnostic {
            code: diagnostic.code,
            relative_start: i64::from(diagnostic.start) - i64::from(start),
            length: diagnostic.length,
            relation: range_relation(diagnostic.start, start, end),
        })
        .collect();
    let owner = file
        .declarations
        .iter()
        .find(|entry| {
            let declaration = &entry.declaration;
            declaration.pos == start && declaration.end == end
        })
        .or_else(|| {
            file.declarations
                .iter()
                .max_by_key(|entry| overlap_length(&entry.declaration, start, end))
        });
    let groups = owner
        .into_iter()
        .map(|entry| {
            let declarations = entry
                .symbol
                .as_ref()
                .map(|symbol| symbol.declarations.as_slice())
                .unwrap_or_else(|| std::slice::from_ref(&entry.declaration));
            GroupDump {
                escaped_name: entry
                    .symbol
                    .as_ref()
                    .map(|symbol| normalize_private_symbol_name(&symbol.escaped_name))
                    .unwrap_or_else(|| "<no-symbol>".to_owned()),
                declarations: declarations
                    .iter()
                    .map(|declaration| {
                        region_declaration(declaration, &file.parse_diagnostics, start)
                    })
                    .collect(),
            }
        })
        .collect();
    RegionDump {
        parse_boundaries,
        groups,
    }
}

fn overlap_length(declaration: &DeclarationRef, start: u32, end: u32) -> u32 {
    declaration
        .end
        .min(end)
        .saturating_sub(declaration.pos.max(start))
}

fn normalize_private_symbol_name(name: &str) -> String {
    let Some(rest) = name.strip_prefix("__#") else {
        return name.to_owned();
    };
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || !rest[digits..].starts_with('@') {
        return name.to_owned();
    }
    format!("__#*@{}", &rest[digits + 1..])
}

fn shape_dump(region: &RegionDump) -> ShapeDump {
    let unique_relations = |boundaries: &[BoundaryDiagnostic]| {
        boundaries
            .iter()
            .map(|boundary| shape_boundary_class(boundary.relation))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    ShapeDump {
        parse_boundaries: unique_relations(&region.parse_boundaries),
        groups: region
            .groups
            .iter()
            .map(|group| ShapeGroup {
                name_class: if group.escaped_name == "<no-symbol>" {
                    "missing"
                } else if group.escaped_name.starts_with("__#*@") {
                    "private"
                } else if group.escaped_name.starts_with("__") {
                    "internal"
                } else {
                    "named"
                },
                declarations: group
                    .declarations
                    .iter()
                    .map(|declaration| ShapeDeclaration {
                        kind: declaration.kind,
                        parent_kind: declaration.parent_kind,
                        name_kind: declaration.name.as_ref().map(|name| name.kind),
                        name_missing: declaration.name.as_ref().map(|name| name.missing),
                        body_kind: declaration.body.as_ref().map(|body| body.kind),
                        body_missing: declaration.body.as_ref().map(|body| body.missing),
                        parse_boundaries: unique_relations(&declaration.parse_boundaries),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn shape_fingerprint(
    rust_region: &RegionDump,
    oracle_region: &RegionDump,
) -> Result<String, Box<dyn Error>> {
    let rust_shape = shape_dump(rust_region);
    let oracle_shape = shape_dump(oracle_region);
    let shape = serde_json::to_vec(&(&rust_shape, &oracle_shape))?;
    Ok(sha256_hex(&shape)[..16].to_owned())
}

fn shape_boundary_class(relation: &str) -> &'static str {
    match relation {
        "before" | "after" => "outside",
        "at-start" | "at-end" => "edge",
        _ => "inside",
    }
}

fn region_declaration(
    declaration: &DeclarationRef,
    diagnostics: &[ParseDiagnostic],
    region_start: u32,
) -> RegionDeclaration {
    RegionDeclaration {
        kind: declaration.kind,
        parent_kind: declaration.parent_kind,
        relative_pos: i64::from(declaration.pos) - i64::from(region_start),
        relative_end: i64::from(declaration.end) - i64::from(region_start),
        missing: declaration.missing,
        name: declaration.name.as_ref().map(|name| RegionName {
            kind: name.kind,
            relative_pos: i64::from(name.pos) - i64::from(region_start),
            relative_end: i64::from(name.end) - i64::from(region_start),
            missing: name.missing,
            text: name.text.clone(),
        }),
        body: declaration.body.as_ref().map(|body| RegionNode {
            kind: body.kind,
            relative_pos: i64::from(body.pos) - i64::from(region_start),
            relative_end: i64::from(body.end) - i64::from(region_start),
            missing: body.missing,
        }),
        parse_boundaries: diagnostics
            .iter()
            .map(|diagnostic| BoundaryDiagnostic {
                code: diagnostic.code,
                relative_start: i64::from(diagnostic.start) - i64::from(declaration.pos),
                length: diagnostic.length,
                relation: declaration_boundary(diagnostic.start, declaration),
            })
            .collect(),
    }
}

fn declaration_boundary(position: u32, declaration: &DeclarationRef) -> &'static str {
    if position < declaration.pos {
        "before"
    } else if position == declaration.pos {
        "at-start"
    } else if let Some(name) = &declaration.name {
        if position < name.pos {
            "before-name"
        } else if position < name.end.max(name.pos + 1) {
            "in-name"
        } else if let Some(body) = &declaration.body {
            if position < body.pos {
                "before-body"
            } else if position < body.end.max(body.pos + 1) {
                "in-body"
            } else if position < declaration.end {
                "after-body"
            } else {
                "after"
            }
        } else if position < declaration.end {
            "after-name"
        } else {
            "after"
        }
    } else if position < declaration.end {
        "inside"
    } else {
        "after"
    }
}

fn range_relation(position: u32, start: u32, end: u32) -> &'static str {
    if position < start {
        "before"
    } else if position == start {
        "at-start"
    } else if position < end {
        "inside"
    } else if position == end {
        "at-end"
    } else {
        "after"
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

struct RecoveryOracle {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl RecoveryOracle {
    fn spawn(workspace: &Path) -> Result<Self, Box<dyn Error>> {
        let mut child = Command::new("node")
            .arg(workspace.join("crates/oracle/recovery-dump.mjs"))
            .arg("--server-jsonl")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(Self {
            stdin: child
                .stdin
                .take()
                .ok_or("recovery oracle stdin unavailable")?,
            stdout: BufReader::new(
                child
                    .stdout
                    .take()
                    .ok_or("recovery oracle stdout unavailable")?,
            ),
            child,
            next_id: 1,
        })
    }

    fn dump(&mut self, program_json: &Path) -> Result<RecoveryDump, Box<dyn Error>> {
        let id = self.next_id;
        self.next_id += 1;
        let request = serde_json::json!({
            "id": id,
            "programJsonPath": program_json.display().to_string(),
        });
        writeln!(self.stdin, "{request}")?;
        self.stdin.flush()?;
        let mut line = String::new();
        if self.stdout.read_line(&mut line)? == 0 {
            return Err("recovery oracle exited without a response".into());
        }
        let response: OracleResponse = serde_json::from_str(&line)?;
        if response.id != Some(id) || !response.ok {
            return Err(format!(
                "recovery oracle failure: {}",
                response
                    .error
                    .unwrap_or_else(|| "invalid response".to_owned())
            )
            .into());
        }
        response
            .result
            .ok_or_else(|| "recovery oracle omitted result".into())
    }
}

impl Drop for RecoveryOracle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Deserialize)]
struct OracleResponse {
    id: Option<u64>,
    ok: bool,
    result: Option<RecoveryDump>,
    error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_symbol_ids_are_wildcarded_without_hiding_the_name() {
        assert_eq!(
            normalize_private_symbol_name("__#76@#method"),
            "__#*@#method"
        );
        assert_eq!(normalize_private_symbol_name("ordinary"), "ordinary");
        assert_eq!(normalize_private_symbol_name("__#x@name"), "__#x@name");
    }

    #[test]
    fn shape_boundaries_preserve_inside_edge_outside_classes() {
        assert_eq!(shape_boundary_class("before"), "outside");
        assert_eq!(shape_boundary_class("after"), "outside");
        assert_eq!(shape_boundary_class("at-start"), "edge");
        assert_eq!(shape_boundary_class("at-end"), "edge");
        assert_eq!(shape_boundary_class("before-body"), "inside");
        assert_eq!(shape_boundary_class("in-body"), "inside");
    }
}
