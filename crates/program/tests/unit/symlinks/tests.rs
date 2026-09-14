use super::*;

#[test]
fn a_symlinked_package_file_yields_the_package_directory_link() {
    let guessed = guess_directory_symlink(
        "/.src/workspace/packageA/index.d.ts",
        "/.src/workspace/packageC/node_modules/package-a/index.d.ts",
        true,
    );
    assert_eq!(
        guessed,
        Some((
            "/.src/workspace/packageA".into(),
            "/.src/workspace/packageC/node_modules/package-a".into()
        ))
    );
}

#[test]
fn the_walk_stops_below_node_modules_and_scoped_directories() {
    assert_eq!(
        guess_directory_symlink(
            "/.src/monorepo/context/index.ts",
            "/.src/monorepo/node_modules/@loopback/context/index.ts",
            true,
        ),
        Some((
            "/.src/monorepo/context".into(),
            "/.src/monorepo/node_modules/@loopback/context".into()
        ))
    );
    assert_eq!(guess_directory_symlink("/a/x.ts", "/b/y.ts", true), None);
}

#[test]
fn different_lone_surrogate_tails_do_not_create_a_directory_alias() {
    let a = JsString::from_code_units(&[47, 97, 47, 0xd800, 46, 116, 115]);
    let b = JsString::from_code_units(&[47, 98, 47, 0xd801, 46, 116, 115]);
    assert_eq!(guess_directory_symlink(&a, &b, true), None);
    assert_eq!(guess_directory_symlink(&a, &b, false), None);
    assert_ne!(canonical(&a, false), canonical(&b, false));
    let a = JsString::from_code_units(&[47, 0xd800, 47, 120, 46, 116, 115]);
    let b = JsString::from_code_units(&[47, 0xd801, 47, 120, 46, 116, 115]);
    let (real, link) = guess_directory_symlink(&a, &b, true).expect("common filename");
    assert_eq!(real.to_utf16(), [47, 0xd800]);
    assert_eq!(link.to_utf16(), [47, 0xd801]);
}
