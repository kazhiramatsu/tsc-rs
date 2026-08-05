use super::*;

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        output.push(TABLE[usize::from(a >> 2)] as char);
        output.push(TABLE[usize::from(((a & 0x03) << 4) | (b >> 4))] as char);
        if chunk.len() > 1 {
            output.push(TABLE[usize::from(((b & 0x0f) << 2) | (c >> 6))] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[usize::from(c & 0x3f)] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn encoded(ordinal: u32, name: &str, text: &str) -> EncodedFile {
    EncodedFile {
        ordinal,
        name: name.to_owned(),
        text_base64: base64(text.as_bytes()),
    }
}

#[test]
fn wire_program_produces_true_passes_and_renderer_joins() {
    let program = WireProgram {
        cwd: "/work".to_owned(),
        options: Vec::new(),
        libs: Vec::new(),
        files: vec![
            encoded(0, "z.ts", "const broken = ;\n"),
            encoded(
                1,
                "a.ts",
                "export {};\nconst dead = 1;\nlet n: number = \"x\";\n",
            ),
        ],
    };
    let mut phases = Vec::new();
    let output = execute_wire_program(&program, |phase| phases.push(phase)).unwrap();
    assert_eq!(phases, WorkerPhase::ORDERED);

    let EngineResult::Completed { outcome } = &output.result else {
        panic!("completed result");
    };
    assert!(outcome
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.pass == DiagnosticPass::Syntactic));
    assert!(outcome
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.pass == DiagnosticPass::Semantic));
    assert!(outcome
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.pass == DiagnosticPass::Suggestion));
    assert!(outcome
        .diagnostics
        .iter()
        .any(|diagnostic| { diagnostic.reports_unnecessary == OptionalBool::present(true) }));
    assert_eq!(
        outcome.renderer.aggregate_text,
        outcome
            .renderer
            .segments
            .iter()
            .map(|segment| segment.raw_text.as_str())
            .collect::<String>()
    );
    assert_eq!(
        output.deduped_indices.len(),
        outcome.renderer.segments.len()
    );
}

#[test]
fn wire_validation_rejects_unknown_options_before_parse() {
    let program = WireProgram {
        cwd: "/".to_owned(),
        options: vec![OrderedSetting {
            ordinal: 0,
            name: "futureOption".to_owned(),
            value: CompilerOptionValue::Boolean { value: true },
        }],
        libs: Vec::new(),
        files: vec![encoded(0, "main.ts", "const x = 1;\n")],
    };
    let mut phases = Vec::new();
    assert!(execute_wire_program(&program, |phase| phases.push(phase)).is_err());
    assert!(phases.is_empty());
}

#[test]
fn inline_no_lib_is_consumed_before_the_closed_checker_projection() {
    let mut program = WireProgram {
        cwd: "/".to_owned(),
        options: vec![OrderedSetting {
            ordinal: 0,
            name: "noLib".to_owned(),
            value: CompilerOptionValue::Boolean { value: true },
        }],
        libs: Vec::new(),
        files: vec![encoded(0, "main.ts", "const x = 1;\n")],
    };
    let mut phases = Vec::new();
    execute_wire_program(&program, |phase| phases.push(phase)).unwrap();
    assert_eq!(phases, WorkerPhase::ORDERED);

    program.options[0].value = CompilerOptionValue::Boolean { value: false };
    phases.clear();
    assert!(execute_wire_program(&program, |phase| phases.push(phase)).is_err());
    assert!(phases.is_empty());
}
