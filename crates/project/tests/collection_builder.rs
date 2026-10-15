//! tsgo's project collection builder tests
//! (`project/projectcollectionbuilder_test.go`): the default projects the
//! session finds for opened documents through solutions and references,
//! and the configs it keeps while they are open.

mod common;

use std::sync::Arc;

use common::*;
use tsc_host::vfs::FileSystem;
use tsc_lsproto::{
    FileChangeType, TextDocumentContentChangePartialOrWholeDocument,
    TextDocumentContentChangeWholeDocument,
};
use tsc_project::{ProjectKind, ProjectTreeRequest, Session, Snapshot};

const MY_PROJECT: &str = "/user/username/projects/myproject";
const MAIN: &str = "file:///user/username/projects/myproject/src/main.ts";
const MAIN_TEXT: &str = "
			import { foo } from './helpers/functions';
			export { foo };";
const DUMMY: &str = "file:///user/username/workspaces/dummy/dummy.ts";

/// tsgo `filesForSolutionConfigFile`.
fn files_for_solution_config_file(
    solution_references: &[&str],
    compiler_options: &str,
    own_files: &[&str],
) -> Vec<(String, String)> {
    let compiler_options = if compiler_options.is_empty() {
        String::new()
    } else {
        format!("\"compilerOptions\": {{\n\t\t\t{compiler_options}\n\t\t}},")
    };
    let references = solution_references
        .iter()
        .map(|reference| format!("{{ \"path\": \"{reference}\" }}"))
        .collect::<Vec<_>>()
        .join(",");
    vec![
        (
            format!("{MY_PROJECT}/tsconfig.json"),
            format!(
                "{{\n\t\t\t{compiler_options}\n\t\t\t\"files\": [{}],\n\t\t\t\"references\": [\n\t\t\t\t{references}\n\t\t\t]\n\t\t}}",
                own_files.join(",")
            ),
        ),
        (
            format!("{MY_PROJECT}/tsconfig-src.json"),
            r#"{
			"compilerOptions": {
				"composite": true,
				"outDir": "./target",
			},
			"include": ["./src/**/*"]
		}"#
            .to_owned(),
        ),
        (format!("{MY_PROJECT}/src/main.ts"), MAIN_TEXT.to_owned()),
        (
            format!("{MY_PROJECT}/src/helpers/functions.ts"),
            "export const foo = 1;".to_owned(),
        ),
    ]
}

/// tsgo `applyIndirectProjectFiles`.
fn apply_indirect_project_files(
    files: &mut Vec<(String, String)>,
    index: usize,
    compiler_options: &str,
) {
    files.push((
        format!("{MY_PROJECT}/tsconfig-indirect{index}.json"),
        format!(
            r#"{{
			"compilerOptions": {{
				"composite": true,
				"outDir": "./target/",
				{compiler_options}
			}},
			"files": [
				"./indirect{index}/main.ts"
			],
			"references": [
				{{
				"path": "./tsconfig-src.json"
				}}
			]
		}}"#
        ),
    ));
    files.push((
        format!("{MY_PROJECT}/indirect{index}/main.ts"),
        "export const indirect = 1;".to_owned(),
    ));
}

fn setup_owned(files: &[(String, String)]) -> Session {
    let files = files
        .iter()
        .map(|(name, text)| (name.as_str(), text.as_str()))
        .collect::<Vec<_>>();
    setup(&files).0
}

fn has_config(snapshot: &Snapshot, name: &str) -> bool {
    snapshot.config(&format!("{MY_PROJECT}/{name}")).is_some()
}

fn my_project(name: &str) -> tsc_project::ProjectId {
    configured(&format!("{MY_PROJECT}/{name}"))
}

/// The default project of `document` in `snapshot`.
fn default_project<'a>(
    snapshot: &'a Snapshot,
    document: &str,
) -> Option<&'a Arc<tsc_project::Project>> {
    let file_name = tsc_program::uri::uri_to_file_name(document).expect("a file name");
    snapshot.default_project(&file_name)
}

/// tsgo's last steps of the solution tests: the file closed and another
/// opened in the inferred project release the configs.
fn close_and_open_dummy(session: &Session, configs: &[&str]) {
    session.did_close_file(&uri(MAIN));
    open(session, DUMMY, "const x = 1;");
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    assert!(snapshot.inferred_project().is_some());
    for config in configs {
        assert!(!has_config(&snapshot, config), "{config} released");
    }
}

#[test]
fn a_solution_referencing_the_default_project_directly() {
    // "when project found is solution referencing default project
    // directly".
    let files = files_for_solution_config_file(&["./tsconfig-src.json"], "", &[]);
    let session = setup_owned(&files);
    open(&session, MAIN, MAIN_TEXT);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    assert!(snapshot.project(&my_project("tsconfig-src.json")).is_some());

    // A request uses the snapshot as it is.
    session
        .get_snapshot_and_default_project(&uri(MAIN))
        .expect("the project of main.ts");
    assert!(Arc::ptr_eq(&session.snapshot(), &snapshot));

    assert!(has_config(&snapshot, "tsconfig.json"), "solution config");
    assert!(
        has_config(&snapshot, "tsconfig-src.json"),
        "direct reference"
    );
    close_and_open_dummy(&session, &["tsconfig.json", "tsconfig-src.json"]);
}

#[test]
fn a_solution_referencing_the_default_project_indirectly() {
    // "when project found is solution referencing default project
    // indirectly".
    let mut files = files_for_solution_config_file(
        &["./tsconfig-indirect1.json", "./tsconfig-indirect2.json"],
        "",
        &[],
    );
    apply_indirect_project_files(&mut files, 1, "");
    apply_indirect_project_files(&mut files, 2, "");
    let session = setup_owned(&files);
    open(&session, MAIN, MAIN_TEXT);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    let src = snapshot
        .project(&my_project("tsconfig-src.json"))
        .expect("the source project");
    assert!(Arc::ptr_eq(default_project(&snapshot, MAIN).unwrap(), src));
    assert!(has_config(&snapshot, "tsconfig.json"), "solution config");
    assert!(
        has_config(&snapshot, "tsconfig-indirect1.json"),
        "direct reference"
    );
    assert!(
        has_config(&snapshot, "tsconfig-src.json"),
        "indirect reference"
    );
    close_and_open_dummy(
        &session,
        &[
            "tsconfig.json",
            "tsconfig-src.json",
            "tsconfig-indirect1.json",
            "tsconfig-indirect2.json",
        ],
    );
}

#[test]
fn a_solution_with_disable_referenced_project_load() {
    // "when project found is solution with disableReferencedProjectLoad
    // referencing default project directly".
    let files = files_for_solution_config_file(
        &["./tsconfig-src.json"],
        r#""disableReferencedProjectLoad": true"#,
        &[],
    );
    let session = setup_owned(&files);
    open(&session, MAIN, MAIN_TEXT);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    assert!(snapshot.project(&my_project("tsconfig-src.json")).is_none());
    assert_eq!(
        default_project(&snapshot, MAIN)
            .expect("a default project")
            .kind(),
        ProjectKind::Inferred
    );
    assert!(has_config(&snapshot, "tsconfig.json"), "solution config");
    assert!(
        !has_config(&snapshot, "tsconfig-src.json"),
        "direct reference"
    );
    close_and_open_dummy(&session, &["tsconfig.json", "tsconfig-src.json"]);
}

#[test]
fn an_indirect_reference_through_disable_referenced_project_load() {
    // "when project found is solution referencing default project
    // indirectly through disableReferencedProjectLoad".
    let mut files = files_for_solution_config_file(&["./tsconfig-indirect1.json"], "", &[]);
    apply_indirect_project_files(&mut files, 1, r#""disableReferencedProjectLoad": true"#);
    let session = setup_owned(&files);
    open(&session, MAIN, MAIN_TEXT);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    assert!(snapshot.project(&my_project("tsconfig-src.json")).is_none());
    assert_eq!(
        default_project(&snapshot, MAIN)
            .expect("a default project")
            .kind(),
        ProjectKind::Inferred
    );
    assert!(has_config(&snapshot, "tsconfig.json"), "solution config");
    assert!(
        has_config(&snapshot, "tsconfig-indirect1.json"),
        "direct reference"
    );
    assert!(
        !has_config(&snapshot, "tsconfig-src.json"),
        "indirect reference"
    );
    close_and_open_dummy(
        &session,
        &[
            "tsconfig.json",
            "tsconfig-src.json",
            "tsconfig-indirect1.json",
        ],
    );
}

#[test]
fn an_indirect_reference_without_disable_referenced_project_load_in_one() {
    // "when project found is solution referencing default project
    // indirectly through disableReferencedProjectLoad in one but without
    // it in another".
    let mut files = files_for_solution_config_file(
        &["./tsconfig-indirect1.json", "./tsconfig-indirect2.json"],
        "",
        &[],
    );
    apply_indirect_project_files(&mut files, 1, r#""disableReferencedProjectLoad": true"#);
    apply_indirect_project_files(&mut files, 2, "");
    let session = setup_owned(&files);
    open(&session, MAIN, MAIN_TEXT);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    let src = snapshot
        .project(&my_project("tsconfig-src.json"))
        .expect("the source project");
    assert!(Arc::ptr_eq(default_project(&snapshot, MAIN).unwrap(), src));
    for config in [
        "tsconfig.json",
        "tsconfig-indirect1.json",
        "tsconfig-indirect2.json",
        "tsconfig-src.json",
    ] {
        assert!(has_config(&snapshot, config), "{config}");
    }
    close_and_open_dummy(
        &session,
        &[
            "tsconfig.json",
            "tsconfig-src.json",
            "tsconfig-indirect1.json",
            "tsconfig-indirect2.json",
        ],
    );
}

#[test]
fn a_project_with_own_files_referencing_the_file() {
    // "when project found is project with own files referencing the file
    // from referenced project".
    let mut files =
        files_for_solution_config_file(&["./tsconfig-src.json"], "", &[r#""./own/main.ts""#]);
    files.push((
        format!("{MY_PROJECT}/own/main.ts"),
        "
			import { foo } from '../src/main';
			foo;
			export function bar() {}
		"
        .to_owned(),
    ));
    let session = setup_owned(&files);
    open(&session, MAIN, MAIN_TEXT);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 2);
    let src = snapshot
        .project(&my_project("tsconfig-src.json"))
        .expect("the source project");
    assert!(snapshot.project(&my_project("tsconfig.json")).is_some());
    assert!(Arc::ptr_eq(default_project(&snapshot, MAIN).unwrap(), src));
    assert!(has_config(&snapshot, "tsconfig.json"), "solution config");
    assert!(
        has_config(&snapshot, "tsconfig-src.json"),
        "direct reference"
    );
    close_and_open_dummy(&session, &["tsconfig.json", "tsconfig-src.json"]);
}

#[test]
fn an_ancestor_solution_finds_the_default_project() {
    // "when file is not part of first config tree found, looks into
    // ancestor folder and its references to find default project".
    let files = [
        (
            "/home/src/projects/project/app/Component-demos.ts",
            "
                import * as helpers from 'demos/helpers';
                export const demo = () => {
                    helpers;
                }
            ",
        ),
        (
            "/home/src/projects/project/app/Component.ts",
            "export const Component = () => {}",
        ),
        (
            "/home/src/projects/project/app/tsconfig.json",
            r#"{
				"compilerOptions": {
					"composite": true,
					"outDir": "../app-dist/",
				},
				"include": ["**/*"],
				"exclude": ["**/*-demos.*"],
			}"#,
        ),
        (
            "/home/src/projects/project/demos/helpers.ts",
            "export const foo = 1;",
        ),
        (
            "/home/src/projects/project/demos/tsconfig.json",
            r#"{
				"compilerOptions": {
					"composite": true,
					"rootDir": "../",
					"outDir": "../demos-dist/",
					"paths": {
						"demos/*": ["./*"],
					},
				},
				"include": [
					"**/*",
					"../app/**/*-demos.*",
				],
			}"#,
        ),
        (
            "/home/src/projects/project/tsconfig.json",
            r#"{
				"compilerOptions": {
					"outDir": "./dist/",
				},
				"references": [
					{ "path": "./demos/tsconfig.json" },
					{ "path": "./app/tsconfig.json" },
				],
				"files": []
			}"#,
        ),
    ];
    let (session, _) = setup(&files);
    let document = "file:///home/src/projects/project/app/Component-demos.ts";
    open(&session, document, files[0].1);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 2);
    let demos = snapshot
        .project(&configured(
            "/home/src/projects/project/demos/tsconfig.json",
        ))
        .expect("the demos project");
    assert!(snapshot
        .project(&configured("/home/src/projects/project/tsconfig.json"))
        .is_some());
    assert!(Arc::ptr_eq(
        default_project(&snapshot, document).unwrap(),
        demos
    ));
    let configs = [
        "/home/src/projects/project/app/tsconfig.json",
        "/home/src/projects/project/demos/tsconfig.json",
        "/home/src/projects/project/tsconfig.json",
    ];
    for config in configs {
        assert!(snapshot.config(config).is_some(), "{config}");
    }

    session.did_close_file(&uri(document));
    open(&session, DUMMY, "const x = 1;");
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    assert!(snapshot.inferred_project().is_some());
    for config in configs {
        assert!(snapshot.config(config).is_none(), "{config} released");
    }
}

#[test]
fn a_declaration_next_to_its_source_in_a_referenced_project() {
    // "when dts file is next to ts file and included as root in referenced
    // project".
    let files = [
        (
            "/home/src/projects/project/src/index.d.ts",
            "
                 declare global {
                    interface Window {
                        electron: ElectronAPI
                        api: unknown
                    }
                }
            ",
        ),
        ("/home/src/projects/project/src/index.ts", "const api = {}"),
        (
            "/home/src/projects/project/tsconfig.json",
            r#"{
				"include": [
					"src/*.d.ts",
				],
				"references": [{ "path": "./tsconfig.node.json" }],
			}"#,
        ),
        (
            "/home/src/projects/project/tsconfig.node.json",
            r#"{
				"include": ["src/**/*"],
                "compilerOptions": {
                    "composite": true,
                },
			}"#,
        ),
    ];
    let (session, _) = setup(&files);
    let document = "file:///home/src/projects/project/src/index.d.ts";
    open(&session, document, files[0].1);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 2);
    assert!(snapshot
        .project(&configured("/home/src/projects/project/tsconfig.json"))
        .is_some());
    assert_eq!(
        default_project(&snapshot, document)
            .expect("a default project")
            .kind(),
        ProjectKind::Inferred
    );
    let configs = [
        "/home/src/projects/project/tsconfig.json",
        "/home/src/projects/project/tsconfig.node.json",
    ];
    for config in configs {
        assert!(snapshot.config(config).is_some(), "{config}");
    }

    session.did_close_file(&uri(document));
    open(&session, DUMMY, "const x = 1;");
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    assert!(snapshot.inferred_project().is_some());
    for config in configs {
        assert!(snapshot.config(config).is_none(), "{config} released");
    }
}

#[test]
fn issue_1630_a_config_kept_for_an_open_file() {
    // "#1630": opening index.ts searches /project/tsconfig.json and checks
    // /project/lib/tsconfig.json, kept for b.ts, without opening it.
    let files = [
        ("/project/lib/tsconfig.json", r#"{ "files": ["a.ts"] }"#),
        ("/project/lib/a.ts", "export const a = 1;"),
        ("/project/lib/b.ts", "export const b = 1;"),
        (
            "/project/tsconfig.json",
            r#"{
				"files": [],
				"references": [{ "path": "./lib" }],
				"compilerOptions": {
					"disableReferencedProjectLoad": true
				}
			}"#,
        ),
        ("/project/index.ts", ""),
    ];
    let (session, _) = setup(&files);
    open(&session, "file:///project/lib/b.ts", files[2].1);
    open(&session, "untitled:Untitled-1", "");
    open(&session, "file:///project/index.ts", files[4].1);
}

#[test]
fn inferred_project_roots_are_in_stable_order() {
    // "inferred project root files are in stable order".
    let files = [
        ("/project/a.ts", "export const a = 1;"),
        ("/project/b.ts", "export const b = 1;"),
        ("/project/c.ts", "export const c = 1;"),
    ];
    let (session, _) = setup(&files);
    open(&session, "file:///project/b.ts", files[1].1);
    open(&session, "file:///project/c.ts", files[2].1);
    open(&session, "file:///project/a.ts", files[0].1);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    let inferred = snapshot.inferred_project().expect("the inferred project");
    assert_eq!(
        inferred.root_file_names(),
        ["/project/a.ts", "/project/b.ts", "/project/c.ts"]
    );
}

#[test]
fn project_lookup_terminates() {
    // "project lookup terminates": references in a cycle.
    let files = [
        (
            "/tsconfig.json",
            r#"{
				"files": [],
				"references": [
					{ "path": "./packages/pkg1" },
					{ "path": "./packages/pkg2" },
				]
			}"#,
        ),
        (
            "/packages/pkg1/tsconfig.json",
            r#"{
				"include": ["src/**/*.ts"],
				"compilerOptions": { "composite": true },
				"references": [{ "path": "../pkg2" }]
			}"#,
        ),
        (
            "/packages/pkg2/tsconfig.json",
            r#"{
				"include": ["src/**/*.ts"],
				"compilerOptions": { "composite": true },
				"references": [{ "path": "../pkg1" }]
			}"#,
        ),
        ("/script.ts", "export const a = 1;"),
    ];
    let (session, _) = setup(&files);
    open(&session, "file:///script.ts", files[3].1);
}

#[test]
fn a_file_moves_to_the_inferred_project_after_its_import_is_deleted() {
    // "file moves to inferred project after import is deleted".
    let files = [
        (
            "/project/tsconfig.json",
            r#"{"compilerOptions": {"strict": true}}"#,
        ),
        (
            "/project/index.ts",
            r#"import { helper } from "./node_modules/dep/index";"#,
        ),
        (
            "/project/node_modules/dep/index.d.ts",
            "export declare function helper(): void;",
        ),
    ];
    let (session, _) = setup(&files);
    let root = "file:///project/index.ts";
    open(&session, root, files[1].1);
    session
        .get_snapshot_and_default_project(&uri(root))
        .expect("the project of index.ts");

    let dependency = "file:///project/node_modules/dep/index.d.ts";
    open(&session, dependency, files[2].1);
    let snapshot = session.snapshot();
    let project = snapshot
        .project(&configured("/project/tsconfig.json"))
        .expect("the configured project");
    assert!(Arc::ptr_eq(
        default_project(&snapshot, dependency).unwrap(),
        project
    ));

    session.did_change_file(
        &uri(root),
        2,
        vec![
            TextDocumentContentChangePartialOrWholeDocument::WholeDocument(
                TextDocumentContentChangeWholeDocument {
                    text: "// import removed".to_owned(),
                },
            ),
        ],
    );
    let (snapshot, _) = session
        .get_snapshot_and_default_project(&uri(dependency))
        .expect("the dependency's project");
    assert_eq!(
        default_project(&snapshot, dependency)
            .expect("a default project")
            .kind(),
        ProjectKind::Inferred
    );
}

#[test]
fn a_package_json_change_updates_the_project() {
    // "should update project on package.json change": the package.json
    // is a location the resolution depends on.
    let files = [
        (
            "/home/projects/myproject/tsconfig.json",
            r#"{
				"compilerOptions": {
					"module": "nodenext",
					"moduleResolution": "nodenext",
					"noLib": true,
					"noEmit": true
				}
			}"#,
        ),
        (
            "/home/projects/myproject/package.json",
            r##"{
				"name": "myproject",
				"type": "module",
				"imports": {
					"#utils": "./src/utils.ts"
				}
			}"##,
        ),
        (
            "/home/projects/myproject/src/index.ts",
            r##"import { add } from "#utils";"##,
        ),
        (
            "/home/projects/myproject/src/utils.ts",
            "export function add(a: number, b: number) { return a + b; }",
        ),
    ];
    let (session, fs) = setup(&files);
    let index = "file:///home/projects/myproject/src/index.ts";
    open(&session, index, files[2].1);
    let semantic_diagnostics = |session: &Session| {
        program(session, index).with_live(|live| {
            let mut count = 0;
            for file in 0..live.file_count() {
                count += live
                    .semantic_diagnostics(file)
                    .expect("check the file")
                    .len();
            }
            count
        })
    };
    assert_eq!(semantic_diagnostics(&session), 0);

    fs.write(
        "/home/projects/myproject/package.json",
        br##"{
			"name": "myproject",
			"type": "module",
			"imports": {
				"#utils": "./src/nonexistent.ts"
			}
		}"##,
    )
    .expect("write the package.json");
    session.did_change_watched_files(&[watch_event(
        "file:///home/projects/myproject/package.json",
        FileChangeType::CHANGED,
    )]);
    assert_eq!(semantic_diagnostics(&session), 1);
}

#[test]
fn a_default_project_search_skips_an_unloaded_reference() {
    // tsgo `TestProjectCollectionDefaultProject`: project1's output is in
    // the programs of project3 and project4, and the search for its
    // default project does not reach project2, not loaded under
    // `disableReferencedProjectLoad`.
    let project = |references: &str| {
        format!(
            r#"{{
			"extends": "../tsconfig.json",
			"files": [],
			"include": ["src/**/*"],
			{references}
			"compilerOptions": {{
				"composite": true,
				"outDir": "./dist",
				"rootDir": "./src",
			}}
		}}"#
        )
    };
    let references = |path: &str| format!(r#""references": [{{ "path": "{path}" }}],"#);
    let user = "import { Bar } from \"../../project1/dist/index.js\";\ndeclare const b: Bar;\nconst x: string = b.a;";
    let files = [
        ("/project1/tsconfig.json".to_owned(), project(&references("../project2"))),
        (
            "/project1/src/index.ts".to_owned(),
            "export const foo = 42;\n\t\texport type Bar = { a: string };".to_owned(),
        ),
        (
            "/project1/dist/index.d.ts".to_owned(),
            "export declare const foo = 42;\n\t\t\texport type Bar = {\n\t\t\t\ta: string;\n\t\t\t};"
                .to_owned(),
        ),
        ("/project2/tsconfig.json".to_owned(), project("")),
        ("/project3/tsconfig.json".to_owned(), project(&references("../project1"))),
        ("/project3/src/index.ts".to_owned(), user.to_owned()),
        ("/project4/tsconfig.json".to_owned(), project(&references("../project1"))),
        ("/project4/src/index.ts".to_owned(), user.to_owned()),
        (
            "/tsconfig.json".to_owned(),
            r#"{
			"compilerOptions": {
				"disableReferencedProjectLoad": true,
				"disableSolutionSearching": true,
				"disableSourceOfProjectReferenceRedirect": true
			},
			"files": [],
			"references": [
				{ "path": "./project1" },
				{ "path": "./project2" },
				{ "path": "./project3" },
				{ "path": "./project4" }
			]
		}"#
            .to_owned(),
        ),
    ];
    let session = setup_owned(&files);
    for file_name in [
        "/project1/dist/index.d.ts",
        "/project1/src/index.ts",
        "/project3/src/index.ts",
        "/project4/src/index.ts",
    ] {
        let text = &files
            .iter()
            .find(|(name, _)| name == file_name)
            .expect("the file")
            .1;
        open(&session, &format!("file://{file_name}"), text);
    }
}

#[test]
fn a_project_tree_request_loads_the_trees_referencing_its_projects() {
    // tsgo `DidRequestProjectTrees` for some projects (what the search
    // across projects asks once its default definition is visible outside
    // its project): a referenced project is loaded only when it references
    // one of them, directly or below; every tree loads all.
    let composite = |references: &str| {
        format!(
            r#"{{ "compilerOptions": {{ "composite": true }}, {references} "include": ["*.ts"] }}"#
        )
    };
    let files = [
        (
            "/p/a/tsconfig.json".to_owned(),
            composite(r#""references": [{ "path": "../b" }],"#),
        ),
        ("/p/a/a.ts".to_owned(), "export const a = 1;".to_owned()),
        (
            "/p/b/tsconfig.json".to_owned(),
            composite(r#""references": [{ "path": "../d" }],"#),
        ),
        ("/p/b/b.ts".to_owned(), "export const b = 1;".to_owned()),
        ("/p/d/tsconfig.json".to_owned(), composite("")),
        ("/p/d/d.ts".to_owned(), "export const d = 1;".to_owned()),
    ];
    let session = setup_owned(&files);
    open(&session, "file:///p/a/a.ts", "export const a = 1;");
    let projects = |snapshot: &Snapshot| {
        snapshot
            .projects()
            .map(|project| project.id().clone())
            .collect::<Vec<_>>()
    };
    let [a, b, d] = [
        "/p/a/tsconfig.json",
        "/p/b/tsconfig.json",
        "/p/d/tsconfig.json",
    ]
    .map(configured);
    assert_eq!(projects(&session.snapshot()), std::slice::from_ref(&a));
    let referencing = |id: &tsc_project::ProjectId| {
        ProjectTreeRequest::referencing(std::collections::BTreeSet::from([id.as_str().to_owned()]))
    };
    // `b` references nothing that is asked for.
    let snapshot = session
        .get_snapshot_loading_project_trees(referencing(&b))
        .expect("a snapshot");
    assert_eq!(projects(&snapshot), std::slice::from_ref(&a));
    // `b` references `d`; `d` itself is not loaded.
    let snapshot = session
        .get_snapshot_loading_project_trees(referencing(&d))
        .expect("a snapshot");
    assert_eq!(projects(&snapshot), [a.clone(), b.clone()]);
    let snapshot = session
        .get_snapshot_loading_project_trees(ProjectTreeRequest::all())
        .expect("a snapshot");
    assert_eq!(projects(&snapshot), [a, b, d]);
}
