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
            "/.src/workspace/packageA".to_owned(),
            "/.src/workspace/packageC/node_modules/package-a".to_owned()
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
            "/.src/monorepo/context".to_owned(),
            "/.src/monorepo/node_modules/@loopback/context".to_owned()
        ))
    );
    assert_eq!(guess_directory_symlink("/a/x.ts", "/b/y.ts", true), None);
}
