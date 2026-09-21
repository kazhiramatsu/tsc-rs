use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

static TEMP_REPO_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TempRepo(PathBuf);

impl TempRepo {
    fn new() -> Self {
        let sequence = TEMP_REPO_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "tsc-rs-readme-status-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(path.join("tsrs2")).unwrap();
        let output = Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["init", "-q"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        fs::write(path.join("README.md"), "# test\n").unwrap();
        fs::create_dir_all(path.join("docs")).unwrap();
        fs::write(path.join("docs/verification-status.md"), "# status\n").unwrap();
        Self(path)
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn groups_thousands() {
    assert_eq!(group_thousands(0), "0");
    assert_eq!(group_thousands(999), "999");
    assert_eq!(group_thousands(1000), "1,000");
    assert_eq!(group_thousands(49024), "49,024");
    assert_eq!(group_thousands(1234567), "1,234,567");
}

#[test]
fn splices_between_markers() {
    let readme = format!(
        "# Title\n\nprose\n\n{README_STATUS_BEGIN}\nold body\n{README_STATUS_END}\n\ntail\n"
    );
    let spliced = splice_readme_status(&readme, "new body\n").unwrap();
    assert_eq!(
        spliced,
        format!(
            "# Title\n\nprose\n\n{README_STATUS_BEGIN}\nnew body\n{README_STATUS_END}\n\ntail\n"
        )
    );
    // Idempotent: splicing the same block changes nothing.
    assert_eq!(
        splice_readme_status(&spliced, "new body\n").unwrap(),
        spliced
    );
}

#[test]
fn rejects_missing_duplicate_or_reversed_markers() {
    assert!(splice_readme_status("no markers", "x").is_err());
    assert!(splice_readme_status(README_STATUS_BEGIN, "x").is_err());
    assert!(splice_readme_status(
        &format!("{README_STATUS_BEGIN}\n{README_STATUS_BEGIN}\n{README_STATUS_END}"),
        "x"
    )
    .is_err());
    assert!(
        splice_readme_status(&format!("{README_STATUS_END}\n{README_STATUS_BEGIN}"), "x").is_err()
    );
}

#[test]
fn internal_status_path_follows_the_git_root_and_keeps_the_user_readme_separate() {
    let repo = TempRepo::new();
    let nested = repo.0.join("tsrs2");
    let canonical_root = fs::canonicalize(&repo.0).unwrap();
    let status = canonical_root.join("docs/verification-status.md");

    assert_eq!(readme_path_for_workspace(&nested).unwrap(), status);
    assert_eq!(readme_path_for_workspace(&repo.0).unwrap(), status);
    assert_eq!(
        fs::read_to_string(canonical_root.join("README.md")).unwrap(),
        "# test\n"
    );
}
