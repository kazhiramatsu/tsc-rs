use super::{read_regular_file_bounded, stage_no_replace, RelativePathV1, SourceSnapshotLimits};
use crate::{ByteLimit, EffectPhase, InfraError};
use std::fs;

#[test]
fn relative_paths_reject_traversal_and_symlink_reads_fail_closed() {
    assert!(RelativePathV1::try_new(b"a/../b".to_vec()).is_err());
    assert!(RelativePathV1::try_new(b"/absolute".to_vec()).is_err());
    assert!(RelativePathV1::try_new(b"a\\b".to_vec()).is_err());
    assert!(SourceSnapshotLimits::new(1, 1, 1).is_ok());

    let root = tempfile_dir();
    fs::write(root.join("plain"), b"ok").expect("write regular file");
    let path = RelativePathV1::try_new(b"plain".to_vec()).expect("valid relative path");
    let file =
        read_regular_file_bounded(&root, &path, ByteLimit::try_new(2).expect("positive limit"))
            .expect("regular file read");
    assert_eq!(file.as_bytes(), b"ok");
    let error =
        read_regular_file_bounded(&root, &path, ByteLimit::try_new(1).expect("positive limit"))
            .expect_err("over-limit read");
    assert_eq!(
        error,
        InfraError::Quota {
            phase: EffectPhase::Read
        }
    );

    #[cfg(unix)]
    {
        let limit = ByteLimit::try_new(16).expect("positive limit");
        std::os::unix::fs::symlink(root.join("plain"), root.join("leaf-link"))
            .expect("final-component symlink");
        let leaf = RelativePathV1::try_new(b"leaf-link".to_vec()).expect("valid relative path");
        assert_eq!(
            read_regular_file_bounded(&root, &leaf, limit).expect_err("symlink leaf read"),
            InfraError::Guard {
                phase: EffectPhase::Read
            }
        );

        fs::create_dir(root.join("sub")).expect("regular directory");
        fs::write(root.join("sub").join("inner"), b"ok").expect("nested regular file");
        std::os::unix::fs::symlink(root.join("sub"), root.join("dir-link"))
            .expect("intermediate directory symlink");
        let nested =
            RelativePathV1::try_new(b"dir-link/inner".to_vec()).expect("valid relative path");
        assert_eq!(
            read_regular_file_bounded(&root, &nested, limit).expect_err("symlink component read"),
            InfraError::Guard {
                phase: EffectPhase::Read
            }
        );
    }
}

#[test]
fn no_replace_stage_preserves_first_writer() {
    let root = tempfile_dir();
    let target = root.join("stage");
    let limit = ByteLimit::try_new(8).expect("positive limit");
    stage_no_replace(&target, b"first", limit).expect("first writer");
    let second = stage_no_replace(&target, b"second", limit).expect_err("no replacement");
    assert!(matches!(
        second,
        InfraError::Io {
            phase: EffectPhase::Commit,
            ..
        }
    ));
    assert_eq!(fs::read(target).expect("staged bytes"), b"first");
}

fn tempfile_dir() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "tsc-rs-fci-3c-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temporary directory");
    path
}
