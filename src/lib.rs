//! tsrs — a TypeScript type checker in Rust that reproduces tsc diagnostics
//! byte-for-byte (plain `--pretty false` format) for a curated subset of the
//! language, with the complete diagnosticMessages.json catalog embedded.

pub mod ast;
pub mod binder;
pub mod checker;
pub mod diagnostics;
pub mod flow_graph;
pub mod harness;
pub mod js_num;
pub mod jsstr;
pub mod options;
pub mod output;
pub mod parser;
pub mod scanner;
pub mod text;
pub mod types;

pub const LIB_TEXT: &str = include_str!("../lib/lib.tsrs.d.ts");
pub const LIB_NAME: &str = "lib.tsrs.d.ts";

use options::CompilerOptions;
use text::SourceText;

pub struct InputFile {
    /// Display name (as given on the command line / fixture).
    pub name: String,
    pub text: String,
}

/// tsc getNormalizedAbsolutePath for POSIX paths: combine with `cwd` when
/// relative, resolve "." / ".." segments, collapse duplicate separators.
pub(crate) fn normalize_path(path: &str, cwd: &str) -> String {
    let combined = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("{}/{}", cwd.trim_end_matches('/'), path)
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in combined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    format!("/{}", parts.join("/"))
}

/// Extensions a root file may have without `allowJs`/`allowNonTsExtensions`,
/// in tsc's `flatten(supportedExtensions)` order (the TS6054/TS6231 list).
const SUPPORTED_EXTENSIONS: [&str; 7] =
    [".ts", ".tsx", ".d.ts", ".cts", ".d.cts", ".mts", ".d.mts"];
/// Extensions tried for an extension-less root (tsc `supportedExtensions[0]`).
const EXTENSIONLESS_TRIES: [&str; 3] = [".ts", ".tsx", ".d.ts"];

fn supported_extensions_display() -> String {
    SUPPORTED_EXTENSIONS
        .iter()
        .map(|e| format!("'{e}'"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// File-less program diagnostic with tsc's file-explaining chain for roots:
///   <head>
///     The file is in the program because:
///       Root file specified for compilation
fn root_file_diagnostic(
    msg: &'static diagnostics::DiagnosticMessage,
    args: &[String],
) -> diagnostics::Diagnostic {
    let mut because = diagnostics::MessageChain::new(
        &diagnostics::gen::The_file_is_in_the_program_because_Colon,
        &[],
    );
    because.next.push(diagnostics::MessageChain::new(
        &diagnostics::gen::Root_file_specified_for_compilation,
        &[],
    ));
    let mut head = diagnostics::MessageChain::new(msg, args);
    head.next.push(because);
    diagnostics::Diagnostic {
        file: None,
        start: 0,
        length: 0,
        message: head,
        related: Vec::new(),
    }
}

/// File-less TS2688 with tsc's type-library file-inclusion chain:
///   Cannot find type definition file for '{0}'.
///     The file is in the program because:
///       Entry point of type library '{0}' specified in compilerOptions
/// A wildcard `types: ["*"]` makes the named entries implicit instead
/// (TS1420, `Entry point for implicit type library '{0}'`), matching tsc's
/// getAutomaticTypeDirectiveNames.
#[allow(dead_code)]
fn type_library_diagnostic(name: &str, implicit: bool) -> diagnostics::Diagnostic {
    let reason = if implicit {
        diagnostics::MessageChain::new(
            &diagnostics::gen::Entry_point_for_implicit_type_library_0,
            &[name.to_string()],
        )
    } else {
        diagnostics::MessageChain::new(
            &diagnostics::gen::Entry_point_of_type_library_0_specified_in_compilerOptions,
            &[name.to_string()],
        )
    };
    let mut because = diagnostics::MessageChain::new(
        &diagnostics::gen::The_file_is_in_the_program_because_Colon,
        &[],
    );
    because.next.push(reason);
    let mut head = diagnostics::MessageChain::new(
        &diagnostics::gen::Cannot_find_type_definition_file_for_0,
        &[name.to_string()],
    );
    head.next.push(because);
    diagnostics::Diagnostic {
        file: None,
        start: 0,
        length: 0,
        message: head,
        related: Vec::new(),
    }
}

/// tsc getSourceFileFromReferenceWorker over the root names: returns the
/// program files (in root order, deduplicated case-insensitively) plus the
/// file-less diagnostics for roots that don't make it in. `lookup` is the
/// host read (disk for the CLI, the fixture map for in-memory checks).
fn resolve_root_files(
    root_names: &[String],
    mut lookup: impl FnMut(&str) -> Option<String>,
) -> (Vec<InputFile>, Vec<diagnostics::Diagnostic>) {
    let mut files: Vec<InputFile> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut diags = Vec::new();
    let include = |name: &str,
                   text: String,
                   files: &mut Vec<InputFile>,
                   seen: &mut std::collections::HashSet<String>| {
        if seen.insert(name.to_lowercase()) {
            files.push(InputFile {
                name: name.to_string(),
                text,
            });
        }
    };
    for name in root_names {
        let base = name.rsplit('/').next().unwrap_or(name);
        if base.contains('.') {
            let lower = name.to_lowercase();
            if !SUPPORTED_EXTENSIONS.iter().any(|e| lower.ends_with(e)) {
                if [".js", ".jsx", ".mjs", ".cjs"]
                    .iter()
                    .any(|e| lower.ends_with(e))
                {
                    diags.push(root_file_diagnostic(
                        &diagnostics::gen::File_0_is_a_JavaScript_file_Did_you_mean_to_enable_the_allowJs_option,
                        &[name.clone()],
                    ));
                } else {
                    diags.push(root_file_diagnostic(
                        &diagnostics::gen::File_0_has_an_unsupported_extension_The_only_supported_extensions_are_1,
                        &[name.clone(), supported_extensions_display()],
                    ));
                }
                continue;
            }
            match lookup(name) {
                Some(text) => include(name, text, &mut files, &mut seen),
                None => diags.push(root_file_diagnostic(
                    &diagnostics::gen::File_0_not_found,
                    &[name.clone()],
                )),
            }
        } else {
            let mut found = false;
            for ext in EXTENSIONLESS_TRIES {
                let candidate = format!("{name}{ext}");
                if let Some(text) = lookup(&candidate) {
                    include(&candidate, text, &mut files, &mut seen);
                    found = true;
                    break;
                }
            }
            if !found {
                diags.push(root_file_diagnostic(
                    &diagnostics::gen::Could_not_resolve_the_path_0_with_the_extensions_Colon_1,
                    &[name.clone(), supported_extensions_display()],
                ));
            }
        }
    }
    (files, diags)
}

/// Check a program given as in-memory files. Returns (stdout bytes, exit code).
/// The embedded lib is prepended unless a root is already named lib.tsrs.d.ts.
pub fn check_program(inputs: Vec<InputFile>, options: &CompilerOptions) -> (String, i32) {
    check_program_with_roots(inputs, &[], options)
}

/// `check_program` plus extra root names that have no materialized content
/// (fixture `@extraRootFiles:`) — they resolve against the input set the way
/// CLI roots resolve against the disk.
pub fn check_program_with_roots(
    mut inputs: Vec<InputFile>,
    extra_roots: &[String],
    options: &CompilerOptions,
) -> (String, i32) {
    if !inputs
        .iter()
        .any(|f| f.name == LIB_NAME || f.name.ends_with("/lib.tsrs.d.ts"))
    {
        inputs.insert(
            0,
            InputFile {
                name: LIB_NAME.to_string(),
                text: LIB_TEXT.to_string(),
            },
        );
    }
    let map: std::collections::HashMap<String, String> = inputs
        .iter()
        .map(|f| (f.name.to_lowercase(), f.text.clone()))
        .collect();
    let root_names: Vec<String> = inputs
        .iter()
        .map(|f| f.name.clone())
        .chain(extra_roots.iter().cloned())
        .collect();
    // non-consuming lookup: like the disk, the same name resolves repeatedly
    // (an extension-less root may re-find an already-included file — dedupe
    // happens in resolve_root_files)
    let (files, root_diags) =
        resolve_root_files(&root_names, |name| map.get(&name.to_lowercase()).cloned());
    check_program_core(
        files.into_iter().map(|f| (f.name, f.text)).collect(),
        root_diags,
        options,
    )
}

/// A `/// <reference … />` triple-slash directive recognized in a file's
/// leading comment block.
struct RefDirective {
    /// Byte offset of the opening `//` of the comment.
    comment_pos: u32,
    /// Byte length of the single-line comment.
    comment_len: u32,
    kind: RefDirectiveKind,
}

enum RefDirectiveKind {
    /// `path="…"` — value text plus the byte offset of its first character
    /// (inside the quotes), matching tsc's captured argument span.
    Path { value: String, value_pos: u32 },
    /// A `<reference …/>` with none of path/types/lib/no-default-lib → TS1084.
    Invalid,
    /// `<amd-module name="…"/>` — the `name` argument's value. A second (or
    /// later) amd-module pragma with a non-empty name is TS2458.
    AmdModule { name: String },
    /// `types`/`lib`/`no-default-lib` directives — accepted, no diagnostic.
    Other,
}

fn is_pragma_ws(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' || b == 0x0c || b == 0x0b
}

/// tsc getNamedArgRegEx: `(\sNAME\s*=\s*)(?:'([^']*)'|"([^"]*)")`, case-insensitive,
/// first match in `comment`. Returns (match_start, group1_len, value).
fn match_named_arg(comment: &str, name: &str) -> Option<(usize, usize, String)> {
    let b = comment.as_bytes();
    let n = b.len();
    let nm = name.as_bytes();
    let mut i = 0;
    while i < n {
        if is_pragma_ws(b[i]) {
            let k = i + 1;
            let matches_name = k + nm.len() <= n
                && b[k..k + nm.len()]
                    .iter()
                    .zip(nm)
                    .all(|(c, d)| c.eq_ignore_ascii_case(d));
            if matches_name {
                let mut p = k + nm.len();
                while p < n && is_pragma_ws(b[p]) {
                    p += 1;
                }
                if p < n && b[p] == b'=' {
                    p += 1;
                    while p < n && is_pragma_ws(b[p]) {
                        p += 1;
                    }
                    if p < n && (b[p] == b'"' || b[p] == b'\'') {
                        let quote = b[p];
                        let vstart = p + 1;
                        let mut e = vstart;
                        while e < n && b[e] != quote {
                            e += 1;
                        }
                        if e < n {
                            // group1 = `\sNAME\s*=\s*`, spanning i..p (the quote).
                            return Some((i, p - i, comment[vstart..e].to_string()));
                        }
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// Parse a single-line comment as a `/// <reference … />` directive, mirroring
/// tsc's tripleSlashXMLCommentStartRegEx + extractPragmas + processPragmasIntoFields.
/// `comment_pos` is the byte offset of the comment's `//`.
fn parse_reference_comment(comment: &str, comment_pos: u32) -> Option<RefDirective> {
    let b = comment.as_bytes();
    // `^///` then `\s*` then `<`.
    if b.len() < 3 || &b[0..3] != b"///" {
        return None;
    }
    let mut i = 3;
    while i < b.len() && is_pragma_ws(b[i]) {
        i += 1;
    }
    if i >= b.len() || b[i] != b'<' {
        return None;
    }
    i += 1;
    // Tag name `\S+` followed by `\s` (whitespace required after the name).
    let name_start = i;
    while i < b.len() && !is_pragma_ws(b[i]) {
        i += 1;
    }
    if i == name_start || i >= b.len() {
        return None;
    }
    let tag = &comment[name_start..i];
    let is_reference = tag.eq_ignore_ascii_case("reference");
    let is_amd_module = tag.eq_ignore_ascii_case("amd-module");
    if !is_reference && !is_amd_module {
        return None;
    }
    // The `.*?/>` tail: a self-closing `/>` must appear.
    if !comment[i..].contains("/>") {
        return None;
    }
    let comment_len = comment.len() as u32;
    if is_amd_module {
        // amd-module's only pragma arg is the required `name`; with no `name`
        // the pragma is not extracted at all (extractPragmas returns).
        return match_named_arg(comment, "name").map(|(_, _, name)| RefDirective {
            comment_pos,
            comment_len,
            kind: RefDirectiveKind::AmdModule { name },
        });
    }
    // processPragmasIntoFields order: no-default-lib, then types, lib, path,
    // else TS1084.
    if let Some((_, _, v)) = match_named_arg(comment, "no-default-lib") {
        if v == "true" {
            return Some(RefDirective {
                comment_pos,
                comment_len,
                kind: RefDirectiveKind::Other,
            });
        }
    }
    if match_named_arg(comment, "types").is_some() || match_named_arg(comment, "lib").is_some() {
        return Some(RefDirective {
            comment_pos,
            comment_len,
            kind: RefDirectiveKind::Other,
        });
    }
    if let Some((mi, g1, value)) = match_named_arg(comment, "path") {
        return Some(RefDirective {
            comment_pos,
            comment_len,
            kind: RefDirectiveKind::Path {
                value_pos: comment_pos + (mi + g1 + 1) as u32,
                value,
            },
        });
    }
    Some(RefDirective {
        comment_pos,
        comment_len,
        kind: RefDirectiveKind::Invalid,
    })
}

/// Reference directives in the leading comment block (tsc collects pragmas from
/// the comments leading file position 0 only — a directive after the first token
/// is ignored).
fn scan_reference_directives(text: &str) -> Vec<RefDirective> {
    let b = text.as_bytes();
    let n = b.len();
    let mut i = 0;
    let mut out = Vec::new();
    loop {
        while i < n && is_pragma_ws(b[i]) {
            i += 1;
        }
        if i + 1 < n && b[i] == b'/' && b[i + 1] == b'/' {
            let start = i;
            let mut j = i + 2;
            while j < n && b[j] != b'\n' && b[j] != b'\r' {
                j += 1;
            }
            if let Some(d) = parse_reference_comment(&text[start..j], start as u32) {
                out.push(d);
            }
            i = j;
        } else if i + 1 < n && b[i] == b'/' && b[i + 1] == b'*' {
            let mut j = i + 2;
            while j + 1 < n && !(b[j] == b'*' && b[j + 1] == b'/') {
                j += 1;
            }
            i = if j + 1 < n { j + 2 } else { n };
        } else {
            break;
        }
    }
    out
}

/// Resolve a reference `path` value relative to the referencing file's
/// directory, returning a program-relative name comparable to the input file
/// names (tsc canonicalizes; the harness file set is case-insensitive).
fn resolve_ref_path(referencing: &str, path: &str) -> String {
    let dir = match referencing.rfind('/') {
        Some(idx) => &referencing[..idx],
        None => "",
    };
    let abs = normalize_path(path, &format!("/{dir}"));
    abs.trim_start_matches('/').to_string()
}

/// An inclusion reason for a program file, mirroring tsc's FileIncludeReason
/// (only the two kinds reachable here: a root name, or a `/// <reference>`).
enum IncludeReason {
    Root,
    /// `spec` is the raw reference text (inside the quotes), `from` the parsed
    /// index of the referencing file, `pos`/`len` the path-value span.
    Ref {
        spec: String,
        from: usize,
        pos: u32,
        len: u32,
    },
}

/// tsc forceConsistentCasingInFileNames (default on in TS 6.0): when a file is
/// pulled into the program twice under names differing only in casing,
/// getSourceFileFromReferenceWorker reports TS1149 / TS1261. We simulate tsc's
/// processRootFile depth-first inclusion — roots in harness order, each file's
/// path `/// <reference>`s processed (DFS) the first time the file is added —
/// and record a diagnostic at every casing collision. The selection follows
/// reportFileNamesDifferOnlyInCasingError: the new inclusion via a root while
/// the existing file already has a referenced reason is TS1261 (the args
/// swapped), otherwise TS1149. The "file is in the program because:" chain and
/// the diagnostic location derive from the FINAL accumulated reasons (tsc
/// builds them lazily in getCombinedDiagnostics), so every collision on a file
/// prints its complete reason list.
fn detect_casing_conflicts(files: &[(String, String)]) -> Vec<diagnostics::Diagnostic> {
    use std::collections::HashMap;
    struct Entry {
        original: String,
        reasons: Vec<IncludeReason>,
    }
    struct Event {
        code: &'static diagnostics::DiagnosticMessage,
        args: Vec<String>,
        canonical: String,
        /// Some when the triggering reason is itself a reference (location is
        /// that reference); None for a root trigger (location falls back to the
        /// first referenced reason in the final list, or file-less).
        trigger_loc: Option<(usize, u32, u32)>,
    }

    // Harness files by canonical (lower-cased) name -> parsed index.
    let mut harness: HashMap<String, usize> = HashMap::new();
    for (i, (name, _)) in files.iter().enumerate() {
        harness.entry(name.to_lowercase()).or_insert(i);
    }

    let mut program: Vec<Entry> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut events: Vec<Event> = Vec::new();

    // Recursive inclusion (explicit work via a recursive helper closure is
    // awkward in Rust, so use an inner fn threading the mutable state).
    fn include(
        name: &str,
        reason: IncludeReason,
        files: &[(String, String)],
        harness: &HashMap<String, usize>,
        program: &mut Vec<Entry>,
        index: &mut HashMap<String, usize>,
        events: &mut Vec<Event>,
    ) {
        let canonical = name.to_lowercase();
        if let Some(&pi) = index.get(&canonical) {
            let existing_original = program[pi].original.clone();
            let trigger_loc = match &reason {
                IncludeReason::Ref { from, pos, len, .. } => Some((*from, *pos, *len)),
                IncludeReason::Root => None,
            };
            let referenced = trigger_loc.is_some();
            program[pi].reasons.push(reason);
            if existing_original != *name {
                // Differs only in casing (same canonical, different bytes).
                let existing_has_ref = program[pi]
                    .reasons
                    .iter()
                    .any(|r| matches!(r, IncludeReason::Ref { .. }));
                let (code, args) = if !referenced && existing_has_ref {
                    (
                        &diagnostics::gen::Already_included_file_name_0_differs_from_file_name_1_only_in_casing,
                        vec![existing_original, name.to_string()],
                    )
                } else {
                    (
                        &diagnostics::gen::File_name_0_differs_from_already_included_file_name_1_only_in_casing,
                        vec![name.to_string(), existing_original],
                    )
                };
                events.push(Event {
                    code,
                    args,
                    canonical,
                    trigger_loc,
                });
            }
            return;
        }
        index.insert(canonical.clone(), program.len());
        program.push(Entry {
            original: name.to_string(),
            reasons: vec![reason],
        });
        // Process this file's path references depth-first (only on first add).
        if let Some(&hi) = harness.get(&canonical) {
            for d in scan_reference_directives(&files[hi].1) {
                if let RefDirectiveKind::Path { value, value_pos } = d.kind {
                    let candidate = resolve_ref_path(&files[hi].0, &value);
                    if harness.contains_key(&candidate.to_lowercase()) {
                        let len = value.len() as u32;
                        include(
                            &candidate,
                            IncludeReason::Ref {
                                spec: value,
                                from: hi,
                                pos: value_pos,
                                len,
                            },
                            files,
                            harness,
                            program,
                            index,
                            events,
                        );
                    }
                }
            }
        }
    }

    for (name, _) in files {
        if name == LIB_NAME || name.ends_with("/lib.tsrs.d.ts") {
            continue;
        }
        include(
            name,
            IncludeReason::Root,
            files,
            &harness,
            &mut program,
            &mut index,
            &mut events,
        );
    }

    let mut out = Vec::new();
    for ev in events {
        let entry = &program[index[&ev.canonical]];
        let loc = ev.trigger_loc.or_else(|| {
            entry.reasons.iter().find_map(|r| match r {
                IncludeReason::Ref { from, pos, len, .. } => Some((*from, *pos, *len)),
                IncludeReason::Root => None,
            })
        });
        let mut because = diagnostics::MessageChain::new(
            &diagnostics::gen::The_file_is_in_the_program_because_Colon,
            &[],
        );
        for r in &entry.reasons {
            because.next.push(match r {
                IncludeReason::Root => diagnostics::MessageChain::new(
                    &diagnostics::gen::Root_file_specified_for_compilation,
                    &[],
                ),
                IncludeReason::Ref { spec, from, .. } => diagnostics::MessageChain::new(
                    &diagnostics::gen::Referenced_via_0_from_file_1,
                    &[spec.clone(), files[*from].0.clone()],
                ),
            });
        }
        let mut head = diagnostics::MessageChain::new(ev.code, &ev.args);
        head.next.push(because);
        let (file, start, length) = match loc {
            Some((fi, pos, len)) => (Some(fi), pos, len),
            None => (None, 0, 0),
        };
        out.push(diagnostics::Diagnostic {
            file,
            start,
            length,
            message: head,
            related: Vec::new(),
        });
    }
    out
}

fn check_program_core(
    files: Vec<(String, String)>,
    root_diags: Vec<diagnostics::Diagnostic>,
    options: &CompilerOptions,
) -> (String, i32) {
    // Parse every file; tsc gating: any syntactic diagnostic suppresses all
    // option/global/semantic diagnostics.
    let mut parsed: Vec<(String, SourceText, ast::SourceFileAst)> = Vec::new();
    let mut syntactic: Vec<diagnostics::Diagnostic> = root_diags;
    for (i, (name, text)) in files.iter().enumerate() {
        let st = SourceText::new(text.clone());
        let jsx = name.ends_with(".tsx");
        let (file_ast, mut diags) = parser::parse_with_jsx(&st.text, i, jsx);
        syntactic.append(&mut diags);
        parsed.push((name.clone(), st, file_ast));
    }

    // Triple-slash `/// <reference path="…" />` directives. tsc extracts these
    // pragmas from the leading comment block of every file (processCommentPragmas
    // → processPragmasIntoFields → getSourceFileFromReferenceWorker). A malformed
    // `<reference …/>` with no recognized argument is TS1084 (a *parse*
    // diagnostic, so it gates semantics like other syntactic errors); a `path`
    // resolving to the referencing file itself is TS1006 and one resolving to no
    // file is TS6053 — both located on the path value and reported alongside
    // semantics (they do not gate). Other reference kinds (types/lib) and valid
    // references to existing files are accepted silently; tsrs takes the harness
    // file set as the whole program, so no new files are pulled in here.
    let mut ref_program_diags: Vec<diagnostics::Diagnostic> = Vec::new();
    for (fi, (name, _text)) in files.iter().enumerate() {
        if name == LIB_NAME || name.ends_with("/lib.tsrs.d.ts") {
            continue;
        }
        // TS2458: a file may carry at most one `<amd-module name="…"/>` pragma;
        // every later one (with a non-empty name) is reported at its comment
        // start (processPragmasIntoFields, a *parse* diagnostic → gates).
        let mut amd_module_named = false;
        for directive in scan_reference_directives(&files[fi].1) {
            match directive.kind {
                RefDirectiveKind::AmdModule { name } => {
                    if amd_module_named {
                        syntactic.push(diagnostics::Diagnostic {
                            file: Some(fi),
                            start: directive.comment_pos,
                            length: directive.comment_len,
                            message: diagnostics::MessageChain::new(
                                &diagnostics::gen::An_AMD_module_cannot_have_multiple_name_assignments,
                                &[],
                            ),
                        related: Vec::new(),
                        });
                    }
                    if !name.is_empty() {
                        amd_module_named = true;
                    }
                }
                RefDirectiveKind::Invalid => {
                    syntactic.push(diagnostics::Diagnostic {
                        file: Some(fi),
                        start: directive.comment_pos,
                        length: directive.comment_len,
                        message: diagnostics::MessageChain::new(
                            &diagnostics::gen::Invalid_reference_directive_syntax,
                            &[],
                        ),
                        related: Vec::new(),
                    });
                }
                RefDirectiveKind::Path { value, value_pos } => {
                    let candidate = resolve_ref_path(name, &value);
                    if candidate.eq_ignore_ascii_case(name) {
                        ref_program_diags.push(diagnostics::Diagnostic {
                            file: Some(fi),
                            start: value_pos,
                            length: value.len() as u32,
                            message: diagnostics::MessageChain::new(
                                &diagnostics::gen::A_file_cannot_have_a_reference_to_itself,
                                &[],
                            ),
                            related: Vec::new(),
                        });
                    } else if !files
                        .iter()
                        .any(|(n, _)| n.eq_ignore_ascii_case(&candidate))
                    {
                        ref_program_diags.push(diagnostics::Diagnostic {
                            file: Some(fi),
                            start: value_pos,
                            length: value.len() as u32,
                            message: diagnostics::MessageChain::new(
                                &diagnostics::gen::File_0_not_found,
                                &[value.clone()],
                            ),
                            related: Vec::new(),
                        });
                    }
                }
                RefDirectiveKind::Other => {}
            }
        }
    }
    // TS1149/TS1261: files included twice under names differing only in casing
    // (forceConsistentCasingInFileNames, default on). Same bucket as the other
    // reference-directive program diagnostics above.
    ref_program_diags.extend(detect_casing_conflicts(&files));
    let diags: Vec<diagnostics::Diagnostic> = if !syntactic.is_empty() {
        syntactic
    } else {
        // options diagnostics gate semantic output (tsc getOptionsDiagnostics)
        let opt_diags = crate::options::check_options(options);
        if !opt_diags.is_empty() {
            opt_diags
        } else {
            let mut bound = binder::bind(&parsed);
            binder::run_function_impl_checks(&mut bound);
            let mut diags = checker::check(&parsed, options, bound);
            // TS6131: --outFile with module left unset, against the first
            // non-ambient external-module file (semantic bucket, span on the
            // externalModuleIndicator — like TS1148 below). A SET module that
            // isn't amd/system is TS6082 in check_options instead.
            if options.out_file.is_some()
                && !options.emit_declaration_only
                && options.module.is_none()
            {
                let indicator = parsed
                    .iter()
                    .enumerate()
                    .find_map(|(fi, (name, _st, ast))| {
                        if name.ends_with(".d.ts") {
                            return None;
                        }
                        module_indicator_span(&ast.stmts).map(|sp| (fi, sp))
                    });
                if let Some((fi, sp)) = indicator {
                    diags.push(diagnostics::Diagnostic {
                        file: Some(fi),
                        start: sp.start,
                        length: sp.end - sp.start,
                        message: diagnostics::MessageChain::new(
                            &diagnostics::gen::Cannot_compile_modules_using_option_0_unless_the_module_flag_is_amd_or_system,
                            &["outFile".to_string()],
                        ),
                        related: Vec::new(),
                    });
                }
            }
            // TS1148: explicit --module none + pre-ES2015 target + module
            // syntax. tsc adds this once, file-bound, in the semantic bucket
            // (it does NOT gate); isolatedModules/verbatim divert to TS5047.
            if options.module.as_deref() == Some("none")
                && options.script_target_rank() < 2
                && !options.isolated_modules
                && !options.verbatim_module_syntax
            {
                let indicator = parsed
                    .iter()
                    .enumerate()
                    .find_map(|(fi, (name, _st, ast))| {
                        if name.ends_with(".d.ts") {
                            return None;
                        }
                        module_indicator_span(&ast.stmts).map(|sp| (fi, sp))
                    });
                if let Some((fi, sp)) = indicator {
                    diags.push(diagnostics::Diagnostic {
                        file: Some(fi),
                        start: sp.start,
                        length: sp.end - sp.start,
                        message: diagnostics::MessageChain::new(
                            &diagnostics::gen::Cannot_use_imports_exports_or_module_augmentations_when_module_is_none,
                            &[],
                        ),
                        related: Vec::new(),
                    });
                }
            }
            // @ts-expect-error / @ts-ignore: suppress next-line diagnostics
            for (fi, (_n, st, ast)) in parsed.iter().enumerate() {
                for &(dstart, dend, expect) in &ast.comment_directives {
                    let dline = st.line_col(dstart).0;
                    let before = diags.len();
                    diags.retain(|d| {
                        if d.file != Some(fi) {
                            return true;
                        }
                        st.line_col(d.start).0 != dline + 1
                    });
                    if expect && diags.len() == before {
                        diags.push(diagnostics::Diagnostic {
                            file: Some(fi),
                            start: dstart,
                            length: dend - dstart,
                            message: diagnostics::MessageChain::new(
                                &diagnostics::gen::Unused_ts_expect_error_directive,
                                &[],
                            ),
                            related: Vec::new(),
                        });
                    }
                }
            }
            // Reference-directive program diagnostics (TS1006/TS6053) print
            // with the semantic bucket and sort by position alongside it.
            diags.extend(ref_program_diags);
            diags
        }
    };
    let files: Vec<(String, SourceText)> = parsed.into_iter().map(|(n, t, _)| (n, t)).collect();

    let paths: Vec<String> = files.iter().map(|(n, _)| n.to_lowercase()).collect();
    let sorted = diagnostics::sort_and_dedupe(diags, &paths);
    let out_files: Vec<output::OutputFile> = files
        .iter()
        .map(|(n, t)| output::OutputFile {
            display_name: n.clone(),
            text: t,
        })
        .collect();
    let out = if options.diag_json {
        output::format_diagnostics_json(&sorted, &out_files)
    } else {
        output::format_diagnostics(&sorted, &out_files)
    };
    // tsc 6.0 CLI exits 2 (DiagnosticsPresent_OutputsGenerated) when any
    // diagnostics exist under --noEmit, 0 otherwise (measured empirically).
    let exit = if sorted.is_empty() { 0 } else { 2 };
    (out, exit)
}

/// First statement that makes a file an external module (tsc's
/// externalModuleIndicator), for the TS1148 span.
fn module_indicator_span(stmts: &[ast::Stmt]) -> Option<text::Span> {
    use ast::{has_modifier, ModifierKind, Stmt};
    stmts.iter().find_map(|s| match s {
        Stmt::Import(d) => Some(d.span),
        Stmt::ExportNamed(d) => Some(d.span),
        Stmt::ExportDefault { span, .. } | Stmt::ExportAssign { span, .. } => Some(*span),
        Stmt::Var(v) if has_modifier(&v.modifiers, ModifierKind::Export) => Some(v.span),
        Stmt::Func(f) if has_modifier(&f.modifiers, ModifierKind::Export) => Some(f.span),
        Stmt::Class(c) if has_modifier(&c.modifiers, ModifierKind::Export) => Some(c.span),
        Stmt::Interface(i) if has_modifier(&i.modifiers, ModifierKind::Export) => Some(i.span),
        Stmt::TypeAlias(t) if has_modifier(&t.modifiers, ModifierKind::Export) => Some(t.span),
        Stmt::Enum(e) if has_modifier(&e.modifiers, ModifierKind::Export) => Some(e.span),
        _ => None,
    })
}

/// Full CLI pipeline: parse argv tsc-style (response files, enum-value and
/// locale validation — command-line errors print and exit 1 without
/// compiling), then resolve root names against `read` (missing → TS6053,
/// unsupported extension → TS6054, extension-less probing → TS6231) and type
/// check. `cwd_display` is the absolute current directory used wherever a
/// resolved path is embedded in a message.
pub fn run_command_line(
    args: &[String],
    mut read: impl FnMut(&str) -> Option<String>,
    cwd_display: &str,
) -> (String, i32) {
    // Harness-only flag: strip `--diag-json` before tsc-style parsing.
    let diag_json = args.iter().any(|a| a == "--diag-json");
    let args: Vec<String> = args
        .iter()
        .filter(|a| *a != "--diag-json")
        .cloned()
        .collect();
    let args = &args[..];
    let mut parsed = options::parse_command_line(args, &mut read);
    parsed.options.diag_json = diag_json;
    if !parsed.errors.is_empty() {
        // tsc executeCommandLine: report in parse order (no sorting), then
        // exit(ExitStatus.DiagnosticsPresent_OutputsSkipped)
        let out = output::format_diagnostics(&parsed.errors, &[]);
        return (out, 1);
    }
    parsed.options.current_directory = Some(cwd_display.to_string());
    let (mut inputs, root_diags) = resolve_root_files(&parsed.files, read);
    if !inputs
        .iter()
        .any(|f| f.name == LIB_NAME || f.name.ends_with("/lib.tsrs.d.ts"))
    {
        inputs.insert(
            0,
            InputFile {
                name: LIB_NAME.to_string(),
                text: LIB_TEXT.to_string(),
            },
        );
    }
    check_program_core(
        inputs.into_iter().map(|f| (f.name, f.text)).collect(),
        root_diags,
        &parsed.options,
    )
}

#[cfg(test)]
mod tests {
    use super::{check_program, InputFile, LIB_NAME};
    use crate::options::CompilerOptions;

    #[test]
    fn namespace_body_this_is_reported_without_poisoning_class_this() {
        let opts = CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: "namespace M {\n  var x = this;\n  class C { m() { return this; } }\n}\n"
                        .to_string(),
                },
            ],
            &opts,
        );

        assert!(out.contains("main.ts(2,11): error TS2331"));
        assert!(out.contains("main.ts(2,11): error TS2683"));
        assert!(!out.contains("main.ts(3,27): error TS2331"));
        assert!(!out.contains("main.ts(3,27): error TS2683"));
    }

    #[test]
    fn class_field_function_expression_this_cuts_class_this() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: "class C {\n  x = function () { return this; }\n  y = () => this;\n}\n"
                    .to_string(),
            }],
            &opts,
        );

        assert!(out.contains("main.ts(2,28): error TS2683"), "{out}");
        assert!(!out.contains("main.ts(3,13): error TS2683"), "{out}");
    }

    #[test]
    fn untyped_explicit_this_parameter_suppresses_no_implicit_this() {
        let opts = CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: "function outer() {\n  return function (this) { return this; };\n}\n"
                    .to_string(),
            }],
            &opts,
        );

        assert!(!out.contains("error TS2683"), "{out}");
    }

    #[test]
    fn super_member_this_return_uses_derived_receiver() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: "class Base {\n  returnThis() { return this; }\n  fn() {}\n}\nclass Derived extends Base {\n  returnThis() { return super.returnThis(); }\n}\nnew Derived().returnThis().fn();\n".to_string(),
            }],
            &opts,
        );

        assert!(!out.contains("error TS2416"), "{out}");
        assert!(!out.contains("error TS2339"), "{out}");
    }

    #[test]
    fn generic_class_this_return_preserves_accessor_receiver_type() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: "class C<T, U> {\n  x!: T;\n  get y() { return null; }\n  set y(v: U) { }\n  fn() { return this; }\n}\nlet c = new C(1, \"\");\nlet r = c.fn();\nr.y = \"\";\n".to_string(),
            }],
            &opts,
        );

        assert!(!out.contains("main.ts(9,1): error TS2322"), "{out}");
    }

    #[test]
    fn identity_mapper_does_not_force_lazy_method_returns() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: "class C1<T, U, V> {\n  constructor(private k: T, private [a, b, c]: [T, U, V]) {}\n  getA() { return this.a; }\n  getB() { return this.b; }\n}\n".to_string(),
            }],
            &opts,
        );

        assert!(!out.contains("error TS7023"), "{out}");
    }

    #[test]
    fn identity_mapper_keeps_readonly_generic_assignment_related() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: "class SampleClass<P> {\n  public props: Readonly<P>;\n  constructor(props: P) {\n    this.props = Object.freeze(props);\n  }\n}\n".to_string(),
            }],
            &opts,
        );

        assert!(!out.contains("error TS2719"), "{out}");
    }

    #[test]
    fn direct_nullish_values_report_18050_in_operator_contexts() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: "let a = null * 1;\nlet b = 1 * undefined;\nlet c = null < 1;\nlet d = null in {};\nlet e = 1 + null;\nlet f = '' + null;\n"
                    .to_string(),
            }],
            &opts,
        );

        assert!(out.contains("main.ts(1,9): error TS18050"), "{out}");
        assert!(out.contains("main.ts(2,13): error TS18050"), "{out}");
        assert!(out.contains("main.ts(3,9): error TS18050"), "{out}");
        assert!(out.contains("main.ts(4,9): error TS18050"), "{out}");
        assert!(out.contains("main.ts(5,13): error TS18050"), "{out}");
        assert!(!out.contains("main.ts(6,14): error TS18050"), "{out}");
    }

    #[test]
    fn non_strict_addition_keeps_2365_for_nullish_operands() {
        let opts = CompilerOptions {
            strict: Some(false),
            strict_null_checks: Some(false),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: "let a = 1 + null;\nlet b = null + undefined;\nlet c = null * undefined;\n"
                    .to_string(),
            }],
            &opts,
        );

        assert!(out.contains("main.ts(1,9): error TS2365"), "{out}");
        assert!(out.contains("main.ts(2,9): error TS2365"), "{out}");
        assert!(out.contains("main.ts(3,9): error TS18050"), "{out}");
        assert!(out.contains("main.ts(3,16): error TS18050"), "{out}");
        assert!(!out.contains("main.ts(1,13): error TS18050"), "{out}");
        assert!(!out.contains("main.ts(2,9): error TS18050"), "{out}");
        assert!(!out.contains("main.ts(2,16): error TS18050"), "{out}");
    }

    #[test]
    fn implicit_any_suggestions_emit_when_no_implicit_any_is_off() {
        let opts = CompilerOptions {
            strict: Some(false),
            diag_json: true,
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: "let v;\nfunction f(x);\nfunction f(x) { return x; }\nclass C { x; m(p) { return p; } }\ninterface I { y; m(q); (r); new(s); }\n"
                    .to_string(),
            }],
            &opts,
        );

        for code in [7043, 7044, 7045, 7050] {
            assert!(
                out.contains(&format!("\"code\":{code},\"category\":2")),
                "{out}"
            );
        }
        for code in [7006, 7008, 7010, 7013, 7020] {
            assert!(!out.contains(&format!("\"code\":{code},")), "{out}");
        }
    }

    #[test]
    fn type_literal_members_report_implicit_any_suggestions() {
        let opts = CompilerOptions {
            strict: Some(false),
            diag_json: true,
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: "let a: { x; class; };\n".to_string(),
            }],
            &opts,
        );

        assert_eq!(
            out.matches("\"code\":7045,\"category\":2").count(),
            2,
            "{out}"
        );
        assert!(
            out.contains("Member 'x' implicitly has an 'any' type"),
            "{out}"
        );
        assert!(
            out.contains("Member 'class' implicitly has an 'any' type"),
            "{out}"
        );
    }

    #[test]
    fn computed_type_member_methods_report_implicit_any_return_names() {
        let opts = CompilerOptions {
            strict: Some(false),
            diag_json: true,
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: "interface I { [Symbol.iterator](); }\n".to_string(),
            }],
            &opts,
        );

        assert!(out.contains("\"code\":7050,\"category\":2"), "{out}");
        assert!(
            out.contains("'[Symbol.iterator]' implicitly has an 'any' return type"),
            "{out}"
        );

        let opts = CompilerOptions {
            strict: Some(true),
            diag_json: true,
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: "interface I { [Symbol.iterator](); }\n".to_string(),
            }],
            &opts,
        );

        assert!(out.contains("\"code\":7010,\"category\":1"), "{out}");
        assert!(
            out.contains("'[Symbol.iterator]', which lacks return-type annotation"),
            "{out}"
        );
    }

    #[test]
    fn computed_object_literal_setters_report_implicit_any() {
        let opts = CompilerOptions {
            strict: Some(false),
            diag_json: true,
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: "let n: number; let o = { set [n](value) {} };\n".to_string(),
            }],
            &opts,
        );

        assert!(out.contains("\"code\":7044,\"category\":2"), "{out}");
        assert!(out.contains("\"code\":7032,\"category\":2"), "{out}");
        assert!(
            out.contains("Property '[n]' implicitly has type 'any'"),
            "{out}"
        );
    }

    #[test]
    fn computed_object_members_use_index_signature_context() {
        let opts = CompilerOptions {
            strict: Some(true),
            diag_json: true,
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: r#"
interface I {
    [s: string]: (x: string) => number;
    [s: number]: (x: any) => number;
}
let o: I = {
    ["" + 0](y) { return y.length; },
    ["" + 1]: y => y.length
};
"#
                .to_string(),
            }],
            &opts,
        );

        assert!(!out.contains("\"code\":7006"), "{out}");
    }

    #[test]
    fn later_es_member_suggestions_are_receiver_specific() {
        let opts = CompilerOptions {
            strict: Some(true),
            diag_json: true,
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: r#"
Symbol.dispose;
Symbol.asyncDispose;
RegExp.escape;
Promise.allSettled([]);
Object.fromEntries([]);
let xs: number[] = [];
xs.values();
declare const match: RegExpExecArray;
match.groups;
declare const plain: {};
plain.includes;
"#
                .to_string(),
            }],
            &opts,
        );

        assert_eq!(
            out.matches("\"code\":2550,\"category\":1").count(),
            6,
            "{out}"
        );
        assert!(
            out.contains("Property 'includes' does not exist on type '{}'"),
            "{out}"
        );
        assert!(
            !out.contains("Property 'includes' does not exist on type '{}'. Do you need"),
            "{out}"
        );
    }

    #[test]
    fn implicit_any_errors_remain_errors_when_no_implicit_any_is_on() {
        let opts = CompilerOptions {
            strict: Some(true),
            diag_json: true,
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: "function f(x);\nfunction f(x) { return x; }\nclass C { x; m(p) { return p; } }\ninterface I { y; m(q); (r); new(s); }\n"
                    .to_string(),
            }],
            &opts,
        );

        for code in [7006, 7008, 7010, 7013, 7020] {
            assert!(
                out.contains(&format!("\"code\":{code},\"category\":1")),
                "{out}"
            );
        }
        for code in [7043, 7044, 7045, 7050] {
            assert!(!out.contains(&format!("\"code\":{code},")), "{out}");
        }
    }

    #[test]
    fn unused_type_parameters_report_as_suggestions() {
        let opts = CompilerOptions {
            strict: Some(true),
            diag_json: true,
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: r#"
interface A<T> {}
interface B<T> { x: T }
interface X<T, U> {}
interface Y<T, U> { u: U }
type Fn = <T>() => void;
type Obj = { m<T>(): void; new <T>(): unknown; <T>(): unknown };
interface Escaped<_T> {}
"#
                .to_string(),
            }],
            &opts,
        );

        assert_eq!(
            out.matches("\"code\":6205,\"category\":2").count(),
            1,
            "{out}"
        );
        assert!(out.contains("All type parameters are unused."), "{out}");
        assert_eq!(
            out.matches("\"code\":6133,\"category\":2").count(),
            6,
            "{out}"
        );
        assert!(
            out.contains("'T' is declared but its value is never read."),
            "{out}"
        );
        assert!(!out.contains("'_T' is declared"), "{out}");
    }

    #[test]
    fn unused_grouping_engine_mirrors_tsc_group_flush_rules() {
        let opts = CompilerOptions {
            no_unused_locals: true,
            no_unused_parameters: true,
            diag_json: true,
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: r#"
declare const src: any;
export function all_vars() {
    let a = 1, b = 2;
}
export function partial_pattern({ k: y, b }: any) {
    return b;
}
export function full_pattern() {
    const { c, d } = src;
}
export function rest_suppresses() {
    const { e, ...rest } = src;
}
export function regroup_single() {
    const { f } = src;
}
export function underscore_partial({ k: _x, g }: any) {
    return g;
}
"#
                .to_string(),
            }],
            &opts,
        );

        // let a, b — all unused → one 6199 at the statement, no per-name 6133
        assert!(out.contains("All variables are unused."), "{out}");
        assert!(!out.contains("'a' is declared"), "{out}");
        // {k: y, b} partial → per-element 6133 for y only
        assert!(
            out.contains("'y' is declared but its value is never read."),
            "{out}"
        );
        // {c, d} fully unused → 6198
        assert!(
            out.contains("All destructured elements are unused."),
            "{out}"
        );
        assert!(!out.contains("'c' is declared"), "{out}");
        // {e, ...rest}: e is suppressed by the trailing rest; rest reports
        assert!(!out.contains("'e' is declared"), "{out}");
        assert!(
            out.contains("'rest' is declared but its value is never read."),
            "{out}"
        );
        // single-element pattern regroups into its list: 6133 named 'f'
        assert!(
            out.contains("'f' is declared but its value is never read."),
            "{out}"
        );
        // {k: _x, g}: propertyName + underscore exempts _x, forcing the
        // per-element form — no 6198, no report for _x
        assert!(!out.contains("'_x' is declared"), "{out}");
    }

    #[test]
    fn single_name_unused_import_reports_6133_on_the_import_statement() {
        let opts = CompilerOptions {
            no_unused_locals: true,
            diag_json: true,
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: "mod.ts".to_string(),
                    text: "export const v1 = 1; export const v2 = 2;".to_string(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: "import { v1 } from \"./mod\";\nimport { v1 as r1, v2 as r2 } from \"./mod\";\nexport {};\n"
                        .to_string(),
                },
            ],
            &opts,
        );

        // one declared name → 6133 with the name, anchored at the statement
        assert!(
            out.contains("'v1' is declared but its value is never read."),
            "{out}"
        );
        // two declared names, all unused → 6192
        assert!(
            out.contains("All imports in import declaration are unused."),
            "{out}"
        );
        assert!(!out.contains("'r1' is declared"), "{out}");
    }

    #[test]
    fn await_using_of_in_for_await_header_is_not_parsed_as_await_expression() {
        let source = "declare const x: any[];\nfor await (await using of x);\n";
        let (ast, diags) = crate::parser::parse_with_jsx(source, 0, false);
        assert!(diags.is_empty(), "{diags:?}");
        match &ast.stmts[1] {
            crate::ast::Stmt::ForOf {
                left, await_span, ..
            } => {
                assert!(await_span.is_some());
                match &**left {
                    crate::ast::ForInit::Var(v) => assert!(v.decls.is_empty(), "{v:?}"),
                    other => panic!("{other:?}"),
                }
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn empty_await_using_declaration_list_reports_1123_not_empty_name_unused() {
        let opts = CompilerOptions {
            strict: Some(false),
            module: Some("esnext".to_string()),
            target: Some("esnext".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile { name: LIB_NAME.to_string(), text: String::new() },
                InputFile {
                    name: "main.ts".to_string(),
                    text: "declare const x: any[];\nexport async function test() { for await (await using of x); }\n".to_string(),
                },
            ],
            &opts,
        );

        assert!(out.contains("error TS1123"), "{out}");
        assert!(!out.contains("TS6133"), "{out}");
    }

    #[test]
    fn static_super_resolves_for_expression_bases() {
        let opts = CompilerOptions {
            target: Some("es2022".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: r#"
declare var dec: any;
@dec
class C1 extends class { } {
    static { super.name; }
}
@dec
class C2 extends (function() {} as any) {
    static { super.name; }
}
"#
                .to_string(),
            }],
            &opts,
        );

        assert!(!out.contains("main.ts"), "{out}");
    }

    #[test]
    fn classic_field_initializers_reject_constructor_scope_names() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
var x = 1;
class C {
    b = x;
    constructor(x: string) { x = 2; }
}
var y = 1;
class D {
    b = y;
    constructor() { var y = ""; }
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(out.contains("main.ts(4,9): error TS2301"), "{out}");
        assert!(out.contains("main.ts(9,9): error TS2301"), "{out}");
    }

    #[test]
    fn classic_field_type_queries_reject_constructor_scope_names_but_allow_this() {
        let opts = CompilerOptions {
            strict: Some(false),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
class C {
    b: typeof x;
    constructor(x) {}
}
class E {
    a = this.x;
    b: typeof this.x;
    constructor(public x) {}
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(out.contains("main.ts(3,15): error TS2844"), "{out}");
        assert!(!out.contains("Cannot find name 'this'"), "{out}");
        assert!(!out.contains("TS2729"), "{out}");
    }

    #[test]
    fn use_define_class_fields_do_not_use_constructor_scope_shadowing() {
        let opts = CompilerOptions {
            strict: Some(false),
            target: Some("esnext".to_string()),
            use_define_for_class_fields: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: "var x = 1;\nclass C { b = x; constructor(x: string) {} }\n".to_string(),
                },
            ],
            &opts,
        );

        assert!(!out.contains("TS2301"), "{out}");
    }

    #[test]
    fn lazy_field_initializer_uses_declaration_scope() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
function outer() {
    function f(x: number) {
        class C {
            public x = x;
        }
        return C;
    }
    let C = f(1);
    let v = new C();
    return v.x;
}
outer();
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(!out.contains("TS2448"), "{out}");
        assert!(!out.contains("TS2454"), "{out}");
    }

    #[test]
    fn ambient_declare_const_is_not_tdz_checked() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: "let r = o ?? 1;\nr;\ndeclare const o: number | undefined;\n".to_string(),
                },
            ],
            &opts,
        );

        assert!(!out.contains("TS2448"), "{out}");
        assert!(!out.contains("TS2454"), "{out}");
    }

    #[test]
    fn mixin_class_value_preserves_base_constructor_type() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: "interface Object {}\ninterface Array<T> { length: number; [n: number]: T; }\n".to_string(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
type Constructor<T = {}> = new (...args: any[]) => T;
class Base {
    baseMethod(): void {}
}
function Tagged<T extends Constructor<Base>>(superClass: T): T & Constructor<{ tag: string }> {
    class C extends superClass {
        tag: string;
        constructor(...args: any[]) {
            super(...args);
            this.tag = "ok";
        }
    }
    return C;
}
const Mixed = Tagged(Base);
const value = new Mixed();
value.baseMethod();
value.tag;
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(!out.contains("main.ts"), "{out}");
    }

    #[test]
    fn generic_class_expression_values_capture_outer_type_arguments() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
class A<T> {
    genericVar!: T;
}
function B1<U>() {
    return class extends A<U> { };
}
class B2<V> {
    anon = class extends A<V> { };
}
function B3<W>() {
    return class Inner<TInner> extends A<W> { };
}
class K extends B1<number>() { }
class C extends (new B2<number>().anon) { }
let b3Number = B3<number>();
class S extends b3Number<string> { }
new C().genericVar = 12;
new K().genericVar = 12;
new S().genericVar = 12;
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(!out.contains("TS2322"), "{out}");
        assert!(!out.contains("TS2339"), "{out}");
    }

    #[test]
    fn heritage_this_property_lookup_does_not_report_base_cycle() {
        let opts = CompilerOptions {
            strict: Some(false),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
class Base { }
class Derived extends Base {
    constructor() {
        class Inner extends this.memberClass { }
        super();
    }
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(!out.contains("TS2506"), "{out}");
    }

    #[test]
    fn mixin_protected_member_access_uses_constructor_return_base() {
        let opts = CompilerOptions {
            strict: Some(false),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
declare class C1 {
    public a: number;
    protected b: number;
    constructor(s: string);
}
declare class M1 {
    constructor(...args: any[]);
    p: number;
}
declare const Mixed: typeof M1 & typeof C1;
class C2 extends Mixed {
    constructor() {
        super("hello");
        this.a;
        this.b;
        this.p;
    }
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(!out.contains("TS2445"), "{out}");
    }

    #[test]
    fn overloaded_new_reports_only_first_argument_mismatch_for_selected_constructor() {
        let opts = CompilerOptions {
            strict: Some(false),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
interface Fn {
    new (s: string, n: number): number;
    new <T>(n: number, t: T): T;
}
declare var fn: Fn;
new fn<Date>("", 0);
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(out.contains("TS2345"), "{out}");
        assert!(out.contains("parameter of type 'number'"), "{out}");
        assert!(!out.contains("parameter of type 'Date'"), "{out}");
    }

    #[test]
    fn empty_object_satisfies_global_object_constraint() {
        let opts = CompilerOptions {
            strict: Some(false),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: "interface Object {}\ninterface Array<T> { length: number; [n: number]: T; }\n".to_string(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
type ClassInterface<C> = { [key in keyof C]: C[key] };
type InstanceInterface<I> = {
    new(...args: any[]): I;
    prototype: I;
};
type Constructor<I extends Object, C = any> = ClassInterface<C> & InstanceInterface<I>;
function cloneClass<T extends Constructor<{}>>(OriginalClass: T): T {
    class AnotherOriginalClass extends OriginalClass {
        constructor(...args: any[]) {
            super(...args);
        }
    }
    return AnotherOriginalClass;
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(!out.contains("TS2344"), "{out}");
        assert!(!out.contains("TS2322"), "{out}");
    }

    #[test]
    fn logical_or_and_nullish_on_type_params_stay_unwrapped_when_nonstrict() {
        let opts = CompilerOptions {
            strict: Some(false),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: "interface Object {}\ntype NonNullable<T> = T & {};\n".to_string(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
function f<T, U>(t: T, u: U) {
    var a: {} = t || u;
    var b: {} = t ?? u;
    var c: {} = t!;
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        // tsc's getNonNullableType is the identity without strictNullChecks,
        // so non-strict `t || u` / `t ?? u` stay `T | U` — no NonNullable<T>
        // wrap. (The remaining TS2322s pin tsrs's stricter non-strict
        // type-param → {} assignability; oracle tsc accepts all three lines.)
        assert!(!out.contains("NonNullable"), "{out}");
        assert!(
            out.contains("Type 'T | U' is not assignable to type '{}'."),
            "{out}"
        );
        assert!(!out.contains("main.ts(5,9): error TS2322"), "{out}");
    }

    #[test]
    fn definite_assignment_flow_query_2454() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: "interface Object {}\ninterface Function {}\ninterface Boolean {}\ninterface Number {}\ninterface String {}\ninterface RegExp {}\ninterface IArguments {}\ninterface Array<T> { length: number; [n: number]: T; }\n".to_string(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
declare const cond: boolean;
function loops() {
    let a: number;
    while (cond) { a = 1; }
    a;                        // 2454: zero-iteration path
    let b: number;
    while (true) { b = 1; break; }
    b;                        // ok: only the break edge reaches here
}
function seq() {
    let c: number;
    if (cond) { c = 1; }
    c;                        // 2454: else path unassigned
    let d: number;
    if (cond) { d = 1; } else { d = 2; }
    d;                        // ok: both branches assign
    let e: number;
    e!;                       // ok: non-null assertion asserts assignment
    (e)!;                     // 2454: parens break the exemption (tsc)
    let f: number;
    f **= 2;                  // 2454: compound assignment reads first
    let g: string | void;
    g;                        // 2454: void does not count as undefined
}
function exhaustive(x: "a" | "b") {
    let y: number;
    switch (x) {
        case "a": y = 1; break;
        case "b": y = 2; break;
    }
    y;                        // ok: exhaustive switch, no-match unreachable
}
function tryFlow() {
    let t: number;
    try { t = mayThrow(); } catch { }
    t;                        // 2454: the exception path skips the assignment
}
declare function mayThrow(): number;
"#
                    .to_string(),
                },
            ],
            &opts,
        );
        let count_2454 = out.matches("TS2454").count();
        assert!(out.contains("main.ts(6,5): error TS2454"), "{out}");
        assert!(out.contains("main.ts(14,5): error TS2454"), "{out}");
        assert!(out.contains("main.ts(20,6): error TS2454"), "{out}");
        assert!(out.contains("main.ts(22,5): error TS2454"), "{out}");
        assert!(out.contains("main.ts(24,5): error TS2454"), "{out}");
        assert!(out.contains("main.ts(37,5): error TS2454"), "{out}");
        assert_eq!(count_2454, 6, "{out}");
    }

    #[test]
    fn strict_property_initialization_flow_2564_2565() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: "interface Object {}\ninterface Function {}\ninterface Boolean {}\ninterface Number {}\ninterface String {}\ninterface RegExp {}\ninterface IArguments {}\ninterface Array<T> { length: number; [n: number]: T; }\n".to_string(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
declare const cond: boolean;
class A {
    a: number;               // 2564: never assigned
    b: number;               // ok: assigned unconditionally
    c: number;               // 2564: only one branch assigns
    d: number;               // ok: both branches assign
    e: number;               // ok: early return guards the fallthrough
    f: number;               // KNOWN DIVERGENCE: assigned in try-finally
                             // without catch — tsc's ReduceLabel drops the
                             // exceptional antecedents (no error); tsrs's
                             // end join keeps them, so 2564 fires here
    "s": number;             // ok: literal-named props are exempt
    u: number | undefined;   // ok: undefined-including type
    constructor() {
        const r1 = this.b;   // 2565: read before the assignment below
        this.b = 1;
        const r2 = this.b;   // ok
        if (cond) { this.c = 1; this.d = 1; } else { this.d = 2; }
        if (cond) { this.e = 1; return; }
        this.e = 2;
        try { this.f = 1; } finally { }
    }
    m(): number { return this.a; }  // ok: methods are not the constructor
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );
        assert!(out.contains("main.ts(4,5): error TS2564"), "{out}");
        assert!(out.contains("main.ts(6,5): error TS2564"), "{out}");
        // the third 2564 pins the try/finally ReduceLabel divergence on `f`
        assert!(out.contains("main.ts(9,5): error TS2564"), "{out}");
        assert_eq!(out.matches("TS2564").count(), 3, "{out}");
        assert!(out.contains("error TS2565: Property 'b'"), "{out}");
        assert_eq!(out.matches("TS2565").count(), 1, "{out}");
    }

    #[test]
    fn deferred_mapped_type_display_is_canonical() {
        let opts = CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
function f<T>(x: T) {
    let y: { [P in keyof T & string as `p_${P}`]: T[P] } = x;
}
function g<T>() {
    var x: { [P in keyof T]: T[P] };
    var x: { [P in keyof T]?: T[P] };
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(
            out.contains("Type 'T' is not assignable to type '{ [P in keyof T & string as `p_${P}`]: T[P]; }'."),
            "{out}"
        );
        assert!(
            !out.contains("{ [P in keyof T & string as `p_${P}`]: T[P] }"),
            "{out}"
        );
        assert!(
            out.contains("but here has type '{ [P in keyof T]?: T[P] | undefined; }'."),
            "{out}"
        );
    }

    #[test]
    fn deferred_mapped_relation_reports_key_constraint_child() {
        let opts = CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
function f<A extends string, B extends string, T, U>(
    x: { [P in B as `p_${P}`]: T }
) {
    let y: { [Q in A as `p_${Q}`]: U } = x;
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(
            out.contains("Type '{ [P in B as `p_${P}`]: T; }' is not assignable to type '{ [Q in A as `p_${Q}`]: U; }'."),
            "{out}"
        );
        assert!(
            out.contains("Type 'A' is not assignable to type 'B'."),
            "{out}"
        );
        assert!(
            out.contains("'A' is assignable to the constraint of type 'B'"),
            "{out}"
        );
    }

    #[test]
    fn keyof_type_parameter_relation_uses_property_key_constraint() {
        let opts = CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
function f<T extends { a: number }>(k: keyof T) {
    let onlyA: "a" = k;
}
function g<T extends { a: number }, K extends keyof T>(k: keyof T) {
    let narrowed: K = k;
}
function h<T, K extends keyof T>(k: keyof T, kt: K, s: string) {
    const a = s as keyof T;
    const b = k as string;
    const c = kt as string;
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(
            out.contains("Type 'string | number | symbol' is not assignable to type '\"a\"'."),
            "{out}"
        );
        assert!(
            out.contains("Type 'keyof T' is not assignable to type 'K'."),
            "{out}"
        );
        assert!(
            out.contains("different subtype of constraint 'string | number | symbol'"),
            "{out}"
        );
        assert!(!out.contains("TS2352"), "{out}");
    }

    #[test]
    fn source_keyof_relation_reports_effective_key_domain() {
        let opts = CompilerOptions {
            strict: Some(true),
            diag_json: true,
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
function f<T, K extends keyof T & string>(k: keyof T) {
    let refined: K = k;
    let concrete: string | number = k;
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(
            out.contains("Type 'keyof T' is not assignable to type 'K'."),
            "{out}"
        );
        assert!(
            out.contains("'K' could be instantiated with an arbitrary type which could be unrelated to 'keyof T'."),
            "{out}"
        );
        assert!(
            !out.contains("Type 'string | number | symbol' is not assignable to type 'K'."),
            "{out}"
        );
        assert!(
            out.contains(
                "Type 'string | number | symbol' is not assignable to type 'string | number'."
            ),
            "{out}"
        );
        assert!(
            out.contains("Type 'symbol' is not assignable to type 'string | number'."),
            "{out}"
        );
        assert!(
            !out.contains("Type 'keyof T' is not assignable to type 'string | number'."),
            "{out}"
        );
    }

    #[test]
    fn deferred_mapped_property_lookup_preserves_keyof_intersection_semantics() {
        let opts = CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: r#"
interface String {
    concat: any;
}
"#
                    .to_string(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
type MyReadonly<T> = { readonly [P in keyof T]: T[P] };
interface Foo { foo: string; }

function ok<T>(x: MyReadonly<T & Foo>) {
    x.foo.concat;
}

function bad<T extends { foo: number }>(x: MyReadonly<T & Foo>) {
    x.foo.concat;
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(!out.contains("Property 'foo' does not exist"), "{out}");
        assert!(out.contains("Property 'concat' does not exist"), "{out}");
    }

    #[test]
    fn abstract_property_initializer_reports_on_property_name() {
        let opts = CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
abstract class C {
    abstract prop = 1;
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(out.contains("main.ts(3,14): error TS1267"), "{out}");
        assert!(
            out.contains("Property 'prop' cannot have an initializer"),
            "{out}"
        );
    }

    #[test]
    fn class_member_kind_override_diagnostics_match_accessors_and_methods() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            use_define_for_class_fields: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
class FunctionBase {
    m() {}
}
class AccessorDerived extends FunctionBase {
    get m() { return () => 1; }
}

class AccessorBase {
    get x() { return 1; }
    set x(v: number) {}
}
class FunctionDerived extends AccessorBase {
    x() { return 1; }
}

class PropertyBase {
    x = 1;
}
class FunctionOverProperty extends PropertyBase {
    x() {}
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(out.contains("error TS2423"), "{out}");
        assert!(out.contains("error TS2425"), "{out}");
        assert!(out.contains("error TS2426"), "{out}");
    }

    #[test]
    fn auto_accessor_optional_reports_on_question_token() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("esnext".to_string()),
            use_define_for_class_fields: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
class C {
    accessor prop?: number;
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(out.contains("main.ts(3,18): error TS1276"), "{out}");
        assert!(
            out.contains("An 'accessor' property cannot be declared optional."),
            "{out}"
        );
    }

    #[test]
    fn string_literal_constructor_fields_are_rejected() {
        let opts = CompilerOptions {
            strict: Some(false),
            target: Some("es2015".to_string()),
            use_define_for_class_fields: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
class C {
    "constructor" = 1;
    static "constructor" = 2;
    ["constructor"] = 3;
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(out.contains("main.ts(3,5): error TS18006"), "{out}");
        assert!(out.contains("main.ts(4,12): error TS18006"), "{out}");
        assert_eq!(out.matches("error TS18006").count(), 2, "{out}");
    }

    #[test]
    fn set_accessor_parameter_grammar_reports_parameter_tokens() {
        let opts = CompilerOptions {
            strict: Some(false),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
class C {
   set optional(a?: number) { }
   set rest(...a) { }
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(out.contains("main.ts(3,18): error TS1051"), "{out}");
        assert!(out.contains("main.ts(4,13): error TS1053"), "{out}");
    }

    #[test]
    fn accessor_parameter_count_ignores_this_parameter() {
        let opts = CompilerOptions {
            strict: Some(false),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
class C {
   get withParam(x: number) { return 1; }
   set noValue(this: C) { }
   set withThis(this: C, value: number) { }
}
const o = {
   get withParam(x: number) { return 1; },
   get withThis(this: C) { return 1; },
   set noValue() { },
   set withThis(this: C, value: number) { },
   set ok(value: number) { }
};
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(out.contains("main.ts(3,8): error TS1054"), "{out}");
        assert!(out.contains("main.ts(4,8): error TS1049"), "{out}");
        assert!(out.contains("main.ts(4,16): error TS2784"), "{out}");
        assert!(out.contains("main.ts(5,17): error TS2784"), "{out}");
        assert!(out.contains("main.ts(8,8): error TS1054"), "{out}");
        assert!(out.contains("main.ts(9,17): error TS2784"), "{out}");
        assert!(out.contains("main.ts(10,8): error TS1049"), "{out}");
        assert!(out.contains("main.ts(11,17): error TS2784"), "{out}");
        assert_eq!(out.matches("error TS1049").count(), 2, "{out}");
    }

    #[test]
    fn definite_assignment_assertion_grammar_reports_bang_token() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
class C {
    a!;
    b! = 1;
    static c!: number;
}
declare class D {
    a!: number;
}
function f() {
    let x!;
    let y! = 1;
    let z!: number = 1;
}
declare let ambient!: number;
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(out.contains("main.ts(3,6): error TS1264"), "{out}");
        assert!(out.contains("main.ts(4,6): error TS1263"), "{out}");
        assert!(out.contains("main.ts(5,13): error TS1255"), "{out}");
        assert!(out.contains("main.ts(8,6): error TS1255"), "{out}");
        assert!(out.contains("main.ts(11,10): error TS1264"), "{out}");
        assert!(out.contains("main.ts(12,10): error TS1263"), "{out}");
        assert!(out.contains("main.ts(13,10): error TS1263"), "{out}");
        assert!(out.contains("main.ts(15,20): error TS1255"), "{out}");
    }

    #[test]
    fn array_to_iterable_checks_element_type() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: r#"
const bad: Iterable<number> = ["x"];
const ok: Iterable<string | number> = ["x", 1];
"#
                .to_string(),
            }],
            &opts,
        );

        assert!(out.contains("main.ts(2,7): error TS2322"), "{out}");
        assert_eq!(out.matches("error TS2322").count(), 1, "{out}");
    }

    #[test]
    fn null_member_and_call_checks_use_nullability_diagnostics() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: r#"
const x = null;
x.y;
(null).y;
null();
"#
                .to_string(),
            }],
            &opts,
        );

        assert!(out.contains("error TS18047"), "{out}");
        assert!(out.contains("error TS2531"), "{out}");
        assert!(out.contains("error TS2721"), "{out}");
        assert!(!out.contains("error TS18050"), "{out}");
        assert!(!out.contains("error TS2339"), "{out}");
        assert!(!out.contains("error TS2349"), "{out}");
    }

    #[test]
    fn non_callable_get_accessor_call_does_not_report_nullish_invoke() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: r#"
class C {
    get y() { return null; }
    set y(v: string) {}
}
const c = new C();
c.y();
"#
                .to_string(),
            }],
            &opts,
        );

        assert!(out.contains("error TS6234"), "{out}");
        assert!(!out.contains("error TS2721"), "{out}");
        assert!(!out.contains("error TS18050"), "{out}");
    }

    #[test]
    fn non_strict_nullish_get_accessor_return_widens_to_any_like() {
        let opts = CompilerOptions {
            strict: Some(false),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: r#"
class C {
    get y() { return null; }
    get z() { return undefined; }
    get n() { return 1; }
}
const c = new C();
c.y();
c.z();
c.n();
"#
                .to_string(),
            }],
            &opts,
        );

        assert_eq!(out.matches("error TS6234").count(), 1, "{out}");
        assert!(!out.contains("error TS2721"), "{out}");
        assert!(!out.contains("error TS18050"), "{out}");
    }

    #[test]
    fn syntactic_truthiness_reports_for_condition_statements() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            allow_unreachable_code: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: r#"
if (null) {}
while (undefined) {}
do {} while ('')
for (; /x/;) { break; }
if (((`x`))) {}
if (((``))) {}
if (function() {}) {}
if (class {}) {}
if ([]) {}
if ({}) {}
if (() => 0) {}
if (1) {}
while (true) { break; }
"#
                .to_string(),
            }],
            &opts,
        );

        assert_eq!(out.matches("error TS2872").count(), 7, "{out}");
        assert_eq!(out.matches("error TS2873").count(), 4, "{out}");
    }

    #[test]
    fn conditional_expression_conditions_report_truthiness_and_function_values() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            allow_unreachable_code: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: r#"
declare function df(): boolean;
declare const cf: () => boolean;
declare let maybe: (() => boolean) | undefined;
declare const obj: { m(): boolean; q?: () => boolean };
interface Opt { g?(): boolean; p?: () => boolean; }
declare const opt: Opt;
class Klass { g?() { return true; } }
declare const klass: Klass;
"" ? 1 : 2;
"x" ? 1 : 2;
df ? 1 : 2;
cf ? 1 : 2;
obj.m ? 1 : 2;
maybe ? 1 : 2;
obj.q ? 1 : 2;
opt.g ? 1 : 2;
opt.p ? 1 : 2;
klass.g ? 1 : 2;
(() => true) ? 1 : 2;
({}) ? 1 : 2;
if (opt.g) {}
if (klass.g) {}
while (df) { break; }
for (; cf;) { break; }
do { break; } while (obj.m)
"#
                .to_string(),
            }],
            &opts,
        );

        assert_eq!(out.matches("error TS2774").count(), 3, "{out}");
        assert_eq!(out.matches("error TS2872").count(), 3, "{out}");
        assert_eq!(out.matches("error TS2873").count(), 1, "{out}");
        assert!(out.contains("main.ts(21,1): error TS2872"), "{out}");
    }

    #[test]
    fn non_strict_function_conditions_do_not_report_always_defined_function() {
        let opts = CompilerOptions {
            strict: Some(false),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: r#"
declare function f(): boolean;
if (f) {}
const x = f ? 1 : 2;
"#
                .to_string(),
            }],
            &opts,
        );

        assert!(!out.contains("error TS2774"), "{out}");
    }

    #[test]
    fn logical_and_or_and_not_report_truthiness_with_context_specific_defined_checks() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: r#"
interface Promise<T> {}
declare const a: any;
declare const v: void;
declare const p: Promise<number>;
declare function f(): boolean;
null && a;
undefined || a;
"" && a;
"x" || a;
({}) && a;
[] || a;
f && a;
f || a;
!f;
!null;
!"x";
!({});
v && a;
v || a;
!v;
p && a;
p || a;
!p;
a && null;
a || undefined;
"#
                .to_string(),
            }],
            &opts,
        );

        assert_eq!(out.matches("error TS2872").count(), 5, "{out}");
        assert_eq!(out.matches("error TS2873").count(), 4, "{out}");
        assert_eq!(out.matches("error TS2774").count(), 1, "{out}");
        assert_eq!(out.matches("error TS1345").count(), 3, "{out}");
        assert_eq!(out.matches("error TS2801").count(), 1, "{out}");
    }

    #[test]
    fn optional_super_methods_do_not_report_always_defined_function_conditions() {
        let opts = CompilerOptions {
            strict_null_checks: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: r#"
class B {
    protected m?(): void;
    protected static s?(): void;
}
class C extends B {
    body() {
        super.m && super.m();
    }
    static body() {
        super.s && super.s();
    }
}
"#
                .to_string(),
            }],
            &opts,
        );

        assert!(!out.contains("error TS2774"), "{out}");
    }

    #[test]
    fn named_function_expression_truthiness_reports_at_name() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: r#"
declare const a: any;
const x = function named() {} || a;
const y = function() {} || a;
"#
                .to_string(),
            }],
            &opts,
        );

        assert_eq!(out.matches("error TS2872").count(), 2, "{out}");
        assert!(out.contains("main.ts(3,20): error TS2872"), "{out}");
        assert!(out.contains("main.ts(4,11): error TS2872"), "{out}");
    }

    #[test]
    fn syntactic_truthiness_reports_numeric_and_wrapped_literals() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2020".to_string()),
            allow_unreachable_code: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: r#"
declare const a: any;
if (0) {}
if (1) {}
if (2) {}
0 ? a : a;
1 ? a : a;
2 ? a : a;
!0;
!1;
!2;
0n ? a : a;
void 0 || a;
(<number>undefined) || a;
undefined! || a;
(<any>{}) || a;
"#
                .to_string(),
            }],
            &opts,
        );

        assert_eq!(out.matches("error TS2872").count(), 5, "{out}");
        assert_eq!(out.matches("error TS2873").count(), 3, "{out}");
        assert!(!out.contains("main.ts(3,5): error TS2872"), "{out}");
        assert!(!out.contains("main.ts(4,5): error TS2872"), "{out}");
        assert!(out.contains("main.ts(5,5): error TS2872"), "{out}");
        assert!(out.contains("main.ts(12,1): error TS2872"), "{out}");
        assert!(out.contains("main.ts(13,1): error TS2873"), "{out}");
        assert!(out.contains("main.ts(14,1): error TS2873"), "{out}");
        assert!(out.contains("main.ts(15,1): error TS2873"), "{out}");
        assert!(out.contains("main.ts(16,1): error TS2872"), "{out}");
    }

    #[test]
    fn exponentiation_left_operand_reports_unparenthesized_unary_and_type_assertions() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2017".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![InputFile {
                name: "main.ts".to_string(),
                text: r#"
let x = 1, y = 2;
-x ** y;
+x ** y;
!x ** y;
~x ** y;
typeof x ** y;
void x ** y;
delete x ** y;
<any>x ** y;
<const>x ** y;
++x ** y;
x++ ** y;
(-x) ** y;
(<any>x) ** y;
async function f() {
    await x ** y;
    (await x) ** y;
}
"#
                .to_string(),
            }],
            &opts,
        );

        assert_eq!(out.matches("error TS17006").count(), 8, "{out}");
        assert_eq!(out.matches("error TS17007").count(), 2, "{out}");
    }

    #[test]
    fn accessor_signatures_report_implicit_any_and_abstract_implementations() {
        let opts = CompilerOptions {
            strict: Some(false),
            diag_json: true,
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
abstract class A {
   abstract get a();
   abstract get aa() { return 1; }
   abstract set b(v);
   abstract set bb(v: string) {}
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(out.contains("\"code\":7033,\"category\":2"), "{out}");
        assert!(out.contains("\"code\":7032,\"category\":2"), "{out}");
        assert!(out.contains("\"code\":1318,\"category\":1"), "{out}");
    }

    #[test]
    fn global_object_is_not_assignable_to_callable_or_constructable_types() {
        let opts = CompilerOptions {
            strict: Some(false),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: r#"
interface Object {
    toString(): string;
}
"#
                    .to_string(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
interface Callable {
    (): void;
}
interface Constructable {
    new(): any;
}
declare var obj: Object;
declare var callable: Callable;
declare var constructable: Constructable;
obj = callable;
callable = obj;
obj = constructable;
constructable = obj;
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(out.contains("main.ts(12,1): error TS2322"), "{out}");
        assert!(out.contains("main.ts(14,1): error TS2322"), "{out}");
    }

    #[test]
    fn this_type_predicate_narrows_call_receiver() {
        let opts = CompilerOptions {
            strict: Some(false),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
class Guard {
    isLeader(): this is Leader { return this instanceof Leader; }
    isFollower(): this is Follower { return this instanceof Follower; }
}
class Leader extends Guard { lead(): void {} }
class Follower extends Guard { follow(): void {} }

let a: Guard = new Follower();
if (a.isLeader()) {
    a.lead();
}
else if (a.isFollower()) {
    a.follow();
}

interface Supplies {
    spoiled: boolean;
}
interface Box<T> {
    contents: T;
    isSupplies(): this is Box<Supplies>;
}
let box: Box<{}>;
if (box.isSupplies()) {
    box.contents.spoiled = true;
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(!out.contains("main.ts"), "{out}");
    }

    #[test]
    fn or_condition_true_branch_merges_narrowing_alternatives() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
declare let x: string | boolean;
if ((typeof x === "boolean" && !x) || typeof x === "string") {
    let s: string = typeof x === "string" ? x : "fallback";
    let b: boolean = typeof x === "boolean" ? x : false;
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(!out.contains("main.ts"), "{out}");
    }

    #[test]
    fn cfg_unreachable_code_never_calls_ranges_and_declaration_kinds() {
        let opts = CompilerOptions {
            strict: Some(true),
            allow_unreachable_code: Some(false),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
declare function fail(msg?: string): never;
declare function g(): void;
function a(): void {
    while (true) { break; }
    g();
}
function b() {
    fail();
    g();
    g();
}
function c() {
    fail();
    class C {}
}
function d() {
    return;
    class D {}
}
function e() {
    try { throw 1; } catch {}
    g();
}
function f2() {
    const x = fail();
    g();
}
function h() {
    return;
    g();
    ;
    g();
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        // never-call: one diagnostic for the contiguous run, at its start
        assert!(out.contains("main.ts(10,5): error TS7027"), "{out}");
        assert!(!out.contains("main.ts(11,5)"), "{out}");
        // class after never-call: structural walk only — not reported
        assert!(!out.contains("main.ts(15,5)"), "{out}");
        // class after return: the structural bit applies — reported
        assert!(out.contains("main.ts(19,5): error TS7027"), "{out}");
        // loop with break exits; try/throw recovers through catch;
        // a declaration-position call does not terminate flow
        assert!(!out.contains("main.ts(6,5)"), "{out}");
        assert!(!out.contains("main.ts(23,5)"), "{out}");
        assert!(!out.contains("main.ts(27,5)"), "{out}");
        // an empty statement is exempt and splits the run in two
        assert!(out.contains("main.ts(31,5): error TS7027"), "{out}");
        assert!(out.contains("main.ts(33,5): error TS7027"), "{out}");
    }

    #[test]
    fn cfg_return_path_ordered_tree_2355_2366_2534() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
declare function fail(): never;
function a(): number { }
function b(): number { if (!!true) return 1; }
function c(): number { fail(); }
function d(): never { fail(); }
function e(): never { }
function f(): number | undefined { }
function g(): number | void { }
function h(): number { if (!!true) { while (true) {} return 1; } }
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        // no return at all vs reachable end with a return
        assert!(out.contains("main.ts(3,15): error TS2355"), "{out}");
        assert!(out.contains("main.ts(4,15): error TS2366"), "{out}");
        // a tail never-call demotes the implicit return: both exempt
        assert!(!out.contains("main.ts(5,15)"), "{out}");
        assert!(!out.contains("main.ts(6,15)"), "{out}");
        assert!(out.contains("main.ts(7,15): error TS2534"), "{out}");
        // top-level undefined exempts, but a `| undefined` union does not;
        // a void member anywhere exempts
        assert!(out.contains("main.ts(8,15): error TS2355"), "{out}");
        assert!(!out.contains("main.ts(9,15)"), "{out}");
        // HasExplicitReturn is syntactic: a structurally unreachable
        // `return` still selects 2366 over 2355
        assert!(out.contains("main.ts(10,15): error TS2366"), "{out}");
    }

    #[test]
    fn cfg_async_return_paths_use_awaited_annotation() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2017".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: r#"
interface Promise<T> {
    then<R1 = T, R2 = never>(
        onfulfilled?: ((value: T) => R1) | undefined | null,
        onrejected?: ((reason: any) => R2) | undefined | null): Promise<R1 | R2>;
}
"#
                    .to_string(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
async function i(): Promise<number> { }
declare class Thenable { then(): void; }
async function j(): Thenable { }
async function k(): Promise<void> { }
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        assert!(out.contains("main.ts(2,21): error TS2355"), "{out}");
        // a thenable whose promised type cannot be extracted is errorType:
        // exempt from the return-path tree (tsc still reports 1055/1064)
        assert!(!out.contains("main.ts(4,21): error TS2355"), "{out}");
        assert!(!out.contains("main.ts(5,21)"), "{out}");
    }

    #[test]
    fn cfg_no_implicit_returns_and_switch_fallthrough() {
        let opts = CompilerOptions {
            strict: Some(false),
            no_implicit_returns: true,
            no_fallthrough_cases_in_switch: true,
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
declare function fail(): never;
function a(): number { if (!!true) return 1; }
function b() { if (!!true) return 1; }
function c() { if (!!true) return; }
function s1(x: number) {
    switch (x) { case 1: x++; case 2: break; }
}
function s2(x: number) {
    switch (x) { case 1: case 2: break; }
}
function s3(x: number) {
    switch (x) { case 1: fail(); case 2: break; }
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );

        // noImplicitReturns fires for annotated functions too (span: the
        // annotation), and for unannotated ones at the name — but only
        // when some return carries a value
        assert!(out.contains("main.ts(3,15): error TS7030"), "{out}");
        assert!(out.contains("main.ts(4,10): error TS7030"), "{out}");
        assert!(!out.contains("main.ts(5,"), "{out}");
        // fallthrough: non-empty non-last clause with a reachable end;
        // empty clauses merge silently; a never-call end suppresses
        assert!(out.contains("main.ts(7,18): error TS7029"), "{out}");
        assert!(!out.contains("main.ts(10,18)"), "{out}");
        assert!(!out.contains("main.ts(13,18)"), "{out}");
    }

    /// Stage 4: auto-variable CFA (tsc autoType). The oracle fixture
    /// controlFlowNoImplicitAny is OUTSIDE the gate corpus, so these pins
    /// carry it: same-container reads take the flow union with the nullish
    /// initial (strict), and no 7005/7034 fires for them.
    #[test]
    fn stage4_auto_variable_cfa_strict() {
        let opts = CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
declare let cond: boolean;
function f1() {
    let x;
    if (cond) { x = 1; }
    if (cond) { x = "hello"; }
    const y = x;
    const z: never = y;
}
function f3() {
    let x = null;
    if (cond) { x = 1; }
    const y = x;
    const z: never = y;
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );
        assert!(
            out.contains("Type 'string | number | undefined' is not assignable to type 'never'"),
            "{out}"
        );
        assert!(
            out.contains("Type 'number | null' is not assignable to type 'never'"),
            "{out}"
        );
        assert!(!out.contains("TS7005"), "{out}");
        assert!(!out.contains("TS7034"), "{out}");
    }

    /// Stage 4: capture reads of auto variables are tsc's autoType — any +
    /// TS7005 at the read + TS7034 at the declaration; a capture that
    /// assigns locally before reading resolves and stays silent.
    #[test]
    fn stage4_auto_capture_reads_7005_7034() {
        let opts = CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
function f9() {
    let x;
    x = 1;
    const g = () => x;
    g();
}
function f6() {
    let ok;
    const h = () => { ok = 2; return ok; };
    h();
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );
        assert!(
            out.contains("main.ts(3,9): error TS7034"),
            "7034 at the decl name: {out}"
        );
        assert!(
            out.contains("main.ts(5,21): error TS7005"),
            "7005 at the capture read: {out}"
        );
        assert!(
            !out.contains("Variable 'ok' implicitly"),
            "locally-assigned capture stays silent: {out}"
        );
    }

    /// Stage 4: non-strict CFA drops nullish members once mixed with an
    /// assigned type, but an exactly-initial read stays `undefined` and a
    /// property access on it errors 18048 even without strictNullChecks
    /// (tsc checkNonNullType's top-level-flags path).
    #[test]
    fn stage4_auto_nonstrict_mixed_drop_and_pure_nullish() {
        let opts = CompilerOptions {
            strict: Some(false),
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
declare let cond: boolean;
function p1() {
    let x;
    if (cond) { x = 1; }
    const y = x;
    const z: never = y;
}
function p3b() {
    let x;
    x.foo;
    x = 1;
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );
        assert!(
            out.contains("Type 'number' is not assignable to type 'never'"),
            "nullish dropped when mixed: {out}"
        );
        assert!(
            out.contains("main.ts(11,5): error TS18048"),
            "pure-undefined receiver errors even non-strict: {out}"
        );
    }

    /// Stage 4: unreachable code narrows nothing (tsc collapses conditions
    /// over unreachable flow in the binder), and a dead matching assignment
    /// contributes nothing to live joins (reachability guard).
    #[test]
    fn stage4_dead_code_reads_declared() {
        let opts = CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
function f(x: string | number) {
    return;
    if (typeof x === "string") {
        const s: string = x;
    }
}
function g() {
    let x: number | string = "a";
    if (false) { x = 1; }
    const y: string = x;
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );
        assert!(
            out.contains("main.ts(5,15): error TS2322"),
            "dead-code reads are DECLARED, unnarrowed: {out}"
        );
        assert!(
            !out.contains("main.ts(11,"),
            "dead assignment drops from the join: {out}"
        );
    }

    /// Stage 4: `this` narrows like any reference — both the VALUE `this`
    /// and a root-level `typeof this` annotation read the narrowed type
    /// under `this instanceof D` (the fact stack never narrowed values).
    #[test]
    fn stage4_this_and_typeof_this_narrowing() {
        let opts = CompilerOptions {
            strict: Some(true),
            target: Some("es2015".to_string()),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
class D1 { f1() { return 1; } }
class C {
    m() {
        if (this instanceof D1) {
            const d: typeof this = this;
            d.f1();
        }
    }
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );
        assert!(
            !out.contains("TS2339"),
            "member resolves on narrowed this: {out}"
        );
        assert!(!out.contains("TS2741"), "value this narrows too: {out}");
        assert!(!out.contains("TS2322"), "{out}");
    }

    /// Stage 4: comparison/arithmetic/unary operands containing nullish
    /// report 18048 (tsc checkNonNullType at operator positions), not the
    /// operator-applicability errors.
    #[test]
    fn stage4_operand_nullish_18048() {
        let opts = CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: "function f() {\n    let x;\n    x < 1;\n    -x;\n    x * 2;\n}\n"
                        .to_string(),
                },
            ],
            &opts,
        );
        assert!(out.contains("main.ts(3,5): error TS18048"), "{out}");
        assert!(out.contains("main.ts(4,6): error TS18048"), "{out}");
        assert!(out.contains("main.ts(5,5): error TS18048"), "{out}");
        assert!(!out.contains("TS2365"), "{out}");
        assert!(!out.contains("TS2362"), "{out}");
    }

    /// Stage 4 write positions: a prop/elem assignment TARGET reads its
    /// declared reference type (tsc AssignmentKind.Definite) while the
    /// RECEIVER narrows normally — `control[key] = value` under a guard.
    #[test]
    fn stage4_write_target_receiver_narrows() {
        let opts = CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        };
        let (out, _code) = check_program(
            vec![
                InputFile {
                    name: LIB_NAME.to_string(),
                    text: String::new(),
                },
                InputFile {
                    name: "main.ts".to_string(),
                    text: r#"
interface A { p: string; }
function u<T extends A, K extends keyof T>(c: T | undefined, k: K, v: T[K]) {
    if (c !== undefined) {
        c[k] = v;
    }
}
"#
                    .to_string(),
                },
            ],
            &opts,
        );
        assert!(!out.contains("TS18048"), "receiver is narrowed: {out}");
        assert!(
            !out.contains("TS2322"),
            "target member type accepts T[K]: {out}"
        );
    }
}
