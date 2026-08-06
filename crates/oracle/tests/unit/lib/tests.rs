use super::*;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir() -> PathBuf {
    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);
    std::env::temp_dir().join(format!(
        "tsc-rs-oracle-test-{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos(),
        NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ))
}

fn write_program_json(source_b64: &str) -> PathBuf {
    let dir = temp_dir();
    fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("program.json");
    fs::write(
            &path,
            format!(
                "{{\n  \"schema\": 1,\n  \"cwd\": \"/\",\n  \"options\": {{\"noLib\": true}},\n  \"libs\": [],\n  \"files\": [{{\"name\": \"main.ts\", \"textB64\": \"{source_b64}\"}}],\n  \"matrixKey\": \"\"\n}}\n"
            ),
        )
        .expect("write program");
    path
}

#[test]
fn oracle_driver_returns_syntactic_diagnostics() {
    let program_json = write_program_json("bGV0ID0gOwo=");
    let pool = OraclePool::new(1).expect("pool");
    let diagnostics = pool.diagnostics(&program_json).expect("diagnostics");

    assert!(diagnostics
        .iter()
        .any(|diag| diag.code == 1389 || diag.code == 1109));
    fs::remove_dir_all(program_json.parent().unwrap()).expect("remove temp dir");
}

#[test]
fn oracle_driver_preserves_implied_node_format() {
    let dir = temp_dir();
    fs::create_dir_all(&dir).expect("temp dir");
    let program_json = dir.join("program.json");
    fs::write(
        &program_json,
        r#"{
  "schema": 1,
  "cwd": "/",
  "options": {"module": "node16", "moduleResolution": "node16", "target": "es2022"},
  "libs": [],
  "files": [
    {"name": "/foo.ts", "textB64": "ZXhwb3J0IGNvbnN0IHggPSAxOwo="},
    {"name": "/main.mts", "textB64": "aW1wb3J0IHsgeCB9IGZyb20gIi4vZm9vIjsKeDsK"}
  ],
  "matrixKey": ""
}
"#,
    )
    .expect("write program");

    let pool = OraclePool::new(1).expect("pool");
    let diagnostics = pool.diagnostics(&program_json).expect("diagnostics");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == 2835),
        "{diagnostics:?}"
    );
    fs::remove_dir_all(dir).expect("remove temp dir");
}

#[test]
fn oracle_driver_reports_launched_node_version() {
    let pool = OraclePool::new(1).expect("pool");
    let version = pool.node_version().expect("version");
    assert!(
        version.starts_with('v') && version.len() > 1,
        "process.version shape: {version}"
    );
}

#[test]
fn oracle_diag_preserves_related_information_presence() {
    let absent: OracleDiag = serde_json::from_str(
        r#"{
  "file": null,
  "start": null,
  "length": null,
  "code": 2769,
  "category": "error",
  "chain": {"text": "No overload matches this call.", "code": 2769, "category": "error"},
  "related": []
}"#,
    )
    .expect("diagnostic without presence marker");
    assert!(!absent.related_information_present);

    let mut present = absent;
    present.related_information_present = true;
    let encoded = serde_json::to_value(&present).expect("serialize diagnostic");
    assert_eq!(encoded["relatedInformationPresent"], true);
    let decoded: OracleDiag =
        serde_json::from_value(encoded).expect("deserialize diagnostic with presence marker");
    assert!(decoded.related_information_present);
    assert!(decoded.related.is_empty());
}

#[test]
fn oracle_driver_pins_vendored_context_rendering_including_suggestions() {
    let program_json = write_program_json("bGV0ID0gOwo=");
    let pool = OraclePool::new_render_only();
    assert!(pool.diagnostics(&program_json).is_err());
    let rendered = pool
        .diagnostics_with_rendering(&program_json)
        .expect("rendered diagnostics");
    assert_eq!(
        rendered.rendered,
        concat!(
            "main.ts:1:1 - error TS2304: Cannot find name 'let'.\n",
            "\n",
            "1 let = ;\n",
            "  ~~~\n",
            "main.ts:1:7 - error TS1109: Expression expected.\n",
            "\n",
            "1 let = ;\n",
            "        ~\n",
        )
    );
    assert_eq!(rendered.diagnostics.len(), 2);
    fs::remove_dir_all(program_json.parent().unwrap()).expect("remove temp dir");

    let suggestion_json = write_program_json("ZnVuY3Rpb24gZigpIHsgbGV0IHggPSAxOyB9Cg==");
    let suggestion = pool
        .diagnostics_with_rendering(&suggestion_json)
        .expect("rendered suggestion");
    assert_eq!(
        suggestion.rendered,
        concat!(
            "main.ts:1:20 - suggestion TS6133: 'x' is declared but its value is never read.\n",
            "\n",
            "1 function f() { let x = 1; }\n",
            "                     ~\n",
        )
    );
    assert_eq!(suggestion.diagnostics[0].category, "suggestion");
    fs::remove_dir_all(suggestion_json.parent().unwrap()).expect("remove temp dir");
}

#[test]
fn oracle_pool_is_deterministic_and_respawns_worker() {
    let program_json = write_program_json("bGV0ID0gOwo=");
    let pool = OraclePool::new(1).expect("pool");
    let first = pool.diagnostics(&program_json).expect("first diagnostics");
    {
        let mut worker = pool.workers[0].lock().expect("worker lock");
        worker.child.kill().expect("kill worker");
        worker.child.wait().expect("wait worker");
    }
    let second = pool.diagnostics(&program_json).expect("second diagnostics");

    assert_eq!(first, second);
    fs::remove_dir_all(program_json.parent().unwrap()).expect("remove temp dir");
}
