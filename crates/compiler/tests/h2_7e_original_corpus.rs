//! Original E8 command and CLI checks; hosted acceptance shares only the command comparator.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{json, Value};

#[path = "integration/h2_7e_original_corpus_shared.rs"]
mod h2_7e_original_corpus_shared;
use h2_7e_original_corpus_shared::{frozen, indexed, workspace};

#[test]
fn h2_7e_original_corpus_preserves_complete_tuples_and_boundaries() {
    assert_eq!(
        std::env::current_dir().unwrap().canonicalize().unwrap(),
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .unwrap(),
        "run through cargo in the compiled worktree; shared target reuse must not select another workspace"
    );
    eprintln!(
        "H2.7e original manifest={} binary={}",
        env!("CARGO_MANIFEST_DIR"),
        env!("CARGO_BIN_EXE_tsc-rs")
    );
    h2_7e_original_corpus_shared::assert_original_corpus(&workspace());
    assert_original_cli_corpus();
}

struct CliTree(PathBuf);
impl Drop for CliTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn assert_cli(case: &Value, expected: &Value) {
    let directory = std::env::temp_dir().join(format!(
        "tsc-rs-h2-7e-corpus-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let tree = CliTree(directory.canonicalize().unwrap());
    // The Program comparison retains /.src verbatim. The CLI additionally
    // checks its real wrapper in a relocated input tree; maps stay relative.
    // The declaration-absent row additionally compares TS's config diagnostic.
    let mut input_paths = BTreeSet::from([PathBuf::from("tsconfig.json")]);
    for file in case["input"]["files"].as_array().unwrap() {
        let relative = file["path"]
            .as_str()
            .unwrap()
            .strip_prefix("/.src/")
            .unwrap();
        assert_eq!(Path::new(relative).components().count(), 1);
        std::fs::write(tree.0.join(relative), file["text"].as_str().unwrap()).unwrap();
        input_paths.insert(relative.into());
    }
    let mut options = case["effective_options"].clone();
    options["target"] = json!(match options["target"].as_u64().unwrap() {
        2 => "es2015",
        9 => "es2022",
        99 => "esnext",
        _ => panic!("original target"),
    });
    if let Some(module) = options["module"].as_u64() {
        options["module"] = json!(match module {
            1 => "commonjs",
            99 => "esnext",
            _ => panic!("original module"),
        });
    }
    assert_eq!(options["newLine"], 0);
    options["newLine"] = json!("crlf");
    let roots = case["input"]["roots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().strip_prefix("/.src/").unwrap())
        .collect::<Vec<_>>();
    std::fs::write(
        tree.0.join("tsconfig.json"),
        serde_json::to_vec(&json!({"compilerOptions":options,"files":roots})).unwrap(),
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tsc-rs"))
        .current_dir(&tree.0)
        .args(["-p", "tsconfig.json", "--pretty", "false"])
        .output()
        .unwrap();
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(expected["status_writes"], json!([]));
    let has_diagnostics = !expected["reported_diagnostics"]
        .as_array()
        .unwrap()
        .is_empty();
    if !has_diagnostics {
        assert!(
            output.stdout.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
    assert_eq!(json!(output.status.code()), expected["exit_code"]);
    let mut output_paths = BTreeSet::new();
    for write in expected["writes"].as_array().unwrap() {
        let path = write["path"]
            .as_str()
            .unwrap()
            .strip_prefix("/.src/")
            .unwrap();
        output_paths.insert(PathBuf::from(path));
        let expected = base64::engine::general_purpose::STANDARD
            .decode(write["materialized_utf8_base64"].as_str().unwrap())
            .unwrap();
        assert_eq!(
            std::fs::read(tree.0.join(path)).unwrap(),
            expected,
            "{path}"
        );
    }
    let actual_paths = std::fs::read_dir(&tree.0)
        .unwrap()
        .map(|e| PathBuf::from(e.unwrap().file_name()))
        .filter(|p| !input_paths.contains(p))
        .collect::<BTreeSet<_>>();
    assert_eq!(actual_paths, output_paths);
    if has_diagnostics {
        assert_eq!(
            case["case_id"],
            "typescript-6.0.3/compiler/declarationMapsWithoutDeclaration.ts#default"
        );
        assert!(case["effective_options"].get("declaration").is_none());
        assert_eq!(
            expected["reported_diagnostics"].as_array().unwrap().len(),
            1
        );
        assert_eq!(expected["reported_diagnostics"][0]["code"], 5069);
        assert_eq!(expected["reported_diagnostics"][0]["file"], Value::Null);
        // The immutable Program observation has no config file. Compare the
        // relocated CLI's config position with the pinned TS6 CLI separately.
        // Rust bytes and absence of extra outputs were checked before TS can
        // write into this tree, so an oracle write cannot hide a Rust mismatch.
        let reference = std::process::Command::new("node")
            .arg(workspace().join("vendor/typescript-6.0.3/lib/_tsc.js"))
            .current_dir(&tree.0)
            .args(["-p", "tsconfig.json", "--pretty", "false"])
            .output()
            .unwrap();
        assert_eq!(output.stdout, reference.stdout);
        assert_eq!(output.stderr, reference.stderr);
        assert_eq!(output.status.code(), reference.status.code());
    }
}

fn assert_original_cli_corpus() {
    let inputs = frozen(
        "ratchets/h2-7de-candidate-inputs.v1.json",
        "f2e078a6b6d10cd3c6df833584924c18e8f78fe98c1e41621c70e10e215a073a",
    );
    let observations = frozen(
        "ratchets/h2-7de-observations.v1.json",
        "1a1681b2375d27d9012b06e29808aca72aa3e39d1dbc1536b80ba2aadf9e8ce2",
    );
    let inputs = indexed(&inputs);
    let mut compared = 0;
    for (id, reference) in indexed(&observations) {
        if reference["required_slices"] != json!(["H2.7e"]) {
            continue;
        }
        for _ in 0..2 {
            assert_cli(inputs[id], &reference["typescript_observation"]);
        }
        compared += 1;
    }
    assert_eq!(compared, 8);
}
