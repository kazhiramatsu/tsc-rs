#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tsrs2_checker::{CompilerOptions, InputFile};
use tsrs2_diags::DiagnosticList;

mod symbol_audit;

fn main() {
    let mut args = std::env::args().skip(1);
    let command = args.next();

    match command.as_deref() {
        None | Some("scaffold-smoke") => scaffold_smoke(),
        Some("expand") => run_or_exit(expand_fixture(args)),
        Some("tokens") => run_or_exit(tokens(args)),
        Some("token-diff") => run_or_exit(token_diff(args)),
        Some("ast-dump") => run_or_exit(ast_dump(args)),
        Some("ast-diff") => run_or_exit(ast_diff(args)),
        Some("symbol-diff") => run_or_exit(symbol_diff(args)),
        Some("parse-diags") => run_or_exit(parse_diags(args)),
        Some("oracle-smoke") => run_or_exit(oracle_smoke(args)),
        Some("oracle-refresh") => run_or_exit(oracle_refresh(args)),
        Some("conformance") => run_or_exit(conformance(args)),
        Some("invariants") => run_or_exit(invariants(args)),
        Some("ledger") => match args.next().as_deref() {
            Some("check") => run_or_exit(ledger_check()),
            Some("coverage") => run_or_exit(ledger_coverage()),
            Some(other) => {
                eprintln!("unknown ledger command: {other}");
                std::process::exit(2);
            }
            None => {
                eprintln!("missing ledger command");
                std::process::exit(2);
            }
        },
        Some("ci") => run_or_exit(ci()),
        Some("codegen") => match args.next().as_deref() {
            Some("diags") => run_or_exit(codegen_diags(false)),
            Some("diags-check") => run_or_exit(codegen_diags(true)),
            Some("nodes") => run_or_exit(codegen_nodes(false)),
            Some("nodes-check") => run_or_exit(codegen_nodes(true)),
            Some("enums") => run_or_exit(codegen_enums(false)),
            Some("enums-check") => run_or_exit(codegen_enums(true)),
            Some("scanner") => run_or_exit(codegen_scanner(false)),
            Some("scanner-check") => run_or_exit(codegen_scanner(true)),
            Some(other) => {
                eprintln!("unknown codegen target: {other}");
                std::process::exit(2);
            }
            None => {
                eprintln!("missing codegen target");
                std::process::exit(2);
            }
        },
        Some(other) => {
            eprintln!("unknown xtask command: {other}");
            std::process::exit(2);
        }
    }
}

fn expand_fixture(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut fixture = None;
    let mut out_dir = None;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out-dir" => {
                let value = args.next().ok_or("missing value after --out-dir")?;
                out_dir = Some(PathBuf::from(value));
            }
            _ if fixture.is_none() => fixture = Some(PathBuf::from(arg)),
            _ => return Err(format!("unexpected expand argument: {arg}").into()),
        }
    }

    let fixture = fixture.ok_or("missing fixture path for expand")?;
    let out_dir = out_dir.ok_or("missing --out-dir for expand")?;
    let workspace = find_tsrs2_root()?;
    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");
    let programs = tsrs2_harness::expand_fixture_file(&fixture, &vendor_lib_dir)?;
    let paths = tsrs2_harness::write_program_jsons(&programs, &out_dir)?;

    for path in paths {
        println!("{}", path.display());
    }

    Ok(())
}

fn tokens(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let path = parse_single_path_arg("tokens", args)?;
    print!("{}", rust_token_dump(&path)?);
    Ok(())
}

struct TokenDiffArgs {
    corpus: bool,
    files: Vec<PathBuf>,
    limit: Option<usize>,
}

fn token_diff(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let args = parse_token_diff_args(args)?;
    let workspace = find_tsrs2_root()?;
    let mut files = if args.corpus {
        collect_fixture_paths(&workspace.join("ts-tests/tests/cases/conformance"))?
    } else {
        args.files
    };
    if files.is_empty() {
        return Err("token-diff requires --corpus, --files, or a file path".into());
    }
    files.sort();
    if let Some(limit) = args.limit {
        files.truncate(limit);
    }

    let mut oracle = TokenDumpOracle::spawn(&workspace)?;
    let mut differing = 0usize;
    for file in &files {
        let text = fs::read_to_string(file)?;
        let variant = language_variant_for_path(file);
        let rust = rust_token_dump_text(&text, variant);
        let oracle_dump = oracle.token_dump(file, &text, language_variant_arg(file))?;
        if rust != oracle_dump {
            differing += 1;
            if differing <= 10 {
                let (line, left, right) = first_diff(&rust, &oracle_dump);
                println!(
                    "diff {} line {}:\n  tsrs:   {}\n  oracle: {}",
                    file.display(),
                    line,
                    left.unwrap_or("<missing>"),
                    right.unwrap_or("<missing>")
                );
            }
        }
    }

    if differing > 0 {
        return Err(format!(
            "token diff failed: {differing}/{} files differ",
            files.len()
        )
        .into());
    }
    println!("token diff ok: files={}", files.len());
    Ok(())
}

fn parse_single_path_arg(
    command: &str,
    args: impl Iterator<Item = String>,
) -> Result<PathBuf, Box<dyn Error>> {
    let mut path = None;
    for arg in args {
        if path.is_none() {
            path = Some(PathBuf::from(arg));
        } else {
            return Err(format!("unexpected {command} argument: {arg}").into());
        }
    }
    path.ok_or_else(|| format!("missing file path for {command}").into())
}

fn parse_token_diff_args(
    args: impl Iterator<Item = String>,
) -> Result<TokenDiffArgs, Box<dyn Error>> {
    let mut corpus = false;
    let mut files = Vec::new();
    let mut limit = None;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--corpus" => corpus = true,
            "--files" => {
                let value = args.next().ok_or("missing value after --files")?;
                files.extend(
                    value
                        .split(',')
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(PathBuf::from),
                );
            }
            "--limit" => {
                let value = args.next().ok_or("missing value after --limit")?;
                limit = Some(value.parse()?);
            }
            _ => files.push(PathBuf::from(arg)),
        }
    }

    Ok(TokenDiffArgs {
        corpus,
        files,
        limit,
    })
}

fn rust_token_dump(path: &Path) -> Result<String, Box<dyn Error>> {
    let text = fs::read_to_string(path)?;
    let variant = language_variant_for_path(path);
    Ok(rust_token_dump_text(&text, variant))
}

fn rust_token_dump_text(text: &str, variant: tsrs2_syntax::LanguageVariant) -> String {
    let mut out = String::new();
    for token in tsrs2_syntax::scan_tokens(text, variant) {
        let _ = writeln!(
            out,
            "{}\t{}\t{}\t{}",
            token.kind as u16,
            token.start,
            token.end,
            u8::from(token.preceding_line_break)
        );
    }
    out
}

#[derive(Debug, Serialize)]
struct TokenDumpRequest<'text> {
    id: u64,
    payload: TokenDumpPayload<'text>,
}

#[derive(Debug, Serialize)]
struct TokenDumpPayload<'text> {
    #[serde(rename = "textBase64")]
    text_base64: &'text str,
    variant: &'static str,
}

#[derive(Debug, Deserialize)]
struct TokenDumpResponse {
    id: Option<u64>,
    ok: bool,
    result: Option<String>,
    error: Option<String>,
}

fn parse_diags(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let path = parse_single_path_arg("parse-diags", args)?;
    let text = fs::read_to_string(&path)?;
    let file_name = path.to_string_lossy();
    let source = if file_name.ends_with(".json") {
        tsrs2_syntax::parse_json_text(file_name.to_string(), text)
    } else {
        tsrs2_syntax::parse_source_file(
            file_name.to_string(),
            text,
            tsrs2_syntax::ParseOptions {
                language_variant: language_variant_for_path(&path),
                // ast-dump.mjs uses ScriptKind TS/TSX, never JS.
                javascript_file: false,
            },
            None,
        )
    };
    for diagnostic in &source.parse_diagnostics {
        println!(
            "{} start={} len={} :: {}",
            diagnostic.code(),
            diagnostic.start.unwrap_or(0),
            diagnostic.length.unwrap_or(0),
            diagnostic.message.text
        );
    }
    Ok(())
}

fn ast_dump(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let path = parse_single_path_arg("ast-dump", args)?;
    let text = fs::read_to_string(&path)?;
    let (dump, parse_errors) = rust_ast_dump_text(&path.to_string_lossy(), &text);
    print!("{dump}");
    if parse_errors > 0 {
        eprintln!("parse errors: {parse_errors}");
    }
    Ok(())
}

/// impl-nodes.md §5: the (kind, pos-utf16, end-utf16) indented pre-order tree
/// via the generated for_each_child, plus the parse-error count that gates
/// tree comparison.
fn rust_ast_dump_text(file_name: &str, text: &str) -> (String, usize) {
    let variant = language_variant_for_path(Path::new(file_name));
    let source = tsrs2_syntax::parse_source_file(
        file_name,
        text,
        tsrs2_syntax::ParseOptions {
            language_variant: variant,
            // ast-dump.mjs uses ScriptKind TS/TSX, never JS.
            javascript_file: false,
        },
        None,
    );
    let map = tsrs2_diags::compute_line_map(text);
    let to_utf16 =
        |pos: u32| -> u32 { map.byte_to_utf16.get(pos as usize).copied().unwrap_or(pos) };

    let mut out = String::new();
    let mut stack = vec![(source.root, 0usize)];
    while let Some((id, depth)) = stack.pop() {
        let node = source.arena.node(id);
        let _ = writeln!(
            out,
            "{}{} {} {}",
            "  ".repeat(depth),
            node.kind as u16,
            to_utf16(node.pos),
            to_utf16(node.end)
        );
        let mut children = Vec::new();
        tsrs2_syntax::for_each_child(&source.arena, node, |child| {
            children.push(child);
            false
        });
        for child in children.into_iter().rev() {
            stack.push((child, depth + 1));
        }
    }
    (out, source.parse_diagnostics.len())
}

fn ast_diff(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let args = parse_token_diff_args(args)?;
    let workspace = find_tsrs2_root()?;
    let mut files = if args.corpus {
        collect_fixture_paths(&workspace.join("ts-tests/tests/cases/conformance"))?
    } else {
        args.files
    };
    if files.is_empty() {
        return Err("ast-diff requires --corpus, --files, or a file path".into());
    }
    files.sort();
    if let Some(limit) = args.limit {
        files.truncate(limit);
    }

    let mut oracle = AstDumpOracle::spawn(&workspace)?;
    let mut compared = 0usize;
    let mut excluded = 0usize;
    let mut differing = 0usize;
    let mut failures = String::new();
    for file in &files {
        let text = fs::read_to_string(file)?;
        let file_name = file.to_string_lossy();
        let (rust_dump, rust_parse_errors) = rust_ast_dump_text(&file_name, &text);
        let oracle_result = oracle.ast_dump(file, &text, &file_name)?;
        // Error-recovery trees may legitimately differ in Missing-node
        // placement; error fixtures are covered by the diagnostic gate.
        if rust_parse_errors > 0 || oracle_result.parse_errors > 0 {
            excluded += 1;
            continue;
        }
        compared += 1;
        if rust_dump != oracle_result.dump {
            differing += 1;
            let (line, left, right) = first_diff(&rust_dump, &oracle_result.dump);
            let entry = format!(
                "diff {} line {}:\n  tsrs:   {}\n  oracle: {}",
                file.display(),
                line,
                left.unwrap_or("<missing>"),
                right.unwrap_or("<missing>")
            );
            if differing <= 10 {
                println!("{entry}");
            }
            failures.push_str(&entry);
            failures.push('\n');
        }
    }

    let failures_path = workspace.join("target/ast-diff-failures.txt");
    if let Some(parent) = failures_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&failures_path, &failures)?;

    println!(
        "ast diff: files={} compared={} excluded={} differing={}",
        files.len(),
        compared,
        excluded,
        differing
    );
    println!("failures: {}", failures_path.display());
    if differing > 0 {
        return Err(
            format!("ast diff failed: {differing}/{compared} compared files differ").into(),
        );
    }
    Ok(())
}

struct AstDumpOracle {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl AstDumpOracle {
    fn spawn(workspace: &Path) -> Result<Self, Box<dyn Error>> {
        let mut child = Command::new("node")
            .arg(workspace.join("crates/oracle/ast-dump.mjs"))
            .arg("--server-jsonl")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or("ast dump oracle stdin unavailable")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("ast dump oracle stdout unavailable")?;

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        })
    }

    fn ast_dump(
        &mut self,
        path: &Path,
        text: &str,
        file_name: &str,
    ) -> Result<AstDumpResult, Box<dyn Error>> {
        let id = self.next_id;
        self.next_id += 1;
        let text_base64 = BASE64.encode(text);
        let request = serde_json::to_string(&AstDumpRequest {
            id,
            payload: AstDumpPayload {
                text_base64: &text_base64,
                file_name,
            },
        })?;
        writeln!(self.stdin, "{request}")?;
        self.stdin.flush()?;

        let mut line = String::new();
        let read = self.stdout.read_line(&mut line)?;
        if read == 0 {
            return Err(format!(
                "oracle ast dump worker exited without a response for {}",
                path.display()
            )
            .into());
        }

        let response: AstDumpResponse = serde_json::from_str(&line)?;
        if response.id != Some(id) {
            return Err(format!(
                "oracle ast dump response id mismatch for {}: expected {id}, got {}{}",
                path.display(),
                response
                    .id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "null".to_owned()),
                response
                    .error
                    .as_deref()
                    .map(|error| format!(": {error}"))
                    .unwrap_or_default()
            )
            .into());
        }
        if !response.ok {
            return Err(format!(
                "oracle ast dump failed for {}: {}",
                path.display(),
                response.error.unwrap_or_else(|| "unknown error".to_owned())
            )
            .into());
        }
        response.result.ok_or_else(|| {
            format!(
                "oracle ast dump response missing result for {}",
                path.display()
            )
            .into()
        })
    }
}

impl Drop for AstDumpOracle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Serialize)]
struct AstDumpRequest<'text> {
    id: u64,
    payload: AstDumpPayload<'text>,
}

#[derive(Debug, Serialize)]
struct AstDumpPayload<'text> {
    #[serde(rename = "textBase64")]
    text_base64: &'text str,
    #[serde(rename = "fileName")]
    file_name: &'text str,
}

#[derive(Debug, Deserialize)]
struct AstDumpResponse {
    id: Option<u64>,
    ok: bool,
    result: Option<AstDumpResult>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AstDumpResult {
    dump: String,
    #[serde(rename = "parseErrors")]
    parse_errors: usize,
}

/// m2-binder-steps.md stage 3.0: compare the Rust symbol audit against
/// oracle symbol-dump.mjs, program.json by program.json. The audit is a
/// TS-only SPOT check: .js/.jsx/.json program files are skipped (the JS
/// special-assignment symbol bodies land in stage 3.4), and files with
/// parse errors on either side are excluded like ast-diff.
fn symbol_diff(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut fixtures: Vec<PathBuf> = Vec::new();
    let mut sample: Option<usize> = None;
    let mut limit: Option<usize> = None;
    let mut positions_only = false;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--sample" => {
                let value = args.next().ok_or("missing value after --sample")?;
                sample = Some(value.parse()?);
            }
            "--limit" => {
                let value = args.next().ok_or("missing value after --limit")?;
                limit = Some(value.parse()?);
            }
            // Walk-parity mode: compare only the pos/end columns, so the
            // audit WALK mirror is verifiable before the binder exists.
            "--positions-only" => positions_only = true,
            _ => fixtures.push(PathBuf::from(arg)),
        }
    }

    let workspace = find_tsrs2_root()?;
    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");
    if let Some(sample) = sample {
        if !fixtures.is_empty() {
            return Err("--sample and explicit fixture paths are mutually exclusive".into());
        }
        let mut corpus =
            collect_fixture_paths(&workspace.join("ts-tests/tests/cases/conformance"))?;
        corpus.sort();
        // Deterministic stride sample across the sorted corpus.
        let count = sample.min(corpus.len());
        for index in 0..count {
            fixtures.push(corpus[index * corpus.len() / count].clone());
        }
    }
    if fixtures.is_empty() {
        return Err("symbol-diff requires fixture paths or --sample N".into());
    }
    fixtures.sort();
    if let Some(limit) = limit {
        fixtures.truncate(limit);
    }

    let temp_root = std::env::temp_dir().join(format!("tsrs2-symbol-diff-{}", std::process::id()));
    if temp_root.exists() {
        fs::remove_dir_all(&temp_root)?;
    }
    fs::create_dir_all(&temp_root)?;

    let mut oracle = SymbolDumpOracle::spawn(&workspace)?;
    let mut programs = 0usize;
    let mut compared = 0usize;
    let mut excluded = 0usize;
    let mut skipped_non_ts = 0usize;
    let mut differing = 0usize;
    let mut failures = String::new();

    for (fixture_index, fixture) in fixtures.iter().enumerate() {
        let expanded = tsrs2_harness::expand_fixture_file(fixture, &vendor_lib_dir)?;
        let out_dir = temp_root.join(fixture_index.to_string());
        let paths = tsrs2_harness::write_program_jsons(&expanded, &out_dir)?;
        for (program, path) in expanded.iter().zip(&paths) {
            programs += 1;
            let oracle_files = oracle.symbol_dump(path)?;
            let rust_files = rust_symbol_dump(program)?;
            if oracle_files.len() != rust_files.len() {
                return Err(format!(
                    "symbol dump file-count mismatch for {}: oracle {} vs tsrs {}",
                    path.display(),
                    oracle_files.len(),
                    rust_files.len()
                )
                .into());
            }
            for (oracle_file, rust_file) in oracle_files.iter().zip(&rust_files) {
                let Some(rust_file) = rust_file else {
                    skipped_non_ts += 1;
                    continue;
                };
                if oracle_file.parse_errors > 0 || rust_file.parse_errors > 0 {
                    excluded += 1;
                    continue;
                }
                compared += 1;
                let project = |lines: &[String]| -> String {
                    if positions_only {
                        lines
                            .iter()
                            .map(|line| {
                                line.splitn(3, '\t').take(2).collect::<Vec<_>>().join("\t")
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    } else {
                        lines.join("\n")
                    }
                };
                let oracle_dump = project(&oracle_file.lines);
                let rust_dump = project(&rust_file.lines);
                if !oracle_file.in_program || oracle_dump != rust_dump {
                    differing += 1;
                    let (line, left, right) = first_diff(&rust_dump, &oracle_dump);
                    let entry = format!(
                        "diff {} [{}] {} line {}:\n  tsrs:   {}\n  oracle: {}",
                        fixture.display(),
                        program.matrix_key,
                        rust_file.name,
                        line,
                        left.unwrap_or("<missing>"),
                        if oracle_file.in_program {
                            right.unwrap_or("<missing>")
                        } else {
                            "<file not in oracle program>"
                        }
                    );
                    if differing <= 10 {
                        println!("{entry}");
                    }
                    failures.push_str(&entry);
                    failures.push('\n');
                }
            }
        }
    }

    fs::remove_dir_all(&temp_root)?;
    let failures_path = workspace.join("target/symbol-diff-failures.txt");
    if let Some(parent) = failures_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&failures_path, &failures)?;

    println!(
        "symbol diff: fixtures={} programs={} compared={} excluded={} skipped-non-ts={} differing={}",
        fixtures.len(),
        programs,
        compared,
        excluded,
        skipped_non_ts,
        differing
    );
    println!("failures: {}", failures_path.display());
    if differing > 0 {
        return Err(
            format!("symbol diff failed: {differing}/{compared} compared files differ").into(),
        );
    }
    Ok(())
}

fn rust_symbol_dump(
    program: &tsrs2_harness::ProgramJson,
) -> Result<Vec<Option<symbol_audit::FileAudit>>, Box<dyn Error>> {
    // tsc host semantics: files are a name-keyed map, so a later file with
    // the same name shadows an earlier one entirely.
    let mut last_text_b64: BTreeMap<&str, &str> = BTreeMap::new();
    for file in &program.files {
        last_text_b64.insert(file.name.as_str(), file.text_b64.as_str());
    }

    let mut out = Vec::with_capacity(program.files.len());
    for file in &program.files {
        if !is_ts_like_file_name(&file.name) {
            out.push(None);
            continue;
        }
        let bytes = BASE64.decode(last_text_b64[file.name.as_str()])?;
        let text = String::from_utf8(bytes)?;
        let language_variant = if file.name.ends_with(".tsx") {
            tsrs2_syntax::LanguageVariant::Jsx
        } else {
            tsrs2_syntax::LanguageVariant::Standard
        };
        let source = tsrs2_syntax::parse_source_file(
            file.name.clone(),
            text,
            tsrs2_syntax::ParseOptions {
                language_variant,
                javascript_file: false,
            },
            None,
        );
        out.push(Some(symbol_audit::FileAudit {
            name: file.name.clone(),
            parse_errors: source.parse_diagnostics.len(),
            lines: symbol_audit::audit_source_file(&source),
        }));
    }
    Ok(out)
}

/// TS-only audit carve-out (m2-binder-steps.md stage 3.4): .js and .json
/// program files stay out of the audit until the JS special-assignment
/// symbol bodies land.
fn is_ts_like_file_name(name: &str) -> bool {
    [".ts", ".tsx", ".mts", ".cts"]
        .iter()
        .any(|extension| name.ends_with(extension))
}

struct SymbolDumpOracle {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl SymbolDumpOracle {
    fn spawn(workspace: &Path) -> Result<Self, Box<dyn Error>> {
        let mut child = Command::new("node")
            .arg(workspace.join("crates/oracle/symbol-dump.mjs"))
            .arg("--server-jsonl")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or("symbol dump oracle stdin unavailable")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("symbol dump oracle stdout unavailable")?;

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        })
    }

    fn symbol_dump(
        &mut self,
        program_json: &Path,
    ) -> Result<Vec<OracleFileAudit>, Box<dyn Error>> {
        let id = self.next_id;
        self.next_id += 1;
        let request = serde_json::to_string(&SymbolDumpRequest {
            id,
            program_json_path: &program_json.display().to_string(),
        })?;
        writeln!(self.stdin, "{request}")?;
        self.stdin.flush()?;

        let mut line = String::new();
        let read = self.stdout.read_line(&mut line)?;
        if read == 0 {
            return Err(format!(
                "oracle symbol dump worker exited without a response for {}",
                program_json.display()
            )
            .into());
        }

        let response: SymbolDumpResponse = serde_json::from_str(&line)?;
        if response.id != Some(id) {
            return Err(format!(
                "oracle symbol dump response id mismatch for {}: expected {id}, got {}{}",
                program_json.display(),
                response
                    .id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "null".to_owned()),
                response
                    .error
                    .as_deref()
                    .map(|error| format!(": {error}"))
                    .unwrap_or_default()
            )
            .into());
        }
        if !response.ok {
            return Err(format!(
                "oracle symbol dump failed for {}: {}",
                program_json.display(),
                response.error.unwrap_or_else(|| "unknown error".to_owned())
            )
            .into());
        }
        let result = response.result.ok_or_else(|| {
            format!(
                "oracle symbol dump response missing result for {}",
                program_json.display()
            )
        })?;
        Ok(result.files)
    }
}

impl Drop for SymbolDumpOracle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Serialize)]
struct SymbolDumpRequest<'path> {
    id: u64,
    #[serde(rename = "programJsonPath")]
    program_json_path: &'path str,
}

#[derive(Debug, Deserialize)]
struct SymbolDumpResponse {
    id: Option<u64>,
    ok: bool,
    result: Option<SymbolDumpResult>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SymbolDumpResult {
    files: Vec<OracleFileAudit>,
}

#[derive(Debug, Deserialize)]
struct OracleFileAudit {
    #[allow(dead_code)]
    name: String,
    #[serde(rename = "inProgram")]
    in_program: bool,
    #[serde(rename = "parseErrors")]
    parse_errors: usize,
    lines: Vec<String>,
}

struct TokenDumpOracle {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl TokenDumpOracle {
    fn spawn(workspace: &Path) -> Result<Self, Box<dyn Error>> {
        let mut child = Command::new("node")
            .arg(workspace.join("crates/oracle/token-dump.mjs"))
            .arg("--server-jsonl")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or("token dump oracle stdin unavailable")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("token dump oracle stdout unavailable")?;

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        })
    }

    fn token_dump(
        &mut self,
        path: &Path,
        text: &str,
        variant: &'static str,
    ) -> Result<String, Box<dyn Error>> {
        let id = self.next_id;
        self.next_id += 1;
        let text_base64 = BASE64.encode(text);
        let request = serde_json::to_string(&TokenDumpRequest {
            id,
            payload: TokenDumpPayload {
                text_base64: &text_base64,
                variant,
            },
        })?;
        writeln!(self.stdin, "{request}")?;
        self.stdin.flush()?;

        let mut line = String::new();
        let read = self.stdout.read_line(&mut line)?;
        if read == 0 {
            return Err(format!(
                "oracle token dump worker exited without a response for {}",
                path.display()
            )
            .into());
        }

        let response: TokenDumpResponse = serde_json::from_str(&line)?;
        if response.id != Some(id) {
            return Err(format!(
                "oracle token dump response id mismatch for {}: expected {id}, got {}{}",
                path.display(),
                response
                    .id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "null".to_owned()),
                response
                    .error
                    .as_deref()
                    .map(|error| format!(": {error}"))
                    .unwrap_or_default()
            )
            .into());
        }
        if !response.ok {
            return Err(format!(
                "oracle token dump failed for {}: {}",
                path.display(),
                response.error.unwrap_or_else(|| "unknown error".to_owned())
            )
            .into());
        }
        response.result.ok_or_else(|| {
            format!(
                "oracle token dump response missing result for {}",
                path.display()
            )
            .into()
        })
    }
}

impl Drop for TokenDumpOracle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn language_variant_for_path(path: &Path) -> tsrs2_syntax::LanguageVariant {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("tsx" | "jsx") => tsrs2_syntax::LanguageVariant::Jsx,
        _ => tsrs2_syntax::LanguageVariant::Standard,
    }
}

fn language_variant_arg(path: &Path) -> &'static str {
    match language_variant_for_path(path) {
        tsrs2_syntax::LanguageVariant::Standard => "standard",
        tsrs2_syntax::LanguageVariant::Jsx => "jsx",
    }
}

fn first_diff<'a>(left: &'a str, right: &'a str) -> (usize, Option<&'a str>, Option<&'a str>) {
    let mut left_lines = left.lines();
    let mut right_lines = right.lines();
    for line_number in 1.. {
        let left = left_lines.next();
        let right = right_lines.next();
        if left != right {
            return (line_number, left, right);
        }
        if left.is_none() && right.is_none() {
            return (line_number, None, None);
        }
    }
    unreachable!("unbounded line iterator returns from inside the loop")
}

fn oracle_refresh(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let parsed = parse_conformance_args(args)?;
    let workspace = find_tsrs2_root()?;
    let summary = tsrs2_conformance::refresh_oracle_goldens(&tsrs2_conformance::RefreshOptions {
        workspace,
        limit: parsed.limit,
        files: parsed.files,
    })?;
    println!(
        "oracle refresh wrote {} fixtures / {} cases / {} oracle diagnostics under {}",
        summary.fixtures, summary.cases, summary.oracle_diagnostics, summary.goldens_root
    );
    Ok(())
}

fn conformance(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let parsed = parse_conformance_args(args)?;
    let workspace = find_tsrs2_root()?;
    let out_json = parsed
        .out_json
        .unwrap_or_else(|| workspace.join("target/conformance/mismatches.json"));
    let summary = tsrs2_conformance::run_conformance(&tsrs2_conformance::ConformanceOptions {
        workspace,
        limit: parsed.limit,
        files: parsed.files,
        out_json: out_json.clone(),
        band: parsed.band,
    })?;
    println!(
        "conformance band={} fixtures={} cases={} T0={:.4}% matched={}/{} FP={} FN={} mismatches={}",
        summary.band,
        summary.fixtures_total,
        summary.cases_total,
        summary.t0_rate * 100.0,
        summary.matched_t0_diagnostics,
        summary.oracle_diagnostics,
        summary.false_positive_diagnostics,
        summary.false_negative_diagnostics,
        summary.mismatch_cases
    );
    println!("mismatch json: {}", out_json.display());
    Ok(())
}

struct ConformanceArgs {
    limit: Option<usize>,
    files: Vec<PathBuf>,
    out_json: Option<PathBuf>,
    band: tsrs2_conformance::DiagnosticBand,
}

fn parse_conformance_args(
    args: impl Iterator<Item = String>,
) -> Result<ConformanceArgs, Box<dyn Error>> {
    let mut limit = None;
    let mut files = Vec::new();
    let mut out_json = None;
    let mut band = tsrs2_conformance::DiagnosticBand::All;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--limit" => {
                let value = args.next().ok_or("missing value after --limit")?;
                limit = Some(value.parse()?);
            }
            "--files" => {
                let value = args.next().ok_or("missing value after --files")?;
                files.extend(
                    value
                        .split(',')
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(PathBuf::from),
                );
            }
            "--out-json" => {
                let value = args.next().ok_or("missing value after --out-json")?;
                out_json = Some(PathBuf::from(value));
            }
            "--band" => {
                let value = args.next().ok_or("missing value after --band")?;
                band = match value.as_str() {
                    "all" => tsrs2_conformance::DiagnosticBand::All,
                    "2xxx" => tsrs2_conformance::DiagnosticBand::TwoXxx,
                    "syntactic" => tsrs2_conformance::DiagnosticBand::Syntactic,
                    _ => return Err(format!("unknown conformance band: {value}").into()),
                };
            }
            "--syntactic-only" => band = tsrs2_conformance::DiagnosticBand::Syntactic,
            _ => return Err(format!("unexpected conformance argument: {arg}").into()),
        }
    }

    Ok(ConformanceArgs {
        limit,
        files,
        out_json,
        band,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InvariantSuite {
    All,
    PrefixDeterminism,
    PrefixConformance,
    Idempotence,
    JobsIndependence,
    Encodings,
    MatrixIndependence,
}

impl InvariantSuite {
    fn parse(value: &str) -> Result<Self, Box<dyn Error>> {
        match value {
            "all" => Ok(Self::All),
            "prefix-determinism" => Ok(Self::PrefixDeterminism),
            "prefix-conformance" => Ok(Self::PrefixConformance),
            "idempotence" => Ok(Self::Idempotence),
            "jobs-independence" => Ok(Self::JobsIndependence),
            "encodings" => Ok(Self::Encodings),
            "matrix-independence" => Ok(Self::MatrixIndependence),
            _ => Err(format!("unknown invariant suite: {value}").into()),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::PrefixDeterminism => "prefix-determinism",
            Self::PrefixConformance => "prefix-conformance",
            Self::Idempotence => "idempotence",
            Self::JobsIndependence => "jobs-independence",
            Self::Encodings => "encodings",
            Self::MatrixIndependence => "matrix-independence",
        }
    }

    fn includes(self, suite: Self) -> bool {
        // prefix-conformance needs the node oracle; it never rides `all`.
        if suite == Self::PrefixConformance {
            return self == Self::PrefixConformance;
        }
        self == Self::All || self == suite
    }
}

struct InvariantArgs {
    suite: InvariantSuite,
    limit: usize,
}

#[derive(Clone, Debug)]
struct SampleProgram {
    fixture: String,
    matrix_key: String,
    files: Vec<InputFile>,
}

fn invariants(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let args = parse_invariant_args(args)?;
    let workspace = find_tsrs2_root()?;
    let programs = load_sample_programs(&workspace, args.limit)?;
    let fixture_count = programs
        .iter()
        .map(|program| program.fixture.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len();

    if args.suite.includes(InvariantSuite::PrefixDeterminism) {
        run_prefix_determinism(&programs)?;
        println!(
            "invariant prefix-determinism ok: programs={}",
            programs.len()
        );
    }
    if args.suite.includes(InvariantSuite::PrefixConformance) {
        let summary = tsrs2_conformance::run_prefix_conformance(
            &tsrs2_conformance::PrefixConformanceOptions {
                workspace: workspace.clone(),
                limit: Some(args.limit),
                files: Vec::new(),
            },
        )?;
        for mismatch in summary.mismatches.iter().take(10) {
            println!(
                "prefix-conformance mismatch: {} [{}] file {} cut {} FP={:?} FN={:?}",
                mismatch.fixture,
                mismatch.matrix_key,
                mismatch.file,
                mismatch.cut,
                mismatch.false_positive,
                mismatch.false_negative
            );
        }
        if summary.mismatched_cases > 0 {
            return Err(format!(
                "prefix-conformance failed: {}/{} truncated cases diverge from the oracle",
                summary.mismatched_cases, summary.cases
            )
            .into());
        }
        println!(
            "invariant prefix-conformance ok: fixtures={} cases={}",
            summary.fixtures, summary.cases
        );
    }
    if args.suite.includes(InvariantSuite::Idempotence) {
        run_idempotence(&programs)?;
        println!("invariant idempotence ok: programs={}", programs.len());
    }
    if args.suite.includes(InvariantSuite::JobsIndependence) {
        run_jobs_independence(&programs)?;
        println!(
            "invariant jobs-independence ok: programs={}",
            programs.len()
        );
    }
    if args.suite.includes(InvariantSuite::Encodings) {
        run_encodings(&programs)?;
        println!("invariant encodings ok: programs={}", programs.len());
    }
    if args.suite.includes(InvariantSuite::MatrixIndependence) {
        run_matrix_independence(&programs)?;
        println!(
            "invariant matrix-independence ok: programs={}",
            programs.len()
        );
    }

    println!(
        "invariants suite={} fixtures={} programs={} ok",
        args.suite.name(),
        fixture_count,
        programs.len()
    );
    Ok(())
}

fn parse_invariant_args(
    args: impl Iterator<Item = String>,
) -> Result<InvariantArgs, Box<dyn Error>> {
    let mut suite = InvariantSuite::All;
    let mut limit = 200usize;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--suite" => {
                let value = args.next().ok_or("missing value after --suite")?;
                suite = InvariantSuite::parse(&value)?;
            }
            "--limit" => {
                let value = args.next().ok_or("missing value after --limit")?;
                limit = value.parse()?;
            }
            _ => return Err(format!("unexpected invariants argument: {arg}").into()),
        }
    }

    Ok(InvariantArgs { suite, limit })
}

fn load_sample_programs(
    workspace: &Path,
    limit: usize,
) -> Result<Vec<SampleProgram>, Box<dyn Error>> {
    let fixtures_root = workspace.join("ts-tests/tests/cases/conformance");
    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");
    let mut fixtures = collect_fixture_paths(&fixtures_root)?;
    fixtures.sort();
    fixtures.truncate(limit);

    let mut programs = Vec::new();
    for fixture in fixtures {
        let fixture_key = fixture
            .strip_prefix(&fixtures_root)?
            .to_string_lossy()
            .replace('\\', "/");
        for program in tsrs2_harness::expand_fixture_file(&fixture, &vendor_lib_dir)? {
            let files = program
                .files
                .into_iter()
                .map(|file| {
                    Ok(InputFile {
                        name: file.name,
                        text: base64_decode_to_string(&file.text_b64)?,
                    })
                })
                .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
            programs.push(SampleProgram {
                fixture: fixture_key.clone(),
                matrix_key: program.matrix_key,
                files,
            });
        }
    }

    Ok(programs)
}

fn midpoint_char_boundary(text: &str) -> usize {
    let midpoint = text.len() / 2;
    text.char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= midpoint)
        .last()
        .unwrap_or(0)
}

/// greenfield §7.6 prefix-determinism, reformulated at the TOKEN level:
/// scanning a truncated file yields the same tokens strictly before the cut
/// as scanning the full file. The original diagnostic-level formulation is
/// unsatisfiable for a tsc-faithful parser — recovery legitimately attributes
/// errors before the cut depending on later text, and tsc itself does
/// (counterexample in docs/NOTES-m1.md). Diagnostic-level fidelity on
/// truncated inputs is covered by the oracle-backed `prefix-conformance`
/// suite instead.
fn run_prefix_determinism(programs: &[SampleProgram]) -> Result<(), Box<dyn Error>> {
    for program in programs {
        for file in &program.files {
            let cut = midpoint_char_boundary(&file.text);
            let variant = language_variant_for_path(Path::new(&file.name));
            let full = tsrs2_syntax::scan_tokens(&file.text, variant);
            let prefix = tsrs2_syntax::scan_tokens(&file.text[..cut], variant);
            // Tokens touching the cut are inherently ambiguous (they may be
            // truncated or merge with later text); everything strictly
            // before it must be byte-identical.
            let full_before = full.iter().filter(|token| (token.end as usize) < cut);
            let prefix_before = prefix.iter().filter(|token| (token.end as usize) < cut);
            if !full_before.eq(prefix_before) {
                return Err(format!(
                    "prefix-determinism failed for {} [{}] file {} (cut {})",
                    program.fixture, program.matrix_key, file.name, cut
                )
                .into());
            }
        }
    }
    Ok(())
}

fn run_idempotence(programs: &[SampleProgram]) -> Result<(), Box<dyn Error>> {
    for program in programs {
        let first = check_bytes(&program.files);
        let second = check_bytes(&program.files);
        if first != second {
            return Err(format!(
                "idempotence failed for {} [{}]",
                program.fixture, program.matrix_key
            )
            .into());
        }
    }
    Ok(())
}

fn run_jobs_independence(programs: &[SampleProgram]) -> Result<(), Box<dyn Error>> {
    let baseline = run_programs_in_job_order(programs, 1);
    for jobs in 2..=16 {
        let candidate = run_programs_in_job_order(programs, jobs);
        if baseline != candidate {
            return Err(format!("jobs-independence failed for jobs={jobs}").into());
        }
    }
    Ok(())
}

fn run_encodings(programs: &[SampleProgram]) -> Result<(), Box<dyn Error>> {
    for program in programs {
        let baseline = diagnostic_semantic_bytes(&check_diagnostics(&program.files));
        for file_index in 0..program.files.len() {
            let original = &program.files[file_index].text;
            let variants = [
                original.trim_start_matches('\u{feff}').to_owned(),
                format!("\u{feff}{}", original.trim_start_matches('\u{feff}')),
                original.replace("\r\n", "\n"),
                original.replace('\n', "\r\n"),
            ];
            for variant in variants {
                let mut files = program.files.clone();
                files[file_index].text = variant;
                let candidate = diagnostic_semantic_bytes(&check_diagnostics(&files));
                if baseline != candidate {
                    return Err(format!(
                        "encodings failed for {} [{}] file {}",
                        program.fixture, program.matrix_key, files[file_index].name
                    )
                    .into());
                }
            }
        }
    }
    Ok(())
}

fn run_matrix_independence(programs: &[SampleProgram]) -> Result<(), Box<dyn Error>> {
    let mut by_fixture = BTreeMap::<&str, Vec<&SampleProgram>>::new();
    for program in programs {
        by_fixture
            .entry(&program.fixture)
            .or_default()
            .push(program);
    }

    for (fixture, fixture_programs) in by_fixture {
        if fixture_programs.len() < 2 {
            continue;
        }
        let forward = fixture_programs
            .iter()
            .map(|program| (program_key(program), check_bytes(&program.files)))
            .collect::<BTreeMap<_, _>>();
        let reverse = fixture_programs
            .iter()
            .rev()
            .map(|program| (program_key(program), check_bytes(&program.files)))
            .collect::<BTreeMap<_, _>>();
        if forward != reverse {
            return Err(format!("matrix-independence failed for {fixture}").into());
        }
    }
    Ok(())
}

fn run_programs_in_job_order(programs: &[SampleProgram], jobs: usize) -> BTreeMap<String, String> {
    let mut output = BTreeMap::new();
    for job in 0..jobs {
        for (index, program) in programs.iter().enumerate() {
            if index % jobs == job {
                output.insert(program_key(program), check_bytes(&program.files));
            }
        }
    }
    output
}

fn program_key(program: &SampleProgram) -> String {
    if program.matrix_key.is_empty() {
        program.fixture.clone()
    } else {
        format!("{}#{}", program.fixture, program.matrix_key)
    }
}

fn check_diagnostics(files: &[InputFile]) -> DiagnosticList {
    tsrs2_checker::check_program(files, &CompilerOptions::default()).diagnostics
}

fn check_bytes(files: &[InputFile]) -> String {
    diagnostic_bytes(&check_diagnostics(files))
}

fn diagnostic_bytes(diagnostics: &DiagnosticList) -> String {
    format!("{diagnostics:#?}")
}

fn diagnostic_semantic_bytes(diagnostics: &DiagnosticList) -> String {
    let mut out = String::new();
    for diagnostic in diagnostics {
        let _ = writeln!(
            out,
            "{}|{}|{}",
            diagnostic.file_name.as_deref().unwrap_or(""),
            diagnostic.code(),
            diagnostic.message_text()
        );
    }
    out
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LedgerEntry {
    rust_path: PathBuf,
    rust_line: usize,
    rust_fn: String,
    port_name: String,
    version: String,
    span_file: String,
    span_start: usize,
    span_end: usize,
    hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct PublicFunction {
    path: PathBuf,
    line: usize,
    name: String,
}

fn ledger_check() -> Result<(), Box<dyn Error>> {
    let workspace = find_tsrs2_root()?;
    let entries = collect_ledger_entries(&workspace)?;
    let stale = verify_ledger_entries(&workspace, &entries)?;
    let public_functions = collect_hot_public_functions(&workspace)?;
    let unported = unported_public_functions(&entries, &public_functions);
    let todo_sites = collect_todo_port_sites(&workspace)?;

    for entry in &stale {
        eprintln!("{entry}");
    }
    for site in &todo_sites {
        eprintln!("todo_port site: {site}");
    }

    println!(
        "ledger check: entries={} stale={} hot_pub_fns={} unported_pub_fns={} todo_port={}",
        entries.len(),
        stale.len(),
        public_functions.len(),
        unported.len(),
        todo_sites.len()
    );
    if !unported.is_empty() {
        println!("unported pub fns:");
        for function in &unported {
            println!(
                "  {}:{} {}",
                display_relative(&workspace, &function.path),
                function.line,
                function.name
            );
        }
    }

    if !stale.is_empty() || !todo_sites.is_empty() {
        return Err("ledger check failed".into());
    }
    Ok(())
}

fn ledger_coverage() -> Result<(), Box<dyn Error>> {
    let workspace = find_tsrs2_root()?;
    let entries = collect_ledger_entries(&workspace)?;
    let public_functions = collect_hot_public_functions(&workspace)?;
    let unported = unported_public_functions(&entries, &public_functions);

    println!(
        "ledger coverage: ported_entries={} hot_pub_fns={} unported_pub_fns={}",
        entries.len(),
        public_functions.len(),
        unported.len()
    );
    println!("ledger coverage: runtime hit data is not instrumented in M0");
    Ok(())
}

fn collect_ledger_entries(workspace: &Path) -> Result<Vec<LedgerEntry>, Box<dyn Error>> {
    let mut entries = Vec::new();
    for path in collect_rs_paths(&workspace.join("crates"))? {
        let text = fs::read_to_string(&path)?;
        entries.extend(parse_ledger_entries_in_file(&path, &text)?);
    }
    entries.sort_by(|left, right| {
        left.rust_path
            .cmp(&right.rust_path)
            .then_with(|| left.rust_line.cmp(&right.rust_line))
    });
    Ok(entries)
}

fn parse_ledger_entries_in_file(
    path: &Path,
    text: &str,
) -> Result<Vec<LedgerEntry>, Box<dyn Error>> {
    let mut entries = Vec::new();
    let mut docs = Vec::<String>::new();
    let mut doc_start = 0usize;

    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim_start();
        if let Some(doc) = trimmed.strip_prefix("///") {
            if docs.is_empty() {
                doc_start = line_number;
            }
            docs.push(doc.trim().to_owned());
            continue;
        }

        if trimmed.is_empty() || trimmed.starts_with("#[") {
            continue;
        }

        if let Some(fn_name) = function_name(trimmed) {
            if let Some(entry) = parse_ledger_doc(path, doc_start, &fn_name, &docs)? {
                entries.push(entry);
            }
        }
        docs.clear();
    }

    Ok(entries)
}

fn parse_ledger_doc(
    path: &Path,
    rust_line: usize,
    rust_fn: &str,
    docs: &[String],
) -> Result<Option<LedgerEntry>, Box<dyn Error>> {
    let Some(port_line) = docs
        .iter()
        .find_map(|doc| doc.strip_prefix("tsc-port:").map(str::trim))
    else {
        return Ok(None);
    };
    let hash = docs
        .iter()
        .find_map(|doc| doc.strip_prefix("tsc-hash:").map(str::trim))
        .and_then(|value| value.split_whitespace().next())
        .ok_or_else(|| format!("{}:{rust_line} missing tsc-hash", path.display()))?;
    let span = docs
        .iter()
        .find_map(|doc| doc.strip_prefix("tsc-span:").map(str::trim))
        .ok_or_else(|| format!("{}:{rust_line} missing tsc-span", path.display()))?;
    let (port_name, version) = parse_tsc_port(port_line)
        .ok_or_else(|| format!("{}:{rust_line} malformed tsc-port", path.display()))?;
    let (span_file, span_start, span_end) = parse_tsc_span(span)
        .ok_or_else(|| format!("{}:{rust_line} malformed tsc-span", path.display()))?;

    Ok(Some(LedgerEntry {
        rust_path: path.to_owned(),
        rust_line,
        rust_fn: rust_fn.to_owned(),
        port_name,
        version,
        span_file,
        span_start,
        span_end,
        hash: hash.to_owned(),
    }))
}

fn parse_tsc_port(value: &str) -> Option<(String, String)> {
    let mut parts = value.split_whitespace();
    let name = parts.next()?.to_owned();
    let version = parts.next()?.strip_prefix('@')?.to_owned();
    Some((name, version))
}

fn parse_tsc_span(value: &str) -> Option<(String, usize, usize)> {
    let (file, range) = value.rsplit_once(':')?;
    let (start, end) = range.split_once('-')?;
    Some((file.to_owned(), start.parse().ok()?, end.parse().ok()?))
}

fn verify_ledger_entries(
    workspace: &Path,
    entries: &[LedgerEntry],
) -> Result<Vec<String>, Box<dyn Error>> {
    let mut stale = Vec::new();
    for entry in entries {
        let actual = source_slice_hash(
            workspace,
            &entry.span_file,
            entry.span_start,
            entry.span_end,
        )?;
        if actual != entry.hash {
            stale.push(format!(
                "{}:{} {} stale: expected {} actual {}",
                display_relative(workspace, &entry.rust_path),
                entry.rust_line,
                entry.rust_fn,
                entry.hash,
                actual
            ));
        }
    }
    Ok(stale)
}

fn source_slice_hash(
    workspace: &Path,
    span_file: &str,
    start: usize,
    end: usize,
) -> Result<String, Box<dyn Error>> {
    if start == 0 || end < start {
        return Err(format!("invalid tsc span range {start}-{end}").into());
    }
    let path = ledger_source_path(workspace, span_file)?;
    let text = fs::read_to_string(&path)?;
    let lines = text.split_inclusive('\n').collect::<Vec<_>>();
    if end > lines.len() {
        return Err(format!(
            "{} has {} lines, cannot read {start}-{end}",
            path.display(),
            lines.len()
        )
        .into());
    }
    let slice = lines[start - 1..end].concat();
    Ok(sha256_hex(slice.as_bytes()))
}

fn ledger_source_path(workspace: &Path, span_file: &str) -> Result<PathBuf, Box<dyn Error>> {
    let span_path = Path::new(span_file);
    if span_path.is_absolute() && span_path.is_file() {
        return Ok(span_path.to_owned());
    }

    let mut candidates = vec![
        workspace
            .join("vendor/typescript-6.0.3/src/compiler")
            .join(span_file),
        workspace
            .join("vendor/typescript-6.0.3/lib")
            .join(span_file),
    ];
    if let Some(parent) = workspace.parent() {
        candidates.push(parent.join("ts-tests/src/compiler").join(span_file));
        candidates.push(parent.join(span_file));
    }
    candidates.push(workspace.join(span_file));

    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| format!("missing ledger source file: {span_file}").into())
}

fn collect_hot_public_functions(workspace: &Path) -> Result<Vec<PublicFunction>, Box<dyn Error>> {
    let hot_files = [
        workspace.join("crates/checker/src/lib.rs"),
        workspace.join("crates/binder/src/lib.rs"),
        workspace.join("crates/syntax/src/lib.rs"),
        workspace.join("crates/syntax/src/for_each_child.rs"),
        workspace.join("crates/syntax/src/scanner.rs"),
    ];
    let mut functions = Vec::new();
    for path in hot_files {
        if !path.is_file() {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if is_public_fn_line(trimmed) {
                if let Some(name) = function_name(trimmed) {
                    functions.push(PublicFunction {
                        path: path.clone(),
                        line: index + 1,
                        name,
                    });
                }
            }
        }
    }
    functions.sort();
    Ok(functions)
}

fn unported_public_functions(
    entries: &[LedgerEntry],
    public_functions: &[PublicFunction],
) -> Vec<PublicFunction> {
    let ported = entries
        .iter()
        .map(|entry| (entry.rust_path.clone(), entry.rust_fn.clone()))
        .collect::<BTreeSet<_>>();
    public_functions
        .iter()
        .filter(|function| !ported.contains(&(function.path.clone(), function.name.clone())))
        .cloned()
        .collect()
}

fn collect_todo_port_sites(workspace: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let mut sites = Vec::new();
    for path in collect_rs_paths(&workspace.join("crates"))? {
        if path
            .strip_prefix(workspace)
            .is_ok_and(|relative| relative.starts_with("crates/xtask"))
        {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        for (index, line) in text.lines().enumerate() {
            if line.contains("todo_port!(") {
                sites.push(format!(
                    "{}:{}",
                    display_relative(workspace, &path),
                    index + 1
                ));
            }
        }
    }
    sites.sort();
    Ok(sites)
}

fn collect_rs_paths(root: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut stack = vec![root.to_owned()];
    let mut paths = Vec::new();
    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().and_then(|name| name.to_str()) != Some("target") {
                    stack.push(path);
                }
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
                paths.push(path);
            }
        }
    }
    paths.sort();
    Ok(paths)
}

fn is_public_fn_line(line: &str) -> bool {
    line.starts_with("pub fn ") || line.starts_with("pub async fn ")
}

fn function_name(line: &str) -> Option<String> {
    let fn_start = line.find("fn ")? + "fn ".len();
    let rest = &line[fn_start..];
    let name = rest
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect::<String>();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn base64_decode_to_string(input: &str) -> Result<String, Box<dyn Error>> {
    let bytes = input.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err("invalid base64 length".into());
    }

    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks_exact(4) {
        let chunk = [chunk[0], chunk[1], chunk[2], chunk[3]];
        decode_base64_chunk(&chunk, &mut out)?;
    }
    Ok(String::from_utf8(out)?)
}

fn decode_base64_chunk(chunk: &[u8; 4], out: &mut Vec<u8>) -> Result<(), Box<dyn Error>> {
    let pad = chunk.iter().rev().take_while(|byte| **byte == b'=').count();
    if pad > 2 {
        return Err("invalid base64 padding".into());
    }

    let first = decode_base64_value(chunk[0])?;
    let second = decode_base64_value(chunk[1])?;
    let third = if chunk[2] == b'=' {
        0
    } else {
        decode_base64_value(chunk[2])?
    };
    let fourth = if chunk[3] == b'=' {
        0
    } else {
        decode_base64_value(chunk[3])?
    };

    out.push((first << 2) | (second >> 4));
    if pad < 2 {
        out.push((second << 4) | (third >> 2));
    }
    if pad < 1 {
        out.push((third << 6) | fourth);
    }
    Ok(())
}

fn decode_base64_value(byte: u8) -> Result<u8, Box<dyn Error>> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(format!("invalid base64 byte: {byte}").into()),
    }
}

fn ci() -> Result<(), Box<dyn Error>> {
    run_command(Command::new("cargo").arg("build").arg("--workspace"))?;
    run_command(Command::new("cargo").arg("test").arg("--workspace"))?;
    run_command(Command::new("cargo").arg("xtask").arg("conformance"))?;
    run_command(
        Command::new("cargo")
            .arg("xtask")
            .arg("invariants")
            .arg("--suite")
            .arg("all"),
    )?;
    run_command(
        Command::new("cargo")
            .arg("xtask")
            .arg("ledger")
            .arg("check"),
    )?;
    Ok(())
}

fn run_command(command: &mut Command) -> Result<(), Box<dyn Error>> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("command failed with status {status:?}").into());
    }
    Ok(())
}

fn oracle_smoke(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let mut limit = 100usize;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--limit" => {
                let value = args.next().ok_or("missing value after --limit")?;
                limit = value.parse()?;
            }
            _ => return Err(format!("unexpected oracle-smoke argument: {arg}").into()),
        }
    }

    let workspace = find_tsrs2_root()?;
    let fixtures_root = workspace.join("ts-tests/tests/cases/conformance");
    let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");
    let temp_root = std::env::temp_dir().join(format!("tsrs2-oracle-smoke-{}", std::process::id()));
    if temp_root.exists() {
        fs::remove_dir_all(&temp_root)?;
    }
    fs::create_dir_all(&temp_root)?;

    let mut fixtures = collect_fixture_paths(&fixtures_root)?;
    fixtures.sort();
    fixtures.truncate(limit);

    let pool = tsrs2_oracle::OraclePool::new(tsrs2_oracle::OraclePool::default_size())?;
    let mut program_count = 0usize;
    for (index, fixture) in fixtures.iter().enumerate() {
        let programs = tsrs2_harness::expand_fixture_file(fixture, &vendor_lib_dir)?;
        let out_dir = temp_root.join(index.to_string());
        let paths = tsrs2_harness::write_program_jsons(&programs, &out_dir)?;
        for path in paths {
            let first = pool.diagnostics(&path)?;
            let second = pool.diagnostics(&path)?;
            if first != second {
                return Err(
                    format!("oracle output changed between runs for {}", path.display()).into(),
                );
            }
            program_count += 1;
        }
    }

    fs::remove_dir_all(&temp_root)?;
    println!(
        "oracle smoke passed: {} fixtures, {} program.json files",
        fixtures.len(),
        program_count
    );
    Ok(())
}

fn collect_fixture_paths(root: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut stack = vec![root.to_owned()];
    let mut fixtures = Vec::new();
    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if is_fixture_path(&path) {
                fixtures.push(path);
            }
        }
    }
    Ok(fixtures)
}

fn is_fixture_path(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("ts" | "tsx" | "js" | "jsx")
    )
}

fn run_or_exit(result: Result<(), Box<dyn Error>>) {
    if let Err(err) = result {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn scaffold_smoke() {
    let harness_diags = tsrs2_harness::check_empty_program().diagnostics.len();
    let conformance_diags = tsrs2_conformance::run_empty_engine_smoke();

    if harness_diags != 0 || conformance_diags != 0 {
        eprintln!("empty-engine scaffold emitted diagnostics");
        std::process::exit(1);
    }

    println!("tsrs2 scaffold ready");
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EnumMember {
    name: String,
    value: EnumValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum EnumValue {
    Int(i32),
    Str(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EnumTable {
    name: String,
    members: Vec<EnumMember>,
}

#[derive(Clone, Copy)]
struct SourceEnum {
    name: &'static str,
    file: &'static str,
}

const RUNTIME_ENUMS: &[&str] = &[
    "SyntaxKind",
    "NodeFlags",
    "ModifierFlags",
    "RelationComparisonResult",
    "FlowFlags",
    "SymbolFlags",
    "TypeFlags",
    "ObjectFlags",
    "SignatureFlags",
    "DiagnosticCategory",
    "ModuleKind",
    "TypeFacts",
    "CheckMode",
];

const CONST_ENUMS: &[SourceEnum] = &[
    SourceEnum {
        name: "TokenFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "UnionReduction",
        file: "types.ts",
    },
    SourceEnum {
        name: "ContextFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "CheckFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "InternalSymbolName",
        file: "types.ts",
    },
    SourceEnum {
        name: "ElementFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "AccessFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "TypeMapKind",
        file: "types.ts",
    },
    SourceEnum {
        name: "InferencePriority",
        file: "types.ts",
    },
    SourceEnum {
        name: "InferenceFlags",
        file: "types.ts",
    },
    SourceEnum {
        name: "Ternary",
        file: "types.ts",
    },
    SourceEnum {
        name: "ScriptTarget",
        file: "types.ts",
    },
    SourceEnum {
        name: "CharacterCodes",
        file: "types.ts",
    },
    SourceEnum {
        name: "IntersectionState",
        file: "checker.ts",
    },
    SourceEnum {
        name: "RecursionFlags",
        file: "checker.ts",
    },
    SourceEnum {
        name: "ExpandingFlags",
        file: "checker.ts",
    },
    SourceEnum {
        name: "ParsingContext",
        file: "parser.ts",
    },
];

fn codegen_enums(check: bool) -> Result<(), Box<dyn Error>> {
    let workspace = find_tsrs2_root()?;
    let tsc_path = workspace.join("vendor/typescript-6.0.3/lib/_tsc.js");
    let tsc = fs::read_to_string(&tsc_path)?;

    let mut runtime_tables = BTreeMap::new();
    for name in RUNTIME_ENUMS {
        let table = parse_runtime_enum(&tsc, name)?;
        runtime_tables.insert((*name).to_owned(), table);
    }

    let mut source_tables = BTreeMap::new();
    for source in CONST_ENUMS {
        let path = compiler_source_path(&workspace, source.file)?;
        let text = fs::read_to_string(path)?;
        let table = parse_source_enum(&text, source.name)?;
        source_tables.insert(source.name.to_owned(), table);
    }

    let syntax = runtime_tables
        .remove("SyntaxKind")
        .ok_or("missing generated SyntaxKind")?;
    let kind_rs = rustfmt_text(&render_syntax_kind(&syntax)?)?;

    let mut flags_tables: Vec<EnumTable> = runtime_tables.into_values().collect();
    flags_tables.extend(source_tables.into_values());
    flags_tables.sort_by(|a, b| a.name.cmp(&b.name));
    let flags_rs = rustfmt_text(&render_flags(&flags_tables)?)?;

    let kind_path = workspace.join("crates/syntax/src/kind.rs");
    let flags_path = workspace.join("crates/types/src/flags.rs");
    write_generated(&kind_path, &kind_rs, check)?;
    write_generated(&flags_path, &flags_rs, check)?;

    if check {
        println!("generated enum files are up to date");
    } else {
        println!("generated enum files");
    }

    Ok(())
}

fn codegen_scanner(check: bool) -> Result<(), Box<dyn Error>> {
    let workspace = find_tsrs2_root()?;
    let tsc_path = workspace.join("vendor/typescript-6.0.3/lib/_tsc.js");
    let tsc = fs::read_to_string(&tsc_path)?;

    let identifier_start = parse_unicode_range_pairs(&tsc, "unicodeESNextIdentifierStart")?;
    let identifier_part = parse_unicode_range_pairs(&tsc, "unicodeESNextIdentifierPart")?;
    let keywords = parse_text_to_keyword_obj(&tsc)?;

    let chars_rs = rustfmt_text(&render_scanner_chars_rs(
        &identifier_start,
        &identifier_part,
    )?)?;
    let keywords_rs = rustfmt_text(&render_scanner_keywords_rs(&keywords)?)?;

    write_generated(
        &workspace.join("crates/syntax/src/chars.rs"),
        &chars_rs,
        check,
    )?;
    write_generated(
        &workspace.join("crates/syntax/src/keywords.rs"),
        &keywords_rs,
        check,
    )?;

    if check {
        println!("generated scanner files are up to date");
    } else {
        println!("generated scanner files");
    }

    Ok(())
}

fn parse_unicode_range_pairs(tsc: &str, name: &str) -> Result<Vec<(u32, u32)>, Box<dyn Error>> {
    let values = parse_js_number_array(tsc, name)?;
    if values.len() % 2 != 0 {
        return Err(format!("{name} has an odd number of range endpoints").into());
    }

    let mut ranges = Vec::with_capacity(values.len() / 2);
    for pair in values.chunks_exact(2) {
        ranges.push((pair[0], pair[1]));
    }
    Ok(ranges)
}

fn parse_js_number_array(tsc: &str, name: &str) -> Result<Vec<u32>, Box<dyn Error>> {
    let marker = format!("var {name} = [");
    let start = tsc
        .find(&marker)
        .ok_or_else(|| format!("missing scanner array: {name}"))?
        + marker.len();
    let rest = &tsc[start..];
    let end = rest
        .find("];")
        .ok_or_else(|| format!("unterminated scanner array: {name}"))?;

    rest[..end]
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(parse_js_number)
        .collect()
}

fn parse_js_number(value: &str) -> Result<u32, Box<dyn Error>> {
    if value.contains('e') || value.contains('E') {
        let parsed = value.parse::<f64>()?;
        if parsed.fract() != 0.0 || parsed < 0.0 || parsed > u32::MAX as f64 {
            return Err(format!("non-integer JavaScript number literal: {value}").into());
        }
        Ok(parsed as u32)
    } else {
        Ok(value.parse()?)
    }
}

fn parse_text_to_keyword_obj(tsc: &str) -> Result<Vec<(String, u16)>, Box<dyn Error>> {
    let marker = "var textToKeywordObj = {";
    let start = tsc.find(marker).ok_or("missing textToKeywordObj")? + marker.len();
    let rest = &tsc[start..];
    let end = rest.find("\n};").ok_or("unterminated textToKeywordObj")?;

    let mut keywords = Vec::new();
    for line in rest[..end].lines() {
        let line = line.trim().trim_end_matches(',');
        if line.is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| format!("malformed keyword entry: {line}"))?;
        let key = parse_keyword_key(key.trim())?;
        let value = value
            .split_whitespace()
            .next()
            .ok_or_else(|| format!("missing keyword value: {line}"))?
            .parse()?;
        keywords.push((key, value));
    }
    keywords.sort_by(|(left, _), (right, _)| left.cmp(right));
    Ok(keywords)
}

fn parse_keyword_key(key: &str) -> Result<String, Box<dyn Error>> {
    if let Some(quoted) = key
        .strip_prefix("[\"")
        .and_then(|key| key.strip_suffix("\"]"))
    {
        Ok(quoted.to_owned())
    } else if key
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        Ok(key.to_owned())
    } else {
        Err(format!("malformed keyword key: {key}").into())
    }
}

fn render_scanner_chars_rs(
    identifier_start: &[(u32, u32)],
    identifier_part: &[(u32, u32)],
) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen scanner`. Do not edit by hand.\n"
    )?;
    writeln!(
        out,
        "pub(crate) fn is_identifier_start(ch: char) -> bool {{"
    )?;
    writeln!(out, "    ch.is_ascii_alphabetic()")?;
    writeln!(out, "        || ch == '$'")?;
    writeln!(out, "        || ch == '_'")?;
    writeln!(
        out,
        "        || (ch as u32 > 0x7f && lookup_in_unicode_map(ch as u32, UNICODE_ES_NEXT_IDENTIFIER_START))"
    )?;
    writeln!(out, "}}\n")?;

    writeln!(out, "pub(crate) fn is_identifier_part(ch: char) -> bool {{")?;
    writeln!(out, "    ch.is_ascii_alphanumeric()")?;
    writeln!(out, "        || ch == '$'")?;
    writeln!(out, "        || ch == '_'")?;
    writeln!(
        out,
        "        || (ch as u32 > 0x7f && lookup_in_unicode_map(ch as u32, UNICODE_ES_NEXT_IDENTIFIER_PART))"
    )?;
    writeln!(out, "}}\n")?;

    writeln!(
        out,
        "fn lookup_in_unicode_map(code: u32, map: &[(u32, u32)]) -> bool {{"
    )?;
    writeln!(out, "    map.binary_search_by(|&(start, end)| {{")?;
    writeln!(out, "        if code < start {{")?;
    writeln!(out, "            std::cmp::Ordering::Greater")?;
    writeln!(out, "        }} else if code > end {{")?;
    writeln!(out, "            std::cmp::Ordering::Less")?;
    writeln!(out, "        }} else {{")?;
    writeln!(out, "            std::cmp::Ordering::Equal")?;
    writeln!(out, "        }}")?;
    writeln!(out, "    }})")?;
    writeln!(out, "    .is_ok()")?;
    writeln!(out, "}}\n")?;

    render_range_table(
        &mut out,
        "UNICODE_ES_NEXT_IDENTIFIER_START",
        identifier_start,
    )?;
    writeln!(out)?;
    render_range_table(&mut out, "UNICODE_ES_NEXT_IDENTIFIER_PART", identifier_part)?;
    Ok(out)
}

fn render_range_table(
    out: &mut String,
    name: &str,
    ranges: &[(u32, u32)],
) -> Result<(), Box<dyn Error>> {
    writeln!(out, "const {name}: &[(u32, u32)] = &[")?;
    for chunk in ranges.chunks(4) {
        write!(out, "    ")?;
        for (index, (start, end)) in chunk.iter().enumerate() {
            if index > 0 {
                write!(out, ", ")?;
            }
            write!(out, "({start}, {end})")?;
        }
        writeln!(out, ",")?;
    }
    writeln!(out, "];")?;
    Ok(())
}

fn render_scanner_keywords_rs(keywords: &[(String, u16)]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen scanner`. Do not edit by hand.\n"
    )?;
    writeln!(out, "use crate::SyntaxKind;\n")?;
    writeln!(
        out,
        "pub(crate) fn keyword_kind(text: &str) -> Option<SyntaxKind> {{"
    )?;
    writeln!(out, "    let kind = match text {{")?;
    for (keyword, value) in keywords {
        writeln!(out, "        \"{keyword}\" => {value},")?;
    }
    writeln!(out, "        _ => return None,")?;
    writeln!(out, "    }};")?;
    writeln!(out, "    SyntaxKind::from_u16(kind)")?;
    writeln!(out, "}}")?;
    Ok(out)
}

fn find_tsrs2_root() -> Result<PathBuf, Box<dyn Error>> {
    let cwd = std::env::current_dir()?;
    for dir in cwd.ancestors() {
        if dir.join("vendor/typescript-6.0.3/lib/_tsc.js").is_file() {
            return Ok(dir.to_owned());
        }

        let nested = dir.join("tsrs2");
        if nested.join("vendor/typescript-6.0.3/lib/_tsc.js").is_file() {
            return Ok(nested);
        }
    }

    Err("could not find tsrs2 workspace root".into())
}

fn compiler_source_path(workspace: &Path, file: &str) -> Result<PathBuf, Box<dyn Error>> {
    let vendored = workspace
        .join("vendor/typescript-6.0.3/src/compiler")
        .join(file);
    if vendored.is_file() {
        return Ok(vendored);
    }

    let checkout = workspace
        .parent()
        .ok_or("tsrs2 workspace has no parent")?
        .join("ts-tests/src/compiler")
        .join(file);
    if checkout.is_file() {
        return Ok(checkout);
    }

    Err(format!("missing TypeScript compiler source file for const enum extraction: {file}").into())
}

fn write_generated(path: &Path, text: &str, check: bool) -> Result<(), Box<dyn Error>> {
    if check {
        let current = fs::read_to_string(path)?;
        if current != text {
            return Err(format!("{} is not up to date", path.display()).into());
        }
    } else {
        fs::write(path, text)?;
    }
    Ok(())
}

fn rustfmt_text(text: &str) -> Result<String, Box<dyn Error>> {
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2021", "--emit", "stdout"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    child
        .stdin
        .as_mut()
        .ok_or("failed to open rustfmt stdin")?
        .write_all(text.as_bytes())?;

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(format!(
            "rustfmt failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    Ok(String::from_utf8(output.stdout)?)
}

fn parse_runtime_enum(tsc: &str, enum_name: &str) -> Result<EnumTable, Box<dyn Error>> {
    let start_marker = format!("var {enum_name} = /* @__PURE__ */ ((");
    let start = tsc
        .find(&start_marker)
        .ok_or_else(|| format!("runtime enum {enum_name} not found in _tsc.js"))?;
    let after_start = &tsc[start..];
    let end = after_start
        .find(&format!("return {enum_name}"))
        .ok_or_else(|| format!("runtime enum {enum_name} has no return sentinel"))?;
    let block = &after_start[..end];

    let mut members = Vec::new();
    for line in block.lines() {
        if let Some(member) = parse_runtime_member(line)? {
            members.push(member);
        }
    }

    if members.is_empty() {
        return Err(format!("runtime enum {enum_name} had no members").into());
    }

    Ok(EnumTable {
        name: enum_name.to_owned(),
        members,
    })
}

fn parse_runtime_member(line: &str) -> Result<Option<EnumMember>, Box<dyn Error>> {
    let Some(name_marker_start) = line.find("[\"") else {
        return Ok(None);
    };
    let name_start = name_marker_start + 2;
    let name_end = line[name_start..]
        .find("\"]")
        .map(|offset| name_start + offset)
        .ok_or_else(|| format!("malformed runtime enum line: {line}"))?;
    let name = &line[name_start..name_end];

    let after_name = &line[name_end + 2..];
    let equals = after_name
        .find('=')
        .ok_or_else(|| format!("runtime enum member has no value: {line}"))?;
    let value_text = after_name[equals + 1..].trim_start();
    let value_end = value_text
        .char_indices()
        .find_map(|(idx, ch)| {
            if (idx == 0 && ch == '-') || ch.is_ascii_digit() {
                None
            } else {
                Some(idx)
            }
        })
        .unwrap_or(value_text.len());
    let value: i32 = value_text[..value_end].parse()?;

    Ok(Some(EnumMember {
        name: name.to_owned(),
        value: EnumValue::Int(value),
    }))
}

fn parse_source_enum(source: &str, enum_name: &str) -> Result<EnumTable, Box<dyn Error>> {
    let block = source_enum_block(source, enum_name)?;
    let mut values = BTreeMap::<String, EnumValue>::new();
    let mut members = Vec::new();
    let mut next_auto_int = Some(0i32);
    let mut in_block_comment = false;

    for raw_line in block.lines() {
        let mut line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if in_block_comment {
            if let Some(end) = line.find("*/") {
                line = line[end + 2..].trim();
                in_block_comment = false;
            } else {
                continue;
            }
        }

        while line.starts_with("/*") {
            if let Some(end) = line.find("*/") {
                line = line[end + 2..].trim();
            } else {
                in_block_comment = true;
                line = "";
                break;
            }
        }

        if line.is_empty() || line.starts_with('*') || line.starts_with("//") {
            continue;
        }

        let without_comment = strip_line_comment(line);
        let mut entry = without_comment.trim().trim_end_matches(',').trim();
        if entry.is_empty() {
            continue;
        }

        if entry.starts_with("export ") {
            continue;
        }

        let (name, value) = if let Some(eq) = entry.find('=') {
            let name = entry[..eq].trim();
            let expr = entry[eq + 1..].trim();
            let value = if is_string_literal(expr) {
                EnumValue::Str(unquote_string(expr)?)
            } else {
                EnumValue::Int(eval_int_expr(expr, &values)?)
            };
            (name, value)
        } else {
            let value = next_auto_int.ok_or_else(|| {
                format!("cannot auto-increment after string enum member: {entry}")
            })?;
            (entry, EnumValue::Int(value))
        };

        if name.is_empty() {
            return Err(format!("empty member name in enum {enum_name}").into());
        }

        entry = name;
        values.insert(entry.to_owned(), value.clone());
        next_auto_int = match value {
            EnumValue::Int(value) => Some(value + 1),
            EnumValue::Str(_) => None,
        };
        members.push(EnumMember {
            name: entry.to_owned(),
            value,
        });
    }

    if members.is_empty() {
        return Err(format!("source enum {enum_name} had no members").into());
    }

    Ok(EnumTable {
        name: enum_name.to_owned(),
        members,
    })
}

fn source_enum_block<'a>(source: &'a str, enum_name: &str) -> Result<&'a str, Box<dyn Error>> {
    let needle = format!("enum {enum_name}");
    let enum_pos = source
        .find(&needle)
        .ok_or_else(|| format!("source enum {enum_name} not found"))?;
    let after_enum = &source[enum_pos..];
    let open_rel = after_enum
        .find('{')
        .ok_or_else(|| format!("source enum {enum_name} has no opening brace"))?;
    let open = enum_pos + open_rel;
    let mut depth = 0usize;
    let mut close = None;

    for (offset, ch) in source[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + offset);
                    break;
                }
            }
            _ => {}
        }
    }

    let close = close.ok_or_else(|| format!("source enum {enum_name} has no closing brace"))?;
    Ok(&source[open + 1..close])
}

fn strip_line_comment(line: &str) -> String {
    let mut quoted = false;
    let mut escaped = false;
    let mut prev = '\0';

    for (idx, ch) in line.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
        } else if ch == '"' {
            quoted = true;
        } else if prev == '/' && ch == '/' {
            return line[..idx - 1].to_owned();
        }
        prev = ch;
    }

    line.to_owned()
}

fn is_string_literal(expr: &str) -> bool {
    expr.starts_with('"') && expr.ends_with('"')
}

fn unquote_string(expr: &str) -> Result<String, Box<dyn Error>> {
    if !is_string_literal(expr) {
        return Err(format!("not a string literal: {expr}").into());
    }

    Ok(expr[1..expr.len() - 1]
        .replace("\\\"", "\"")
        .replace("\\\\", "\\"))
}

fn eval_int_expr(expr: &str, values: &BTreeMap<String, EnumValue>) -> Result<i32, Box<dyn Error>> {
    let expr = trim_wrapping_parens(expr.trim());
    let mut result = 0i32;

    for part in expr.split('|') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        result |= eval_shift_expr(part, values)?;
    }

    Ok(result)
}

fn eval_shift_expr(
    expr: &str,
    values: &BTreeMap<String, EnumValue>,
) -> Result<i32, Box<dyn Error>> {
    if let Some(shift) = expr.find("<<") {
        let left = eval_atom(&expr[..shift], values)?;
        let right = eval_atom(&expr[shift + 2..], values)?;
        return Ok(left << right);
    }

    eval_atom(expr, values)
}

fn eval_atom(expr: &str, values: &BTreeMap<String, EnumValue>) -> Result<i32, Box<dyn Error>> {
    let expr = trim_wrapping_parens(expr.trim());
    if let Some(rest) = expr.strip_prefix('-') {
        return Ok(-eval_atom(rest, values)?);
    }

    if let Some(hex) = expr.strip_prefix("0x").or_else(|| expr.strip_prefix("0X")) {
        return Ok(i32::from_str_radix(hex, 16)?);
    }

    if expr.chars().all(|ch| ch.is_ascii_digit()) {
        return Ok(expr.parse()?);
    }

    match values.get(expr) {
        Some(EnumValue::Int(value)) => Ok(*value),
        Some(EnumValue::Str(_)) => {
            Err(format!("string enum member used as integer: {expr}").into())
        }
        None => Err(format!("unknown enum value expression: {expr}").into()),
    }
}

fn trim_wrapping_parens(mut expr: &str) -> &str {
    loop {
        let trimmed = expr.trim();
        if trimmed.starts_with(')') || !trimmed.starts_with('(') || !trimmed.ends_with(')') {
            return trimmed;
        }

        let mut depth = 0i32;
        let mut wraps = true;
        for (idx, ch) in trimmed.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 && idx != trimmed.len() - 1 {
                        wraps = false;
                        break;
                    }
                }
                _ => {}
            }
        }

        if wraps {
            expr = &trimmed[1..trimmed.len() - 1];
        } else {
            return trimmed;
        }
    }
}

fn render_syntax_kind(table: &EnumTable) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen enums`. Do not edit by hand."
    )?;
    writeln!(out)?;
    writeln!(out, "#![allow(non_upper_case_globals)]")?;
    writeln!(out)?;
    writeln!(out, "#[repr(u16)]")?;
    writeln!(
        out,
        "#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]"
    )?;
    writeln!(out, "pub enum SyntaxKind {{")?;

    let mut canonical = BTreeMap::<i32, String>::new();
    let mut aliases = Vec::<(&EnumMember, String)>::new();
    for member in &table.members {
        let value = member_int(member, &table.name)?;
        if let Some(existing) = canonical.get(&value) {
            aliases.push((member, existing.clone()));
            continue;
        }

        canonical.insert(value, member.name.clone());
        writeln!(out, "    /// tsc SyntaxKind.{}", member.name)?;
        writeln!(out, "    {} = {},", member.name, value)?;
    }
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl SyntaxKind {{")?;
    for (member, target) in aliases {
        writeln!(out, "    /// tsc SyntaxKind.{}", member.name)?;
        writeln!(
            out,
            "    pub const {}: Self = Self::{};",
            member.name, target
        )?;
    }
    writeln!(out)?;
    writeln!(out, "    pub const fn value(self) -> u16 {{")?;
    writeln!(out, "        self as u16")?;
    writeln!(out, "    }}")?;
    writeln!(out)?;
    writeln!(
        out,
        "    pub const fn from_u16(value: u16) -> Option<Self> {{"
    )?;
    writeln!(out, "        match value {{")?;
    for (value, name) in &canonical {
        writeln!(out, "            {} => Some(Self::{}),", value, name)?;
    }
    writeln!(out, "            _ => None,")?;
    writeln!(out, "        }}")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "#[cfg(test)]")?;
    writeln!(out, "mod tests {{")?;
    writeln!(out, "    use super::SyntaxKind;")?;
    writeln!(out)?;
    writeln!(out, "    #[test]")?;
    writeln!(out, "    fn generated_values_match_tsc_pins() {{")?;
    writeln!(
        out,
        "        assert_eq!(SyntaxKind::Identifier as u16, 80);"
    )?;
    writeln!(
        out,
        "        assert_eq!(SyntaxKind::FirstAssignment.value(), SyntaxKind::EqualsToken as u16);"
    )?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;

    Ok(out)
}

fn render_flags(tables: &[EnumTable]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen enums`. Do not edit by hand."
    )?;
    writeln!(out)?;

    for table in tables {
        if table
            .members
            .iter()
            .all(|member| matches!(member.value, EnumValue::Int(_)))
        {
            render_int_table(&mut out, table)?;
        } else {
            render_string_table(&mut out, table)?;
        }
        writeln!(out)?;
    }

    writeln!(out, "#[cfg(test)]")?;
    writeln!(out, "mod tests {{")?;
    writeln!(out, "    use super::*;")?;
    writeln!(out)?;
    writeln!(out, "    #[test]")?;
    writeln!(out, "    fn generated_values_match_tsc_pins() {{")?;
    writeln!(
        out,
        "        assert_eq!(TypeFlags::STRING_LITERAL.bits(), 1024);"
    )?;
    writeln!(
        out,
        "        assert_eq!(FlowFlags::TRUE_CONDITION.bits(), 32);"
    )?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;

    Ok(out)
}

fn render_int_table(out: &mut String, table: &EnumTable) -> Result<(), Box<dyn Error>> {
    writeln!(
        out,
        "#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]"
    )?;
    writeln!(out, "pub struct {}(i32);", table.name)?;
    writeln!(out)?;
    writeln!(out, "impl {} {{", table.name)?;

    let mut used_names = BTreeMap::<String, usize>::new();
    for member in &table.members {
        let const_name = screaming_const_name(&member.name);
        let const_name = disambiguate_const_name(const_name, &mut used_names);
        let value = member_int(member, &table.name)?;
        writeln!(out, "    /// tsc {}.{}", table.name, member.name)?;
        writeln!(out, "    pub const {}: Self = Self({});", const_name, value)?;
    }

    writeln!(out)?;
    writeln!(out, "    pub const fn from_bits(bits: i32) -> Self {{")?;
    writeln!(out, "        Self(bits)")?;
    writeln!(out, "    }}")?;
    writeln!(out)?;
    writeln!(out, "    pub const fn bits(self) -> i32 {{")?;
    writeln!(out, "        self.0")?;
    writeln!(out, "    }}")?;
    writeln!(out)?;
    writeln!(out, "    pub const fn is_empty(self) -> bool {{")?;
    writeln!(out, "        self.0 == 0")?;
    writeln!(out, "    }}")?;
    writeln!(out)?;
    writeln!(
        out,
        "    pub const fn contains(self, other: Self) -> bool {{"
    )?;
    writeln!(out, "        (self.0 & other.0) == other.0")?;
    writeln!(out, "    }}")?;
    writeln!(out)?;
    writeln!(
        out,
        "    pub const fn intersects(self, other: Self) -> bool {{"
    )?;
    writeln!(out, "        (self.0 & other.0) != 0")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl std::ops::BitOr for {} {{", table.name)?;
    writeln!(out, "    type Output = Self;")?;
    writeln!(out)?;
    writeln!(out, "    fn bitor(self, rhs: Self) -> Self::Output {{")?;
    writeln!(out, "        Self(self.0 | rhs.0)")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl std::ops::BitAnd for {} {{", table.name)?;
    writeln!(out, "    type Output = Self;")?;
    writeln!(out)?;
    writeln!(out, "    fn bitand(self, rhs: Self) -> Self::Output {{")?;
    writeln!(out, "        Self(self.0 & rhs.0)")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl std::ops::BitOrAssign for {} {{", table.name)?;
    writeln!(out, "    fn bitor_assign(&mut self, rhs: Self) {{")?;
    writeln!(out, "        self.0 |= rhs.0;")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;

    Ok(())
}

fn render_string_table(out: &mut String, table: &EnumTable) -> Result<(), Box<dyn Error>> {
    writeln!(out, "pub struct {};", table.name)?;
    writeln!(out)?;
    writeln!(out, "impl {} {{", table.name)?;
    let mut used_names = BTreeMap::<String, usize>::new();
    for member in &table.members {
        let const_name = screaming_const_name(&member.name);
        let const_name = disambiguate_const_name(const_name, &mut used_names);
        let EnumValue::Str(value) = &member.value else {
            return Err(format!("mixed string/int enum is not supported: {}", table.name).into());
        };
        writeln!(out, "    /// tsc {}.{}", table.name, member.name)?;
        writeln!(
            out,
            "    pub const {}: &'static str = {:?};",
            const_name, value
        )?;
    }
    writeln!(out, "}}")?;
    Ok(())
}

fn member_int(member: &EnumMember, enum_name: &str) -> Result<i32, Box<dyn Error>> {
    match member.value {
        EnumValue::Int(value) => Ok(value),
        EnumValue::Str(_) => Err(format!("{enum_name}.{} is not an integer", member.name).into()),
    }
}

fn disambiguate_const_name(name: String, used: &mut BTreeMap<String, usize>) -> String {
    let count = used.entry(name.clone()).or_default();
    *count += 1;
    if *count == 1 {
        name
    } else {
        format!("{name}_{}", *count)
    }
}

fn screaming_const_name(ts_name: &str) -> String {
    if ts_name == "$" {
        return "DOLLAR".to_owned();
    }
    if ts_name == "_" {
        return "UNDERSCORE".to_owned();
    }

    let mut out = String::new();
    let chars: Vec<char> = ts_name.chars().collect();
    for (idx, ch) in chars.iter().copied().enumerate() {
        if !ch.is_ascii_alphanumeric() {
            if !out.ends_with('_') {
                out.push('_');
            }
            continue;
        }

        if idx > 0 && ch.is_ascii_uppercase() {
            let prev = chars[idx - 1];
            let next = chars.get(idx + 1).copied();
            let splits_word = (prev.is_ascii_lowercase() || prev.is_ascii_digit())
                || (prev.is_ascii_uppercase() && next.is_some_and(|c| c.is_ascii_lowercase()));
            if splits_word && !out.ends_with('_') {
                out.push('_');
            }
        }

        out.push(ch.to_ascii_uppercase());
    }

    let mut out = out.trim_matches('_').to_owned();
    if out.is_empty() {
        out = "VALUE".to_owned();
    }
    if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

fn is_rust_keyword(name: &str) -> bool {
    matches!(
        name,
        "as" | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "union"
            | "unsafe"
            | "use"
            | "where"
            | "while"
    )
}

#[derive(Clone, Debug)]
struct DtsField {
    name: String,
    type_text: String,
    optional: bool,
}

#[derive(Clone, Debug, Default)]
struct InterfaceDecl {
    bases: Vec<String>,
    fields: Vec<DtsField>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChildKind {
    Node,
    Nodes,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ChildVisit {
    name: String,
    kind: ChildKind,
}

#[derive(Clone, Debug)]
struct NodeSchema {
    kind_name: String,
    data_name: String,
    fields: Vec<SchemaField>,
    children: Vec<ChildVisit>,
}

#[derive(Clone, Debug)]
struct SchemaField {
    ts_name: String,
    rust_name: String,
    ty: RustFieldType,
    optional: bool,
    child: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RustFieldType {
    Node,
    NodeArray,
    Bool,
    String,
    Number,
    SyntaxKind,
    Payload,
}

fn codegen_nodes(check: bool) -> Result<(), Box<dyn Error>> {
    let workspace = find_tsrs2_root()?;
    let tsc = fs::read_to_string(workspace.join("vendor/typescript-6.0.3/lib/_tsc.js"))?;
    let dts = fs::read_to_string(workspace.join("vendor/typescript-6.0.3/lib/typescript.d.ts"))?;

    let child_table = parse_for_each_child_table(&tsc)?;
    let interfaces = parse_dts_interfaces(&dts)?;
    let mut dts_nodes = collect_dts_nodes(&interfaces)?;
    seed_token_payload_nodes(&mut dts_nodes);
    seed_fieldless_nodes(&mut dts_nodes);
    seed_grammar_flag_fields(&mut dts_nodes);
    let schemas = merge_node_schema(child_table, dts_nodes);

    let nodes_rs = rustfmt_text(&render_nodes_rs(&schemas)?)?;
    let for_each_child_rs = rustfmt_text(&render_for_each_child_rs(&schemas)?)?;
    let schema_json = render_nodes_schema_json(&schemas)?;

    write_generated(
        &workspace.join("crates/syntax/src/nodes.rs"),
        &nodes_rs,
        check,
    )?;
    write_generated(
        &workspace.join("crates/syntax/src/for_each_child.rs"),
        &for_each_child_rs,
        check,
    )?;
    write_generated(
        &workspace.join("crates/syntax/nodes.schema.json"),
        &schema_json,
        check,
    )?;

    if check {
        println!("generated node schema files are up to date");
    } else {
        println!("generated node schema files");
    }

    Ok(())
}

fn parse_for_each_child_table(
    tsc: &str,
) -> Result<BTreeMap<String, Vec<ChildVisit>>, Box<dyn Error>> {
    let table = extract_balanced_after(tsc, "var forEachChildTable = ", '{', '}')?;
    let mut helper_cache = BTreeMap::<String, Vec<ChildVisit>>::new();
    let mut result = BTreeMap::<String, Vec<ChildVisit>>::new();

    for entry in split_top_level_entries(table) {
        let Some(kind_start) = entry.find("/*") else {
            continue;
        };
        let kind_name_start = kind_start + 2;
        let kind_name_end = entry[kind_name_start..]
            .find("*/")
            .map(|offset| kind_name_start + offset)
            .ok_or_else(|| format!("malformed forEachChildTable entry: {entry}"))?;
        let kind_name = entry[kind_name_start..kind_name_end].trim().to_owned();
        let value = entry
            .split_once(':')
            .map(|(_, value)| value.trim())
            .ok_or_else(|| format!("forEachChildTable entry has no value: {entry}"))?;

        let visits = if value.starts_with("function ") {
            extract_visits(value)
        } else {
            let helper_name = value.trim_end_matches(',').trim();
            if let Some(visits) = helper_cache.get(helper_name) {
                visits.clone()
            } else {
                let helper = extract_function(tsc, helper_name)?;
                let visits = extract_visits(helper);
                helper_cache.insert(helper_name.to_owned(), visits.clone());
                visits
            }
        };
        result.insert(kind_name, visits);
    }

    if result.is_empty() {
        return Err("forEachChildTable extraction produced no entries".into());
    }

    Ok(result)
}

fn extract_balanced_after<'a>(
    text: &'a str,
    marker: &str,
    open_ch: char,
    close_ch: char,
) -> Result<&'a str, Box<dyn Error>> {
    let marker_pos = text
        .find(marker)
        .ok_or_else(|| format!("marker not found: {marker}"))?;
    let after_marker = marker_pos + marker.len();
    let open_rel = text[after_marker..]
        .find(open_ch)
        .ok_or_else(|| format!("opening delimiter not found after marker: {marker}"))?;
    let open = after_marker + open_rel;
    let mut depth = 0usize;
    let mut close = None;
    for (offset, ch) in text[open..].char_indices() {
        if ch == open_ch {
            depth += 1;
        } else if ch == close_ch {
            depth -= 1;
            if depth == 0 {
                close = Some(open + offset);
                break;
            }
        }
    }
    let close =
        close.ok_or_else(|| format!("closing delimiter not found after marker: {marker}"))?;
    Ok(&text[open + 1..close])
}

fn extract_function<'a>(text: &'a str, name: &str) -> Result<&'a str, Box<dyn Error>> {
    extract_balanced_after(text, &format!("function {name}("), '{', '}')
}

fn split_top_level_entries(block: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (idx, ch) in block.char_indices() {
        match ch {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                let entry = block[start..idx].trim();
                if !entry.is_empty() {
                    entries.push(entry.to_owned());
                }
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    let tail = block[start..].trim();
    if !tail.is_empty() {
        entries.push(tail.to_owned());
    }
    entries
}

fn extract_visits(text: &str) -> Vec<ChildVisit> {
    let mut visits = Vec::new();
    for (needle, kind) in [
        ("visitNode2(cbNode, node.", ChildKind::Node),
        ("visitNodes(cbNode, cbNodes, node.", ChildKind::Nodes),
    ] {
        let mut rest = text;
        while let Some(pos) = rest.find(needle) {
            let field_start = pos + needle.len();
            let after = &rest[field_start..];
            let field_len = after
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                .map(char::len_utf8)
                .sum::<usize>();
            if field_len > 0 {
                visits.push(ChildVisit {
                    name: after[..field_len].to_owned(),
                    kind,
                });
            }
            rest = &after[field_len..];
        }
    }

    // Whole-identifier occurrence position: a bare `find` would match
    // `node.type` inside `node.typeParameters` and scramble the visit order.
    visits.sort_by_key(|visit| field_occurrence_position(text, &visit.name));
    visits.dedup();
    visits
}

fn field_occurrence_position(text: &str, name: &str) -> usize {
    let needle = format!("node.{name}");
    let mut from = 0usize;
    while let Some(pos) = text[from..].find(&needle) {
        let abs = from + pos;
        let after = text[abs + needle.len()..].chars().next();
        if !matches!(after, Some(ch) if ch.is_ascii_alphanumeric() || ch == '_') {
            return abs;
        }
        from = abs + needle.len();
    }
    usize::MAX
}

fn parse_dts_interfaces(dts: &str) -> Result<BTreeMap<String, InterfaceDecl>, Box<dyn Error>> {
    let mut interfaces = BTreeMap::<String, InterfaceDecl>::new();
    let lines: Vec<&str> = dts.lines().collect();
    let mut idx = 0usize;

    while idx < lines.len() {
        let line = lines[idx].trim();
        let Some(interface_pos) = line.find("interface ") else {
            idx += 1;
            continue;
        };
        if !line[..interface_pos].trim().is_empty() {
            idx += 1;
            continue;
        }

        let header = line;
        let name_start = interface_pos + "interface ".len();
        let name_end = header[name_start..]
            .find(['<', ' ', '{'])
            .map(|offset| name_start + offset)
            .unwrap_or(header.len());
        let name = header[name_start..name_end].to_owned();
        let bases = parse_interface_bases(header);

        let mut body = String::new();
        let mut depth = header.matches('{').count() as i32 - header.matches('}').count() as i32;
        if let Some(open) = header.find('{') {
            body.push_str(&header[open + 1..]);
            body.push('\n');
        }

        idx += 1;
        while idx < lines.len() && depth > 0 {
            let body_line = lines[idx];
            depth += body_line.matches('{').count() as i32;
            depth -= body_line.matches('}').count() as i32;
            if depth >= 0 {
                body.push_str(body_line);
                body.push('\n');
            }
            idx += 1;
        }

        let fields = parse_interface_fields(&body);
        let decl = interfaces.entry(name).or_default();
        for base in bases {
            if !decl.bases.contains(&base) {
                decl.bases.push(base);
            }
        }
        for field in fields {
            merge_dts_field(&mut decl.fields, field);
        }
    }

    Ok(interfaces)
}

fn parse_interface_bases(header: &str) -> Vec<String> {
    let Some(extends_pos) = header.find(" extends ") else {
        return Vec::new();
    };
    let bases_text = header[extends_pos + " extends ".len()..]
        .split('{')
        .next()
        .unwrap_or_default();
    bases_text
        .split(',')
        .filter_map(|base| {
            let base = base.trim();
            if base.is_empty() {
                return None;
            }
            Some(
                base.split(|ch: char| ch == '<' || ch.is_whitespace())
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
        .filter(|base| !base.is_empty())
        .collect()
}

fn parse_interface_fields(body: &str) -> Vec<DtsField> {
    let mut fields = Vec::new();
    let mut entry = String::new();
    let mut in_block_comment = false;

    for raw_line in body.lines() {
        let mut line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if in_block_comment {
            if let Some(end) = line.find("*/") {
                line = line[end + 2..].trim();
                in_block_comment = false;
            } else {
                continue;
            }
        }
        while line.starts_with("/*") {
            if let Some(end) = line.find("*/") {
                line = line[end + 2..].trim();
            } else {
                in_block_comment = true;
                line = "";
                break;
            }
        }
        if line.is_empty() || line.starts_with('*') || line.starts_with("//") {
            continue;
        }

        entry.push_str(line);
        entry.push(' ');
        if line.ends_with(';') {
            if let Some(field) = parse_dts_field(&entry) {
                fields.push(field);
            }
            entry.clear();
        }
    }

    fields
}

fn parse_dts_field(entry: &str) -> Option<DtsField> {
    let entry = strip_line_comment(entry)
        .trim()
        .trim_end_matches(';')
        .trim()
        .to_owned();
    if entry.is_empty() || entry.contains('(') || entry.starts_with('[') {
        return None;
    }
    let entry = entry
        .strip_prefix("readonly ")
        .unwrap_or(&entry)
        .strip_prefix("/** @internal */ ")
        .unwrap_or(entry.as_str())
        .trim();
    let colon = entry.find(':')?;
    let mut name = entry[..colon].trim();
    let optional = name.ends_with('?') || entry[colon + 1..].contains("undefined");
    name = name.trim_end_matches('?').trim();
    if name.starts_with('_') || name == "parent" {
        return None;
    }
    Some(DtsField {
        name: name.trim_matches('"').to_owned(),
        type_text: entry[colon + 1..].trim().to_owned(),
        optional,
    })
}

fn merge_dts_field(fields: &mut Vec<DtsField>, field: DtsField) {
    if let Some(existing) = fields
        .iter_mut()
        .find(|existing| existing.name == field.name)
    {
        *existing = field;
    } else {
        fields.push(field);
    }
}

fn collect_dts_nodes(
    interfaces: &BTreeMap<String, InterfaceDecl>,
) -> Result<BTreeMap<String, Vec<DtsField>>, Box<dyn Error>> {
    let mut nodes = BTreeMap::<String, Vec<DtsField>>::new();
    for (interface_name, decl) in interfaces {
        let Some(kind_field) = decl.fields.iter().find(|field| field.name == "kind") else {
            continue;
        };
        let kinds = syntax_kinds_from_type(&kind_field.type_text);
        if kinds.is_empty() {
            continue;
        }
        let fields = collect_interface_fields(interface_name, interfaces, &mut Vec::new())?;
        let fields: Vec<DtsField> = fields
            .into_iter()
            .filter(|field| field.name != "kind")
            .collect();
        for kind in kinds {
            nodes.entry(kind).or_insert_with(|| fields.clone());
        }
    }
    Ok(nodes)
}

fn seed_token_payload_nodes(nodes: &mut BTreeMap<String, Vec<DtsField>>) {
    for kind in ["Identifier", "PrivateIdentifier"] {
        nodes.entry(kind.to_owned()).or_insert_with(|| {
            vec![
                DtsField {
                    name: "escapedText".to_owned(),
                    type_text: "__String".to_owned(),
                    optional: false,
                },
                DtsField {
                    name: "text".to_owned(),
                    type_text: "string".to_owned(),
                    optional: false,
                },
            ]
        });
    }

    for kind in [
        "StringLiteral",
        "NumericLiteral",
        "BigIntLiteral",
        "RegularExpressionLiteral",
        "NoSubstitutionTemplateLiteral",
        "JsxText",
    ] {
        nodes.entry(kind.to_owned()).or_insert_with(|| {
            vec![DtsField {
                name: "text".to_owned(),
                type_text: "string".to_owned(),
                optional: false,
            }]
        });
    }

    for kind in ["TemplateHead", "TemplateMiddle", "TemplateTail"] {
        nodes.entry(kind.to_owned()).or_insert_with(|| {
            vec![
                DtsField {
                    name: "text".to_owned(),
                    type_text: "string".to_owned(),
                    optional: false,
                },
                DtsField {
                    name: "rawText".to_owned(),
                    type_text: "string".to_owned(),
                    optional: true,
                },
            ]
        });
    }
}

fn seed_fieldless_nodes(nodes: &mut BTreeMap<String, Vec<DtsField>>) {
    for kind in ["DebuggerStatement", "EmptyStatement", "OmittedExpression"] {
        nodes.entry(kind.to_owned()).or_default();
    }
}

/// isTypeOnly/isExportEquals: grammar bits the JS-file walker (and later
/// import elision) reads; seeded explicitly because the dts payload
/// extraction does not surface them.
fn seed_grammar_flag_fields(nodes: &mut BTreeMap<String, Vec<DtsField>>) {
    for kind in [
        "ImportClause",
        "ExportDeclaration",
        "ImportSpecifier",
        "ExportSpecifier",
    ] {
        nodes.entry(kind.to_owned()).or_default().push(DtsField {
            name: "isTypeOnly".to_owned(),
            type_text: "boolean".to_owned(),
            optional: false,
        });
    }
    nodes
        .entry("ExportAssignment".to_owned())
        .or_default()
        .push(DtsField {
            name: "isExportEquals".to_owned(),
            type_text: "boolean".to_owned(),
            optional: true,
        });
    // tsc PrefixUnaryExpression.operator / PostfixUnaryExpression.operator:
    // a SyntaxKind payload, not a child node. The binder consumes it
    // (getDeclarationName signed-numeric computed names, strict-mode
    // ++/-- checks, createFlowMutation).
    for kind in ["PrefixUnaryExpression", "PostfixUnaryExpression"] {
        nodes.entry(kind.to_owned()).or_default().push(DtsField {
            name: "operator".to_owned(),
            type_text: "SyntaxKind".to_owned(),
            optional: false,
        });
    }
}

fn collect_interface_fields(
    interface_name: &str,
    interfaces: &BTreeMap<String, InterfaceDecl>,
    stack: &mut Vec<String>,
) -> Result<Vec<DtsField>, Box<dyn Error>> {
    if stack.iter().any(|name| name == interface_name) {
        return Ok(Vec::new());
    }
    let Some(decl) = interfaces.get(interface_name) else {
        return Ok(Vec::new());
    };

    stack.push(interface_name.to_owned());
    let mut fields = Vec::new();
    for base in &decl.bases {
        for field in collect_interface_fields(base, interfaces, stack)? {
            merge_dts_field(&mut fields, field);
        }
    }
    for field in &decl.fields {
        merge_dts_field(&mut fields, field.clone());
    }
    stack.pop();
    Ok(fields)
}

fn syntax_kinds_from_type(type_text: &str) -> Vec<String> {
    let mut kinds = Vec::new();
    let mut rest = type_text;
    while let Some(pos) = rest.find("SyntaxKind.") {
        let start = pos + "SyntaxKind.".len();
        let after = &rest[start..];
        let len = after
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .map(char::len_utf8)
            .sum::<usize>();
        if len > 0 {
            kinds.push(after[..len].to_owned());
        }
        rest = &after[len..];
    }
    kinds
}

fn merge_node_schema(
    child_table: BTreeMap<String, Vec<ChildVisit>>,
    dts_nodes: BTreeMap<String, Vec<DtsField>>,
) -> Vec<NodeSchema> {
    let mut schemas = BTreeMap::<String, NodeSchema>::new();
    for (kind_name, dts_fields) in dts_nodes {
        let children = child_table.get(&kind_name).cloned().unwrap_or_default();
        schemas.insert(
            kind_name.clone(),
            build_node_schema(kind_name, dts_fields, children),
        );
    }
    for (kind_name, children) in child_table {
        schemas.entry(kind_name.clone()).or_insert_with(|| {
            let dts_fields = children
                .iter()
                .map(|child| DtsField {
                    name: child.name.clone(),
                    type_text: match child.kind {
                        ChildKind::Node => "Node".to_owned(),
                        ChildKind::Nodes => "NodeArray<Node>".to_owned(),
                    },
                    optional: true,
                })
                .collect();
            build_node_schema(kind_name, dts_fields, children)
        });
    }
    schemas.into_values().collect()
}

fn build_node_schema(
    kind_name: String,
    dts_fields: Vec<DtsField>,
    children: Vec<ChildVisit>,
) -> NodeSchema {
    let mut fields = Vec::new();
    for dts_field in dts_fields {
        let child = children.iter().find(|child| child.name == dts_field.name);
        let ty = if let Some(child) = child {
            match child.kind {
                ChildKind::Node => RustFieldType::Node,
                ChildKind::Nodes => RustFieldType::NodeArray,
            }
        } else {
            rust_field_type(&dts_field.type_text)
        };
        let optional = dts_field.optional;
        fields.push(SchemaField {
            rust_name: rust_field_name(&dts_field.name),
            ts_name: dts_field.name,
            ty,
            optional,
            child: child.is_some(),
        });
    }
    for child in &children {
        if fields.iter().all(|field| field.ts_name != child.name) {
            fields.push(SchemaField {
                ts_name: child.name.clone(),
                rust_name: rust_field_name(&child.name),
                ty: match child.kind {
                    ChildKind::Node => RustFieldType::Node,
                    ChildKind::Nodes => RustFieldType::NodeArray,
                },
                optional: true,
                child: true,
            });
        }
    }

    NodeSchema {
        data_name: format!("{}Data", kind_name),
        kind_name,
        fields,
        children,
    }
}

fn rust_field_type(type_text: &str) -> RustFieldType {
    if type_text.contains("NodeArray<") {
        RustFieldType::NodeArray
    } else if type_text.contains("boolean") {
        RustFieldType::Bool
    } else if type_text.contains("string") || type_text.contains("__String") {
        RustFieldType::String
    } else if type_text.contains("number") {
        RustFieldType::Number
    } else if type_text.contains("SyntaxKind") {
        RustFieldType::SyntaxKind
    } else if type_text.contains("Node")
        || type_text.contains("Expression")
        || type_text.contains("Declaration")
        || type_text.contains("Identifier")
        || type_text.contains("Token")
        || type_text.contains("Type")
        || type_text.contains("Statement")
        || type_text.contains("Clause")
        || type_text.contains("Element")
        || type_text.contains("Literal")
        || type_text.contains("Name")
    {
        RustFieldType::Node
    } else {
        RustFieldType::Payload
    }
}

fn rust_field_name(ts_name: &str) -> String {
    let snake = snake_case(ts_name);
    match snake.as_str() {
        "type" | "default" | "abstract" | "final" | "box" | "move" | "ref" | "use" => {
            format!("r#{snake}")
        }
        _ => snake,
    }
}

fn snake_case(name: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (idx, ch) in chars.iter().copied().enumerate() {
        if !ch.is_ascii_alphanumeric() {
            if !out.ends_with('_') {
                out.push('_');
            }
            continue;
        }
        if idx > 0 && ch.is_ascii_uppercase() {
            let prev = chars[idx - 1];
            let next = chars.get(idx + 1).copied();
            let splits_word = (prev.is_ascii_lowercase() || prev.is_ascii_digit())
                || (prev.is_ascii_uppercase() && next.is_some_and(|c| c.is_ascii_lowercase()));
            if splits_word && !out.ends_with('_') {
                out.push('_');
            }
        }
        out.push(ch.to_ascii_lowercase());
    }
    out.trim_matches('_').to_owned()
}

fn render_nodes_rs(schemas: &[NodeSchema]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen nodes`. Do not edit by hand."
    )?;
    writeln!(out)?;
    writeln!(out, "use crate::SyntaxKind;")?;
    writeln!(out)?;
    writeln!(
        out,
        "#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]"
    )?;
    writeln!(out, "pub struct NodeId(pub u32);")?;
    writeln!(out)?;
    writeln!(
        out,
        "#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]"
    )?;
    writeln!(out, "pub struct NodeArrayId(pub u32);")?;
    writeln!(out)?;
    writeln!(out, "#[derive(Clone, Debug, Eq, PartialEq)]")?;
    writeln!(out, "pub struct NodeArray {{")?;
    writeln!(out, "    pub nodes: Vec<NodeId>,")?;
    writeln!(out, "    pub pos: u32,")?;
    writeln!(out, "    pub end: u32,")?;
    writeln!(out, "    pub has_trailing_comma: bool,")?;
    writeln!(out, "    /// tsc createMissingList's isMissingList marker.")?;
    writeln!(out, "    pub is_missing_list: bool,")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "#[derive(Clone, Debug, PartialEq)]")?;
    writeln!(out, "pub enum NodePayload {{")?;
    writeln!(out, "    Bool(bool),")?;
    writeln!(out, "    String(String),")?;
    writeln!(out, "    Number(f64),")?;
    writeln!(out, "    Kind(SyntaxKind),")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "#[derive(Clone, Debug, PartialEq)]")?;
    writeln!(out, "pub struct Node {{")?;
    writeln!(out, "    pub kind: SyntaxKind,")?;
    writeln!(out, "    pub flags: i32,")?;
    writeln!(out, "    pub pos: u32,")?;
    writeln!(out, "    pub end: u32,")?;
    writeln!(out, "    pub parent: Option<NodeId>,")?;
    writeln!(out, "    pub data: NodeData,")?;
    writeln!(out, "}}")?;
    writeln!(out)?;

    for schema in schemas {
        writeln!(out, "#[derive(Clone, Debug, PartialEq)]")?;
        writeln!(out, "pub struct {} {{", schema.data_name)?;
        for field in &schema.fields {
            writeln!(
                out,
                "    pub {}: {},",
                field.rust_name,
                render_field_type(field)
            )?;
        }
        writeln!(out, "}}")?;
        writeln!(out)?;
    }

    writeln!(out, "#[derive(Clone, Debug, PartialEq)]")?;
    writeln!(out, "pub enum NodeData {{")?;
    writeln!(out, "    Token,")?;
    for schema in schemas {
        writeln!(out, "    {}({}),", schema.kind_name, schema.data_name)?;
    }
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl NodeData {{")?;
    writeln!(out, "    pub const fn kind(&self) -> Option<SyntaxKind> {{")?;
    writeln!(out, "        match self {{")?;
    writeln!(out, "            Self::Token => None,")?;
    for schema in schemas {
        writeln!(
            out,
            "            Self::{}(_) => Some(SyntaxKind::{}),",
            schema.kind_name, schema.kind_name
        )?;
    }
    writeln!(out, "        }}")?;
    writeln!(out, "    }}")?;
    writeln!(out)?;
    writeln!(out, "    pub fn missing(kind: SyntaxKind) -> Self {{")?;
    writeln!(out, "        match kind {{")?;
    for schema in schemas {
        writeln!(
            out,
            "            SyntaxKind::{} => Self::{}({} {{",
            schema.kind_name, schema.kind_name, schema.data_name
        )?;
        for field in &schema.fields {
            writeln!(
                out,
                "                {}: {},",
                field.rust_name,
                render_missing_field_value(field, &schema.kind_name)
            )?;
        }
        writeln!(out, "            }}),")?;
    }
    writeln!(out, "            _ => Self::Token,")?;
    writeln!(out, "        }}")?;
    writeln!(out, "    }}")?;
    for schema in schemas {
        let accessor = format!("as_{}", snake_case(&schema.kind_name));
        writeln!(out)?;
        writeln!(
            out,
            "    pub fn {}(&self) -> Option<&{}> {{",
            accessor, schema.data_name
        )?;
        writeln!(out, "        match self {{")?;
        writeln!(
            out,
            "            Self::{}(data) => Some(data),",
            schema.kind_name
        )?;
        writeln!(out, "            _ => None,")?;
        writeln!(out, "        }}")?;
        writeln!(out, "    }}")?;
    }
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "#[cfg(test)]")?;
    writeln!(out, "mod tests {{")?;
    writeln!(out, "    use super::*;")?;
    writeln!(out)?;
    writeln!(out, "    #[test]")?;
    writeln!(out, "    fn generated_node_schema_has_core_nodes() {{")?;
    writeln!(out, "        assert_eq!(NodeData::Token.kind(), None);")?;
    writeln!(
        out,
        "        let _ = IdentifierData {{ escaped_text: String::new(), text: String::new() }};"
    )?;
    writeln!(
        out,
        "        assert_eq!(NodeData::missing(SyntaxKind::Identifier).kind(), Some(SyntaxKind::Identifier));"
    )?;
    writeln!(
        out,
        "        assert_eq!(NodeData::missing(SyntaxKind::SemicolonToken).kind(), None);"
    )?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;

    Ok(out)
}

fn render_missing_field_value(field: &SchemaField, kind_name: &str) -> String {
    if field.optional {
        return "None".to_owned();
    }

    match field.ty {
        RustFieldType::Node => "NodeId::default()".to_owned(),
        RustFieldType::NodeArray => "NodeArrayId::default()".to_owned(),
        RustFieldType::Bool => "false".to_owned(),
        RustFieldType::String => "String::new()".to_owned(),
        RustFieldType::Number => "0.0".to_owned(),
        RustFieldType::SyntaxKind => format!("SyntaxKind::{kind_name}"),
        RustFieldType::Payload => "NodePayload::String(String::new())".to_owned(),
    }
}

fn render_field_type(field: &SchemaField) -> String {
    let base = match field.ty {
        RustFieldType::Node => "NodeId",
        RustFieldType::NodeArray => "NodeArrayId",
        RustFieldType::Bool => "bool",
        RustFieldType::String => "String",
        RustFieldType::Number => "f64",
        RustFieldType::SyntaxKind => "SyntaxKind",
        RustFieldType::Payload => "NodePayload",
    };
    if field.optional {
        format!("Option<{base}>")
    } else {
        base.to_owned()
    }
}

fn render_for_each_child_rs(schemas: &[NodeSchema]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen nodes`. Do not edit by hand."
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "use crate::nodes::{{Node, NodeArray, NodeArrayId, NodeData, NodeId}};"
    )?;
    writeln!(out)?;
    writeln!(out, "pub trait NodeLookup {{")?;
    writeln!(
        out,
        "    fn node_array(&self, id: NodeArrayId) -> &NodeArray;"
    )?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(
        out,
        "pub fn for_each_child<L, F>(lookup: &L, node: &Node, mut cb: F) -> Option<NodeId>"
    )?;
    writeln!(out, "where")?;
    writeln!(out, "    L: NodeLookup,")?;
    writeln!(out, "    F: FnMut(NodeId) -> bool,")?;
    writeln!(out, "{{")?;
    writeln!(out, "    match &node.data {{")?;
    writeln!(out, "        NodeData::Token => None,")?;
    for schema in schemas {
        if schema.children.is_empty() {
            writeln!(
                out,
                "        NodeData::{}(_data) => None,",
                schema.kind_name
            )?;
        } else {
            writeln!(out, "        NodeData::{}(data) => {{", schema.kind_name)?;
            for child in &schema.children {
                let field = schema
                    .fields
                    .iter()
                    .find(|field| field.ts_name == child.name)
                    .ok_or_else(|| format!("missing generated field for child {}", child.name))?;
                let helper = match (child.kind, field.optional) {
                    (ChildKind::Node, false) => "visit_node",
                    (ChildKind::Node, true) => "visit_optional_node",
                    (ChildKind::Nodes, false) => "visit_nodes",
                    (ChildKind::Nodes, true) => "visit_optional_nodes",
                };
                if child.kind == ChildKind::Node {
                    writeln!(
                        out,
                        "            if let Some(result) = {}(data.{}, &mut cb) {{ return Some(result); }}",
                        helper, field.rust_name
                    )?;
                } else {
                    writeln!(
                        out,
                        "            if let Some(result) = {}(lookup, data.{}, &mut cb) {{ return Some(result); }}",
                        helper, field.rust_name
                    )?;
                }
            }
            writeln!(out, "            None")?;
            writeln!(out, "        }}")?;
        }
    }
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(
        out,
        "fn visit_node<F>(id: NodeId, cb: &mut F) -> Option<NodeId>"
    )?;
    writeln!(
        out,
        "where F: FnMut(NodeId) -> bool {{ if cb(id) {{ Some(id) }} else {{ None }} }}"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "fn visit_optional_node<F>(id: Option<NodeId>, cb: &mut F) -> Option<NodeId>"
    )?;
    writeln!(
        out,
        "where F: FnMut(NodeId) -> bool {{ id.and_then(|id| visit_node(id, cb)) }}"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "fn visit_nodes<L, F>(lookup: &L, id: NodeArrayId, cb: &mut F) -> Option<NodeId>"
    )?;
    writeln!(out, "where L: NodeLookup, F: FnMut(NodeId) -> bool {{")?;
    writeln!(out, "    for node in &lookup.node_array(id).nodes {{")?;
    writeln!(out, "        if cb(*node) {{ return Some(*node); }}")?;
    writeln!(out, "    }}")?;
    writeln!(out, "    None")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "fn visit_optional_nodes<L, F>(lookup: &L, id: Option<NodeArrayId>, cb: &mut F) -> Option<NodeId>")?;
    writeln!(out, "where L: NodeLookup, F: FnMut(NodeId) -> bool {{ id.and_then(|id| visit_nodes(lookup, id, cb)) }}")?;
    Ok(out)
}

fn render_nodes_schema_json(schemas: &[NodeSchema]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(out, "{{")?;
    writeln!(out, "  \"schema\": 1,")?;
    writeln!(out, "  \"nodes\": [")?;
    for (idx, schema) in schemas.iter().enumerate() {
        writeln!(out, "    {{")?;
        writeln!(out, "      \"kindName\": {:?},", schema.kind_name)?;
        writeln!(out, "      \"dataName\": {:?},", schema.data_name)?;
        writeln!(out, "      \"fields\": [")?;
        for (field_idx, field) in schema.fields.iter().enumerate() {
            writeln!(
                out,
                "        {{\"name\": {:?}, \"rustName\": {:?}, \"type\": {:?}, \"optional\": {}, \"child\": {}}}{}",
                field.ts_name,
                field.rust_name,
                format!("{:?}", field.ty),
                field.optional,
                field.child,
                if field_idx + 1 == schema.fields.len() { "" } else { "," }
            )?;
        }
        writeln!(out, "      ],")?;
        writeln!(out, "      \"children\": [")?;
        for (child_idx, child) in schema.children.iter().enumerate() {
            writeln!(
                out,
                "        {{\"name\": {:?}, \"array\": {}}}{}",
                child.name,
                child.kind == ChildKind::Nodes,
                if child_idx + 1 == schema.children.len() {
                    ""
                } else {
                    ","
                }
            )?;
        }
        writeln!(out, "      ]")?;
        writeln!(
            out,
            "    }}{}",
            if idx + 1 == schemas.len() { "" } else { "," }
        )?;
    }
    writeln!(out, "  ]")?;
    writeln!(out, "}}")?;
    Ok(out)
}

#[derive(Clone, Debug)]
struct DiagnosticEntry {
    name: String,
    code: u32,
    category: String,
    text: String,
    reports_unnecessary: bool,
    reports_deprecated: bool,
    elided_in_compatibility_pyramid: bool,
}

#[derive(Clone, Debug)]
struct DiagnosticEntryFields {
    code: u32,
    category: String,
    reports_unnecessary: bool,
    reports_deprecated: bool,
    elided_in_compatibility_pyramid: bool,
}

fn codegen_diags(check: bool) -> Result<(), Box<dyn Error>> {
    let workspace = find_tsrs2_root()?;
    let path = workspace.join("vendor/typescript-6.0.3/lib/diagnosticMessages.json");
    let raw = fs::read_to_string(path)?;
    let mut entries = parse_diagnostic_catalog(&raw)?;

    entries.sort_by_key(|entry| entry.code);
    let gen_rs = rustfmt_text(&render_diags_gen(&entries)?)?;
    write_generated(&workspace.join("crates/diags/src/gen.rs"), &gen_rs, check)?;

    if check {
        println!("generated diagnostic messages are up to date");
    } else {
        println!("generated diagnostic messages");
    }

    Ok(())
}

fn parse_diagnostic_catalog(src: &str) -> Result<Vec<DiagnosticEntry>, Box<dyn Error>> {
    let mut json = JsonReader::new(src);
    json.ws();
    json.expect('{')?;
    let mut entries = Vec::new();

    loop {
        json.ws();
        if json.peek() == Some('}') {
            json.bump();
            break;
        }

        let text = json.string()?;
        json.ws();
        json.expect(':')?;
        json.ws();
        let fields = parse_diagnostic_entry(&mut json)?;
        entries.push(DiagnosticEntry {
            name: diagnostic_static_name(&text),
            code: fields.code,
            category: fields.category,
            text,
            reports_unnecessary: fields.reports_unnecessary,
            reports_deprecated: fields.reports_deprecated,
            elided_in_compatibility_pyramid: fields.elided_in_compatibility_pyramid,
        });

        json.ws();
        match json.bump() {
            Some(',') => continue,
            Some('}') => break,
            other => {
                return Err(
                    format!("expected ',' or '}}' after diagnostic entry, got {other:?}").into(),
                )
            }
        }
    }

    let mut names = BTreeMap::<String, u32>::new();
    for entry in &entries {
        if let Some(existing) = names.insert(entry.name.clone(), entry.code) {
            return Err(format!(
                "diagnostic static name collision: {} for codes {} and {}",
                entry.name, existing, entry.code
            )
            .into());
        }
    }

    Ok(entries)
}

fn parse_diagnostic_entry(
    json: &mut JsonReader<'_>,
) -> Result<DiagnosticEntryFields, Box<dyn Error>> {
    json.expect('{')?;
    let mut code = None;
    let mut category = None;
    let mut reports_unnecessary = false;
    let mut reports_deprecated = false;
    let mut elided = false;

    loop {
        json.ws();
        if json.peek() == Some('}') {
            json.bump();
            break;
        }

        let key = json.string()?;
        json.ws();
        json.expect(':')?;
        json.ws();
        match key.as_str() {
            "code" => code = Some(json.number()? as u32),
            "category" => category = Some(json.string()?),
            "reportsUnnecessary" => reports_unnecessary = json.boolean()?,
            "reportsDeprecated" => reports_deprecated = json.boolean()?,
            "elidedInCompatabilityPyramid" => elided = json.boolean()?,
            _ => json.skip_value()?,
        }
        json.ws();
        match json.bump() {
            Some(',') => continue,
            Some('}') => break,
            other => {
                return Err(
                    format!("expected ',' or '}}' in diagnostic entry, got {other:?}").into(),
                )
            }
        }
    }

    Ok(DiagnosticEntryFields {
        code: code.ok_or("diagnostic entry missing code")?,
        category: category.ok_or("diagnostic entry missing category")?,
        reports_unnecessary,
        reports_deprecated,
        elided_in_compatibility_pyramid: elided,
    })
}

fn render_diags_gen(entries: &[DiagnosticEntry]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by `cargo xtask codegen diags`. Do not edit by hand."
    )?;
    writeln!(out)?;
    writeln!(out, "use super::{{DiagnosticCategory, DiagnosticMessage}};")?;
    writeln!(out)?;

    for entry in entries {
        writeln!(
            out,
            "pub static {}: DiagnosticMessage = DiagnosticMessage {{",
            entry.name
        )?;
        writeln!(out, "    code: {},", entry.code)?;
        writeln!(out, "    category: DiagnosticCategory::{},", entry.category)?;
        writeln!(out, "    text: {:?},", entry.text)?;
        writeln!(
            out,
            "    reports_unnecessary: {},",
            entry.reports_unnecessary
        )?;
        writeln!(out, "    reports_deprecated: {},", entry.reports_deprecated)?;
        writeln!(
            out,
            "    elided_in_compatibility_pyramid: {},",
            entry.elided_in_compatibility_pyramid
        )?;
        writeln!(out, "}};")?;
    }

    writeln!(out)?;
    writeln!(
        out,
        "pub static ALL_BY_CODE: &[(u32, &DiagnosticMessage)] = &["
    )?;
    for entry in entries {
        writeln!(out, "    ({}, &{}),", entry.code, entry.name)?;
    }
    writeln!(out, "];")?;
    writeln!(out)?;
    writeln!(out, "#[cfg(test)]")?;
    writeln!(out, "mod tests {{")?;
    writeln!(out, "    use super::*;")?;
    writeln!(out)?;
    writeln!(out, "    #[test]")?;
    writeln!(out, "    fn generated_diagnostic_pins_match_tsc() {{")?;
    writeln!(
        out,
        "        assert_eq!(Unterminated_string_literal.code, 1002);"
    )?;
    writeln!(out, "        assert_eq!(_0_expected.code, 1005);")?;
    writeln!(
        out,
        "        assert_eq!(ALL_BY_CODE.len(), {});",
        entries.len()
    )?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;

    Ok(out)
}

fn diagnostic_static_name(message: &str) -> String {
    let mut out = String::new();
    let mut previous_was_separator = false;

    for ch in message.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            previous_was_separator = false;
        } else if !previous_was_separator {
            out.push('_');
            previous_was_separator = true;
        }
    }

    let mut out = out.trim_matches('_').to_owned();
    if out.is_empty() {
        out = "Diagnostic".to_owned();
    }
    if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if is_rust_keyword(&out) {
        out.insert_str(0, "r#");
    }
    out
}

struct JsonReader<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> JsonReader<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            bytes: src.as_bytes(),
            index: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.bytes.get(self.index).copied().map(char::from)
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek();
        if ch.is_some() {
            self.index += 1;
        }
        ch
    }

    fn expect(&mut self, expected: char) -> Result<(), Box<dyn Error>> {
        match self.bump() {
            Some(actual) if actual == expected => Ok(()),
            actual => Err(format!(
                "expected {expected:?}, got {actual:?} at byte {}",
                self.index
            )
            .into()),
        }
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.index += 1;
        }
    }

    fn string(&mut self) -> Result<String, Box<dyn Error>> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            let ch = self.bump().ok_or("unterminated JSON string")?;
            match ch {
                '"' => return Ok(out),
                '\\' => {
                    let escaped = self.bump().ok_or("unterminated JSON escape")?;
                    match escaped {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        'b' => out.push('\u{0008}'),
                        'f' => out.push('\u{000C}'),
                        'u' => {
                            let mut hex = String::new();
                            for _ in 0..4 {
                                hex.push(self.bump().ok_or("short JSON unicode escape")?);
                            }
                            let code = u32::from_str_radix(&hex, 16)?;
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        other => return Err(format!("unknown JSON escape \\{other}").into()),
                    }
                }
                _ if ch.is_ascii() => out.push(ch),
                _ => {
                    self.index -= 1;
                    let rest = std::str::from_utf8(&self.bytes[self.index..])?;
                    let decoded = rest.chars().next().ok_or("invalid UTF-8 in JSON string")?;
                    self.index += decoded.len_utf8();
                    out.push(decoded);
                }
            }
        }
    }

    fn number(&mut self) -> Result<i64, Box<dyn Error>> {
        let start = self.index;
        while matches!(self.peek(), Some('-' | '+' | '0'..='9')) {
            self.index += 1;
        }
        Ok(std::str::from_utf8(&self.bytes[start..self.index])?.parse()?)
    }

    fn boolean(&mut self) -> Result<bool, Box<dyn Error>> {
        if self.bytes[self.index..].starts_with(b"true") {
            self.index += 4;
            Ok(true)
        } else if self.bytes[self.index..].starts_with(b"false") {
            self.index += 5;
            Ok(false)
        } else {
            Err(format!("expected JSON boolean at byte {}", self.index).into())
        }
    }

    fn skip_value(&mut self) -> Result<(), Box<dyn Error>> {
        self.ws();
        match self.peek() {
            Some('"') => {
                self.string()?;
            }
            Some('{') => self.skip_balanced('{', '}')?,
            Some('[') => self.skip_balanced('[', ']')?,
            Some('t') | Some('f') => {
                self.boolean()?;
            }
            Some('n') if self.bytes[self.index..].starts_with(b"null") => {
                self.index += 4;
            }
            Some('-' | '+' | '0'..='9') => {
                self.number()?;
            }
            other => {
                return Err(
                    format!("unexpected JSON value {other:?} at byte {}", self.index).into(),
                )
            }
        }
        Ok(())
    }

    fn skip_balanced(&mut self, open: char, close: char) -> Result<(), Box<dyn Error>> {
        self.expect(open)?;
        let mut depth = 1usize;
        while depth > 0 {
            match self.bump() {
                Some('"') => {
                    self.index -= 1;
                    self.string()?;
                }
                Some(ch) if ch == open => depth += 1,
                Some(ch) if ch == close => depth -= 1,
                Some(_) => {}
                None => return Err("unterminated JSON container".into()),
            }
        }
        Ok(())
    }
}
