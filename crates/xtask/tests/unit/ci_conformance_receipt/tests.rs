use super::*;

fn temporary_workspace(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "tsc-rs-ci-receipt-{label}-{}-{}",
        std::process::id(),
        fresh_nonce().unwrap()
    ));
    fs::create_dir_all(&path).unwrap();
    path.canonicalize().unwrap()
}

fn invocation(workspace: &Path) -> Invocation {
    let outputs = OUTPUT_ROLES
        .iter()
        .map(|role| {
            let path = PathBuf::from(format!("{role}.json"));
            fs::write(workspace.join(&path), format!("{role}\n")).unwrap();
            OutputSpec {
                role: (*role).to_owned(),
                path,
            }
        })
        .collect();
    Invocation {
        workspace: workspace.to_owned(),
        receipt_path: PathBuf::from("receipt.json"),
        nonce: fresh_nonce().unwrap(),
        head: "a".repeat(40),
        producer_executable_sha256: "b".repeat(64),
        fingerprint_sha256: "c".repeat(64),
        started_unix_ms: now_unix_ms().unwrap(),
        outputs,
    }
}

#[test]
fn exact_receipt_round_trips_only_with_its_move_token() {
    let workspace = temporary_workspace("roundtrip");
    let invocation = invocation(&workspace);
    let guard = begin(&invocation).unwrap();
    let token = publish(guard, &invocation).unwrap();
    let receipt = consume(token, &invocation).unwrap();
    assert_eq!(
        receipt
            .receipt
            .bindings
            .iter()
            .map(|binding| binding.role.as_str())
            .collect::<Vec<_>>(),
        OUTPUT_ROLES
    );
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn output_tampering_and_wrong_order_fail_closed() {
    let workspace = temporary_workspace("tamper");
    let mut invocation = invocation(&workspace);
    let token = publish(begin(&invocation).unwrap(), &invocation).unwrap();
    fs::write(workspace.join(&invocation.outputs[1].path), b"tampered\n").unwrap();
    assert!(consume(token, &invocation).is_err());

    invocation.outputs.swap(0, 1);
    assert!(begin(&invocation).is_err());
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn old_nonce_and_receipt_byte_changes_are_rejected() {
    let workspace = temporary_workspace("nonce");
    let first = invocation(&workspace);
    let token = publish(begin(&first).unwrap(), &first).unwrap();
    let receipt_path = workspace.join(&first.receipt_path);
    let mut bytes = fs::read(&receipt_path).unwrap();
    bytes.push(b' ');
    fs::write(&receipt_path, bytes).unwrap();
    assert!(consume(token, &first).is_err());

    let mut stale = invocation(&workspace);
    stale.nonce = "d".repeat(64);
    let old_guard = begin(&stale).unwrap();
    stale.nonce = "e".repeat(64);
    assert!(publish(old_guard, &stale).is_err());
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn stale_token_duplicate_outputs_and_absolute_paths_are_rejected() {
    let workspace = temporary_workspace("identity");
    let first = invocation(&workspace);
    let old_token = publish(begin(&first).unwrap(), &first).unwrap();

    let mut second = invocation(&workspace);
    second.nonce = "d".repeat(64);
    let new_token = publish(begin(&second).unwrap(), &second).unwrap();
    assert!(consume(old_token, &first).is_err());
    consume(new_token, &second).unwrap();

    let mut duplicate = invocation(&workspace);
    duplicate.outputs[1].path = duplicate.outputs[0].path.clone();
    assert!(begin(&duplicate).is_err());

    let mut absolute = invocation(&workspace);
    absolute.receipt_path = workspace.join("absolute-receipt.json");
    assert!(begin(&absolute).is_err());
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn failed_output_binding_publishes_no_receipt() {
    let workspace = temporary_workspace("partial");
    let invocation = invocation(&workspace);
    let guard = begin(&invocation).unwrap();
    fs::remove_file(workspace.join(&invocation.outputs[2].path)).unwrap();
    assert!(publish(guard, &invocation).is_err());
    assert!(!workspace.join(&invocation.receipt_path).exists());
    fs::remove_dir_all(workspace).unwrap();
}

#[cfg(unix)]
#[test]
fn symlinked_outputs_are_rejected() {
    use std::os::unix::fs::symlink;

    let workspace = temporary_workspace("symlink");
    let first_invocation = invocation(&workspace);
    let first = workspace.join(&first_invocation.outputs[0].path);
    fs::remove_file(&first).unwrap();
    symlink(workspace.join(&first_invocation.outputs[1].path), &first).unwrap();
    assert!(begin(&first_invocation).is_err());
    fs::remove_file(&first).unwrap();
    fs::write(&first, b"all\n").unwrap();

    let outside = temporary_workspace("outside");
    let linked_parent = workspace.join("linked-parent");
    symlink(&outside, &linked_parent).unwrap();
    let mut parent_escape = invocation(&workspace);
    parent_escape.receipt_path = PathBuf::from("linked-parent/receipt.json");
    assert!(begin(&parent_escape).is_err());
    fs::remove_file(linked_parent).unwrap();
    fs::remove_dir_all(outside).unwrap();
    fs::remove_dir_all(workspace).unwrap();
}
