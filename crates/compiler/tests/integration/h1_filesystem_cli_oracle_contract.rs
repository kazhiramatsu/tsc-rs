use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde_json::{json, Map, Value};

const ORACLE_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h1-emit-oracle.v1.json"
));

static NEXT_TEMP_TREE: AtomicU64 = AtomicU64::new(0);

struct TempTree {
    root: PathBuf,
}

impl TempTree {
    fn new() -> Self {
        loop {
            let sequence = NEXT_TEMP_TREE.fetch_add(1, Ordering::Relaxed);
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock follows Unix epoch")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "tsc-rs-h1-fs-oracle-{timestamp}-{sequence}-{}",
                std::process::id()
            ));
            match fs::create_dir(&root) {
                Ok(()) => return Self { root },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create filesystem-oracle tree: {error}"),
            }
        }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn compiler_current_directory(&self) -> PathBuf {
        fs::canonicalize(&self.root).expect("canonicalize compiler current directory")
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            if !std::thread::panicking() {
                panic!("remove filesystem-oracle tree: {error}");
            }
        }
    }
}

fn project_relative(path: &str) -> &str {
    path.strip_prefix("/project/")
        .expect("oracle path belongs to /project")
}

fn materialize_case(case: &Value, tree: &TempTree) {
    let mut roots = Vec::new();
    for source in case["input"]["root_files"]
        .as_array()
        .expect("oracle root files")
    {
        let relative = project_relative(source["path"].as_str().expect("oracle source path"));
        let path = tree.path(relative);
        fs::create_dir_all(path.parent().expect("source has a parent"))
            .expect("create oracle source parent");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(source["utf8_base64"].as_str().expect("oracle source bytes"))
            .expect("decode oracle source bytes");
        fs::write(path, bytes).expect("write oracle source");
        roots.push(Value::String(relative.to_owned()));
    }

    let input_options = case["input"]["compiler_options"]
        .as_object()
        .expect("oracle compiler options");
    let mut options = Map::from_iter([
        ("target".to_owned(), Value::String("esnext".to_owned())),
        ("module".to_owned(), Value::String("preserve".to_owned())),
        ("lib".to_owned(), json!(["es5"])),
    ]);
    for name in [
        "useDefineForClassFields",
        "noEmitOnError",
        "emitBOM",
        "listEmittedFiles",
    ] {
        if let Some(value) = input_options.get(name) {
            options.insert(name.to_owned(), value.clone());
        }
    }
    if let Some(value) = input_options.get("newLine").and_then(Value::as_i64) {
        options.insert(
            "newLine".to_owned(),
            Value::String(
                match value {
                    0 => "crlf",
                    1 => "lf",
                    _ => panic!("oracle newLine is outside the admitted profile"),
                }
                .to_owned(),
            ),
        );
    }
    let config = json!({
        "compilerOptions": Value::Object(options),
        "files": roots,
    });
    fs::write(
        tree.path("tsconfig.json"),
        serde_json::to_vec(&config).expect("serialize filesystem-oracle config"),
    )
    .expect("write filesystem-oracle config");
}

fn expected_stdout(case: &Value, tree: &TempTree) -> String {
    let mut output = String::new();
    for diagnostic in case["observation"]["reported_diagnostics"]
        .as_array()
        .expect("oracle reported diagnostics")
    {
        let code = diagnostic["code"].as_u64().expect("diagnostic code");
        let category = diagnostic["category"]
            .as_str()
            .expect("diagnostic category");
        let message = diagnostic["chain"]["text"]
            .as_str()
            .expect("diagnostic text");
        if diagnostic["file"]["present"] == Value::Bool(true) {
            let file = project_relative(
                diagnostic["file"]["value"]
                    .as_str()
                    .expect("diagnostic file"),
            );
            let line = diagnostic["line"]["value"]
                .as_u64()
                .expect("diagnostic line")
                + 1;
            let column = diagnostic["column"]["value"]
                .as_u64()
                .expect("diagnostic column")
                + 1;
            output.push_str(&format!(
                "{file}({line},{column}): {category} TS{code}: {message}\n"
            ));
        } else {
            output.push_str(&format!("{category} TS{code}: {message}\n"));
        }
    }
    let current_directory = tree.compiler_current_directory();
    for status in case["observation"]["status_writes"]
        .as_array()
        .expect("oracle status writes")
    {
        let status = status.as_str().expect("oracle status text");
        output.push_str(&status.replace("/project", &current_directory.to_string_lossy()));
        output.push('\n');
    }
    output
}

fn javascript_files(root: &Path) -> Vec<PathBuf> {
    fn visit(root: &Path, current: &Path, files: &mut Vec<PathBuf>) {
        let mut entries = fs::read_dir(current)
            .expect("read emitted tree")
            .map(|entry| entry.expect("read emitted entry").path())
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                visit(root, &path, files);
            } else if path.extension().is_some_and(|extension| extension == "js") {
                files.push(
                    path.strip_prefix(root)
                        .expect("emitted file belongs to temp tree")
                        .to_path_buf(),
                );
            }
        }
    }

    let mut files = Vec::new();
    visit(root, root, &mut files);
    files
}

#[test]
fn admitted_h1_oracle_cases_match_the_filesystem_cli_route() {
    let oracle: Value = serde_json::from_slice(ORACLE_BYTES).expect("parse H1 emit oracle");
    for case in oracle["cases"].as_array().expect("oracle cases") {
        if case["input"]["classification"] != Value::String("admitted".to_owned()) {
            continue;
        }
        let id = case["input"]["id"].as_str().expect("oracle case id");
        let tree = TempTree::new();
        materialize_case(case, &tree);
        let output = Command::new(env!("CARGO_BIN_EXE_tsc-rs"))
            .current_dir(&tree.root)
            .args(["--pretty", "false", "-p", "tsconfig.json"])
            .output()
            .expect("run filesystem CLI oracle case");

        assert_eq!(
            output.status.code(),
            case["observation"]["process_exit"]["code"]
                .as_i64()
                .and_then(|value| i32::try_from(value).ok()),
            "{id}: process exit"
        );
        assert_eq!(
            String::from_utf8(output.stdout).expect("CLI stdout is UTF-8"),
            expected_stdout(case, &tree),
            "{id}: stdout"
        );
        assert!(output.stderr.is_empty(), "{id}: stderr was not empty");

        let writes = case["observation"]["writes"]
            .as_array()
            .expect("oracle writes");
        let mut expected_paths = Vec::new();
        for write in writes {
            let relative = project_relative(write["path"].as_str().expect("oracle output path"));
            expected_paths.push(PathBuf::from(relative));
            let expected = base64::engine::general_purpose::STANDARD
                .decode(
                    write["materialized_utf8_base64"]
                        .as_str()
                        .expect("oracle materialized bytes"),
                )
                .expect("decode oracle materialized bytes");
            assert_eq!(
                fs::read(tree.path(relative)).expect("read filesystem CLI output"),
                expected,
                "{id}: output bytes for {relative}"
            );
        }
        expected_paths.sort();
        assert_eq!(
            javascript_files(&tree.root),
            expected_paths,
            "{id}: output set"
        );
    }
}
