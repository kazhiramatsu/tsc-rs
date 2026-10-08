//! The inputs of tsgo's command-line and tsconfig parsing tests, transcribed
//! from the Go tables in `tsc/internal/tsoptions/commandlineparser_test.go`
//! and `tsconfigparsing_test.go` at the profile's commit; each table names
//! its source lines. The baselines repeat the arguments, the files and the
//! texts, so a transcription error shows as a difference.

/// The kind of the tests' tsconfig-only `optionName` declaration
/// (`createVerifyNullForNonNullIncluded`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ExtraOptionKind {
    String,
    Number,
}

/// `verifyNull`: the null scenarios of one option.
#[derive(Clone, Copy, Debug)]
pub(super) struct VerifyNull {
    pub(super) sub_scenario: &'static str,
    pub(super) option_name: &'static str,
    /// Empty when the option has no non-null scenario.
    pub(super) non_null_value: &'static str,
    pub(super) extra_option: Option<ExtraOptionKind>,
}

/// `parseJsonConfigTestCase`.
#[derive(Clone, Copy, Debug)]
pub(super) struct ParseJsonConfigCase {
    pub(super) title: &'static str,
    pub(super) include_compiler_options: bool,
    pub(super) input: &'static [TestConfig],
}

/// `testConfig` (no transcribed table sets `existingOptions`).
#[derive(Clone, Copy, Debug)]
pub(super) struct TestConfig {
    pub(super) json_text: &'static str,
    pub(super) config_file_name: &'static str,
    /// Empty when the Go literal leaves `basePath` unset.
    pub(super) base_path: &'static str,
    pub(super) all_file_list: &'static [(&'static str, &'static str)],
}

/// `TestCommandLineParseResult` (commandlineparser_test.go:28-82): name and arguments.
pub(super) const PARSE_COMMAND_LINE: &[(&str, &[&str])] = &[
    (
        "Parse single option of library flag",
        &["--lib", "es6", "0.ts"],
    ),
    (
        "Handles may only be used with --build flags",
        &["--build", "--clean", "--dry", "--force", "--verbose"],
    ),
    (
        "Handles did you mean for misspelt flags",
        &["--declarations", "--allowTS"],
    ),
    (
        "Parse multiple options of library flags",
        &["--lib", "es5,es2015.symbol.wellknown", "0.ts"],
    ),
    (
        "Parse invalid option of library flags",
        &["--lib", "es5,invalidOption", "0.ts"],
    ),
    ("Parse empty options of --jsx", &["0.ts", "--jsx"]),
    ("Parse empty options of --module", &["0.ts", "--module"]),
    ("Parse empty options of --newLine", &["0.ts", "--newLine"]),
    ("Parse empty options of --target", &["0.ts", "--target"]),
    (
        "Parse empty options of --moduleResolution",
        &["0.ts", "--moduleResolution"],
    ),
    ("Parse empty options of --lib", &["0.ts", "--lib"]),
    ("Parse empty string of --lib", &["0.ts", "--lib", ""]),
    (
        "Parse immediately following command line argument of --lib",
        &["0.ts", "--lib", "--sourcemap"],
    ),
    (
        "Parse --lib option with extra comma",
        &["--lib", "es5,", "es7", "0.ts"],
    ),
    (
        "Parse --lib option with trailing white-space",
        &["--lib", "es5, ", "es7", "0.ts"],
    ),
    (
        "Parse multiple compiler flags with input files at the end",
        &[
            "--lib",
            "es5,es2015.symbol.wellknown",
            "--target",
            "es5",
            "0.ts",
        ],
    ),
    (
        "Parse multiple compiler flags with input files in the middle",
        &[
            "--module",
            "commonjs",
            "--target",
            "es5",
            "0.ts",
            "--lib",
            "es5,es2015.symbol.wellknown",
        ],
    ),
    (
        "Parse multiple library compiler flags ",
        &[
            "--module",
            "commonjs",
            "--target",
            "es5",
            "--lib",
            "es5",
            "0.ts",
            "--lib",
            "es2015.core, es2015.symbol.wellknown ",
        ],
    ),
    (
        "Parse explicit boolean flag value",
        &["--strictNullChecks", "false", "0.ts"],
    ),
    (
        "Parse non boolean argument after boolean flag",
        &["--noImplicitAny", "t", "0.ts"],
    ),
    ("Parse implicit boolean flag value", &["--strictNullChecks"]),
    ("parse --incremental", &["--incremental", "0.ts"]),
    (
        "parse --tsBuildInfoFile",
        &["--tsBuildInfoFile", "build.tsbuildinfo", "0.ts"],
    ),
    (
        "allows tsconfig only option to be set to null",
        &["--composite", "null", "-tsBuildInfoFile", "null", "0.ts"],
    ),
    ("parse --watchFile", &["--watchFile", "UseFsEvents", "0.ts"]),
    (
        "parse --watchDirectory",
        &["--watchDirectory", "FixedPollingInterval", "0.ts"],
    ),
    (
        "parse --fallbackPolling",
        &["--fallbackPolling", "PriorityInterval", "0.ts"],
    ),
    (
        "parse --synchronousWatchDirectory",
        &["--synchronousWatchDirectory", "0.ts"],
    ),
    (
        "errors on missing argument to --fallbackPolling",
        &["0.ts", "--fallbackPolling"],
    ),
    (
        "parse --excludeDirectories",
        &["--excludeDirectories", "**/temp", "0.ts"],
    ),
    (
        "errors on invalid excludeDirectories",
        &["--excludeDirectories", "**/../*", "0.ts"],
    ),
    (
        "parse --excludeFiles",
        &["--excludeFiles", "**/temp/*.ts", "0.ts"],
    ),
    (
        "errors on invalid excludeFiles",
        &["--excludeFiles", "**/../*", "0.ts"],
    ),
];

/// `TestParseCommandLineVerifyNull`'s first scenario (commandlineparser_test.go:200).
pub(super) const PARSE_COMMAND_LINE_FALSE: (&str, &[&str]) = (
    "allows setting option type boolean to false",
    &["--composite", "false", "0.ts"],
);

/// `TestParseCommandLineVerifyNull`'s `verifyNullSubScenarios` (commandlineparser_test.go:202-221).
pub(super) const VERIFY_NULL: &[VerifyNull] = &[
    VerifyNull {
        sub_scenario: "option of type boolean",
        option_name: "composite",
        non_null_value: "true",
        extra_option: None,
    },
    VerifyNull {
        sub_scenario: "option of type object",
        option_name: "paths",
        non_null_value: "",
        extra_option: None,
    },
    VerifyNull {
        sub_scenario: "option of type list",
        option_name: "rootDirs",
        non_null_value: "abc,xyz",
        extra_option: None,
    },
    VerifyNull {
        sub_scenario: "option of type string",
        option_name: "optionName",
        non_null_value: "hello",
        extra_option: Some(ExtraOptionKind::String),
    },
    VerifyNull {
        sub_scenario: "option of type number",
        option_name: "optionName",
        non_null_value: "10",
        extra_option: Some(ExtraOptionKind::Number),
    },
];

/// `TestParseBuildCommandLine`'s scenarios, then its `extraScenarios`
/// (commandlineparser_test.go:543-566, 572-578).
pub(super) const PARSE_BUILD_OPTIONS: &[(&str, &[&str])] = &[
    ("parse build without any options ", &[]),
    ("Parse multiple options", &["--verbose", "--force", "tests"]),
    (
        "Parse option with invalid option",
        &["--verbose", "--invalidOption"],
    ),
    (
        "Parse multiple flags with input projects at the end",
        &["--force", "--verbose", "src", "tests"],
    ),
    (
        "Parse multiple flags with input projects in the middle",
        &["--force", "src", "tests", "--verbose"],
    ),
    (
        "Parse multiple flags with input projects in the beginning",
        &["src", "tests", "--force", "--verbose"],
    ),
    (
        "parse build with --incremental",
        &["--incremental", "tests"],
    ),
    (
        "parse build with --locale en-us",
        &["--locale", "en-us", "src"],
    ),
    (
        "parse build with --tsBuildInfoFile",
        &["--tsBuildInfoFile", "build.tsbuildinfo", "tests"],
    ),
    (
        "reports other common may not be used with --build flags",
        &["--strict"],
    ),
    (
        "--clean and --force together is invalid",
        &["--clean", "--force"],
    ),
    (
        "--clean and --verbose together is invalid",
        &["--clean", "--verbose"],
    ),
    (
        "--clean and --watch together is invalid",
        &["--clean", "--watch"],
    ),
    (
        "--watch and --dry together is invalid",
        &["--watch", "--dry"],
    ),
    (
        "parse --watchFile",
        &["--watchFile", "UseFsEvents", "--verbose"],
    ),
    (
        "parse --watchDirectory",
        &["--watchDirectory", "FixedPollingInterval", "--verbose"],
    ),
    (
        "parse --fallbackPolling",
        &["--fallbackPolling", "PriorityInterval", "--verbose"],
    ),
    (
        "parse --synchronousWatchDirectory",
        &["--synchronousWatchDirectory", "--verbose"],
    ),
    (
        "errors on missing argument",
        &["--verbose", "--fallbackPolling"],
    ),
    (
        "errors on invalid excludeDirectories",
        &["--excludeDirectories", "**/../*"],
    ),
    ("parse --excludeFiles", &["--excludeFiles", "**/temp/*.ts"]),
    (
        "errors on invalid excludeFiles",
        &["--excludeFiles", "**/../*"],
    ),
    ("parse --builders", &["--builders", "2"]),
    (
        "--singleThreaded and --builders together",
        &["--singleThreaded", "--builders", "2"],
    ),
    ("reports error when --builders is 0", &["--builders", "0"]),
    (
        "reports error when --builders is negative",
        &["--builders", "-1"],
    ),
    (
        "reports error when --builders is invalid type",
        &["--builders", "invalid"],
    ),
];

/// `parseConfigFileTextToJsonTests` (tsconfigparsing_test.go:39-127).
pub(super) const JSON_PARSE: &[(&str, &[&str])] = &[
    (
        "returns empty config for file with only whitespaces",
        &["", " "],
    ),
    (
        "returns empty config for file with comments only",
        &["// Comment", "/* Comment*/"],
    ),
    ("returns empty config when config is empty object", &["{}"]),
    (
        "returns config object without comments",
        &[
            r#"{ // Excluded files
            "exclude": [
                // Exclude d.ts
                "file.d.ts"
            ]
        }"#,
            r#"{
            /* Excluded
                    Files
            */
            "exclude": [
                /* multiline comments can be in the middle of a line */"file.d.ts"
            ]
        }"#,
        ],
    ),
    (
        "keeps string content untouched",
        &[
            r#"{
            "exclude": [
                "xx//file.d.ts"
            ]
        }"#,
            r#"{
            "exclude": [
                "xx/*file.d.ts*/"
            ]
        }"#,
        ],
    ),
    (
        "handles escaped characters in strings correctly",
        &[
            r#"{
            "exclude": [
                "xx\"//files"
            ]
        }"#,
            r#"{
            "exclude": [
                "xx\\" // end of line comment
            ]
        }"#,
        ],
    ),
    (
        "returns object when users correctly specify library",
        &[
            r#"{
            "compilerOptions": {
                "lib": ["es5"]
            }
        }"#,
            r#"{
            "compilerOptions": {
                "lib": ["es5", "es6"]
            }
        }"#,
        ],
    ),
];

/// `parseJsonConfigFileTests` (tsconfigparsing_test.go:165-781, with the configs of 783-816).
pub(super) const PARSE_JSON_CONFIG: &[ParseJsonConfigCase] = &[
    ParseJsonConfigCase {
        title: "ignore dotted files and folders",
        include_compiler_options: false,
        input: &[TestConfig {
            json_text: "{}",
            config_file_name: "tsconfig.json",
            base_path: "/apath",
            all_file_list: &[
                ("/apath/test.ts", ""),
                ("/apath/.git/a.ts", ""),
                ("/apath/.b.ts", ""),
                ("/apath/..c.ts", ""),
            ],
        }],
    },
    ParseJsonConfigCase {
        title: "allow dotted files and folders when explicitly requested",
        include_compiler_options: false,
        input: &[TestConfig {
            json_text: r#"{
                    "files": ["/apath/.git/a.ts", "/apath/.b.ts", "/apath/..c.ts"]
                }"#,
            config_file_name: "tsconfig.json",
            base_path: "/apath",
            all_file_list: &[
                ("/apath/test.ts", ""),
                ("/apath/.git/a.ts", ""),
                ("/apath/.b.ts", ""),
                ("/apath/..c.ts", ""),
            ],
        }],
    },
    ParseJsonConfigCase {
        title: "implicitly exclude common package folders",
        include_compiler_options: false,
        input: &[TestConfig {
            json_text: "{}",
            config_file_name: "tsconfig.json",
            base_path: "/",
            all_file_list: &[
                ("/node_modules/a.ts", ""),
                ("/bower_components/b.ts", ""),
                ("/jspm_packages/c.ts", ""),
                ("/d.ts", ""),
                ("/folder/e.ts", ""),
            ],
        }],
    },
    ParseJsonConfigCase {
        title: "generates errors for empty files list",
        include_compiler_options: false,
        input: &[TestConfig {
            json_text: r#"{
                "files": []
            }"#,
            config_file_name: "/apath/tsconfig.json",
            base_path: "/apath",
            all_file_list: &[("/apath/a.ts", "")],
        }],
    },
    ParseJsonConfigCase {
        title: "generates errors for empty files list when no references are provided",
        include_compiler_options: false,
        input: &[TestConfig {
            json_text: r#"{
                "files": [],
                "references": []
            }"#,
            config_file_name: "/apath/tsconfig.json",
            base_path: "/apath",
            all_file_list: &[("/apath/a.ts", "")],
        }],
    },
    ParseJsonConfigCase {
        title: "generates errors for directory with no .ts files",
        include_compiler_options: false,
        input: &[TestConfig {
            json_text: r#"{
            }"#,
            config_file_name: "/apath/tsconfig.json",
            base_path: "/apath",
            all_file_list: &[("/apath/a.js", "")],
        }],
    },
    ParseJsonConfigCase {
        title: "generates errors for empty include",
        include_compiler_options: false,
        input: &[TestConfig {
            json_text: r#"{
                "include": []
            }"#,
            config_file_name: "/apath/tsconfig.json",
            base_path: "tests/cases/unittests",
            all_file_list: &[("/apath/a.ts", "")],
        }],
    },
    ParseJsonConfigCase {
        title: "generates errors for include with parent directory after recursive wildcard",
        include_compiler_options: false,
        input: &[TestConfig {
            json_text: r#"{
                "include": ["**/../*.ts"]
            }"#,
            config_file_name: "/apath/tsconfig.json",
            base_path: "/apath",
            all_file_list: &[("/apath/main.ts", "")],
        }],
    },
    ParseJsonConfigCase {
        title: "parses tsconfig with compilerOptions, files, include, and exclude",
        include_compiler_options: true,
        input: &[TestConfig {
            json_text: r#"{
  "compilerOptions": {
    "outDir": "./dist",
    "strict": true,
    "noImplicitAny": true,
    "target": "ES2017",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "moduleDetection": "auto",
    "jsx": "react",
	"maxNodeModuleJsDepth": 1,
	"paths": {
      "jquery": ["./vendor/jquery/dist/jquery"]
    }
  },
  "files": ["/apath/src/index.ts", "/apath/src/app.ts"],
  "include": ["/apath/src/**/*"],
  "exclude": ["/apath/node_modules", "/apath/dist"]
}"#,
            config_file_name: "/apath/tsconfig.json",
            base_path: "/apath",
            all_file_list: &[
                ("/apath/src/index.ts", ""),
                ("/apath/src/app.ts", ""),
                ("/apath/node_modules/module.ts", ""),
                ("/apath/dist/output.js", ""),
            ],
        }],
    },
    ParseJsonConfigCase {
        title: "generates errors when commandline option is in tsconfig",
        include_compiler_options: false,
        input: &[TestConfig {
            json_text: r#"{
  "compilerOptions": {
    "help": true
  }
}"#,
            config_file_name: "/apath/tsconfig.json",
            base_path: "/apath",
            all_file_list: &[("/apath/a.ts", "")],
        }],
    },
    ParseJsonConfigCase {
        title:
            "does not generate errors for empty files list when one or more references are provided",
        include_compiler_options: false,
        input: &[TestConfig {
            json_text: r#"{
                "files": [],
                "references": [{ "path": "/apath" }]
            }"#,
            config_file_name: "/apath/tsconfig.json",
            base_path: "/apath",
            all_file_list: &[("/apath/a.ts", "")],
        }],
    },
    ParseJsonConfigCase {
        title: "exclude outDir unless overridden",
        include_compiler_options: false,
        input: &[
            TestConfig {
                json_text: r#"{
                "compilerOptions": {
                    "outDir": "bin"
                }
            }"#,
                config_file_name: "tsconfig.json",
                base_path: "/",
                all_file_list: &[("/bin/a.ts", ""), ("/b.ts", "")],
            },
            TestConfig {
                json_text: r#"{
                "compilerOptions": {
                    "outDir": "bin"
                },
                "exclude": [ "obj" ]
            }"#,
                config_file_name: "tsconfig.json",
                base_path: "/",
                all_file_list: &[("/bin/a.ts", ""), ("/b.ts", "")],
            },
        ],
    },
    ParseJsonConfigCase {
        title: "exclude declarationDir unless overridden",
        include_compiler_options: false,
        input: &[
            TestConfig {
                json_text: r#"{
                "compilerOptions": {
                    "declarationDir": "declarations"
                }
            }"#,
                config_file_name: "tsconfig.json",
                base_path: "/",
                all_file_list: &[("/declarations/a.d.ts", ""), ("/a.ts", "")],
            },
            TestConfig {
                json_text: r#"{
                "compilerOptions": {
                    "declarationDir": "declarations"
                },
                "exclude": [ "types" ]
            }"#,
                config_file_name: "tsconfig.json",
                base_path: "/",
                all_file_list: &[("/declarations/a.d.ts", ""), ("/a.ts", "")],
            },
        ],
    },
    ParseJsonConfigCase {
        title: "generates errors for empty directory",
        include_compiler_options: false,
        input: &[TestConfig {
            json_text: r#"{
                "compilerOptions": {
                    "allowJs": true
                }
            }"#,
            config_file_name: "/apath/tsconfig.json",
            base_path: "/apath",
            all_file_list: &[],
        }],
    },
    ParseJsonConfigCase {
        title: "generates errors for includes with outDir",
        include_compiler_options: false,
        input: &[TestConfig {
            json_text: r#"{
                "compilerOptions": {
                    "outDir": "./"
                },
                "include": ["**/*"]
            }"#,
            config_file_name: "/apath/tsconfig.json",
            base_path: "/apath",
            all_file_list: &[("/apath/a.ts", "")],
        }],
    },
    ParseJsonConfigCase {
        title: "generates errors when include is not string",
        include_compiler_options: false,
        input: &[TestConfig {
            json_text: r#"{
  "include": [
    [
      "./**/*.ts"
    ]
  ]
}"#,
            config_file_name: "/apath/tsconfig.json",
            base_path: "/apath",
            all_file_list: &[("/apath/a.ts", "")],
        }],
    },
    ParseJsonConfigCase {
        title: "generates errors when files is not string",
        include_compiler_options: false,
        input: &[TestConfig {
            json_text: r#"{
  "files": [
    {
      "compilerOptions": {
        "experimentalDecorators": true,
        "allowJs": true
      }
    }
  ]
}"#,
            config_file_name: "/apath/tsconfig.json",
            base_path: "/apath",
            all_file_list: &[("/apath/a.ts", "")],
        }],
    },
    ParseJsonConfigCase {
        title: "with outDir from base tsconfig",
        include_compiler_options: false,
        input: &[
            TestConfig {
                json_text: r#"{
  "extends": "./tsconfigWithoutConfigDir.json"
}"#,
                config_file_name: "tsconfig.json",
                base_path: "/",
                all_file_list: &[
                    (
                        "/tsconfigWithoutConfigDir.json",
                        r#"{
  "compilerOptions": {
    "outDir": "bin"
  }
}"#,
                    ),
                    ("/bin/a.ts", ""),
                    ("/b.ts", ""),
                ],
            },
            TestConfig {
                json_text: r#"{
  "extends": "./tsconfigWithConfigDir.json"
}"#,
                config_file_name: "tsconfig.json",
                base_path: "/",
                all_file_list: &[
                    (
                        "/tsconfigWithConfigDir.json",
                        r#"{
  "compilerOptions": {
    "outDir": "${configDir}/bin"
  }
}"#,
                    ),
                    ("/bin/a.ts", ""),
                    ("/b.ts", ""),
                ],
            },
        ],
    },
    ParseJsonConfigCase {
        title: "returns error when tsconfig have excludes",
        include_compiler_options: false,
        input: &[TestConfig {
            json_text: r#"{
                    "compilerOptions": {
                        "lib": ["es5"]
                    },
                    "excludes": [
                        "foge.ts"
                    ]
                }"#,
            config_file_name: "tsconfig.json",
            base_path: "/apath",
            all_file_list: &[("/apath/test.ts", ""), ("/apath/foge.ts", "")],
        }],
    },
    ParseJsonConfigCase {
        title: "parses tsconfig with extends, files, include and other options",
        include_compiler_options: true,
        input: &[TestConfig {
            json_text: "{\n\t\t\t\t\"extends\": \"./tsconfigWithExtends.json\",\n\t\t\t\t\"compilerOptions\": {\n\t\t\t\t    \"outDir\": \"./dist\",\n    \t\t\t\t\"strict\": true,\n    \t\t\t\t\"noImplicitAny\": true,\n\t\t\t\t\t\"baseUrl\": \"\",\n\t\t\t\t},\n\t\t\t}",
            config_file_name: "tsconfig.json",
            base_path: "/",
            all_file_list: &[
                (
                    "/tsconfigWithExtends.json",
                    r#"{
  "files": ["/src/index.ts", "/src/app.ts"],
  "include": ["/src/**/*"],
  "exclude": [],
  "ts-node": {
    "compilerOptions": {
      "module": "commonjs"
    },
    "transpileOnly": true
  }
}"#,
                ),
                ("/src/index.ts", ""),
                ("/src/app.ts", ""),
                ("/node_modules/module.ts", ""),
                ("/dist/output.js", ""),
            ],
        }],
    },
    ParseJsonConfigCase {
        title: "parses tsconfig with extends and configDir",
        include_compiler_options: true,
        input: &[TestConfig {
            json_text: r#"{
				"extends": "./tsconfig.base.json"
			}"#,
            config_file_name: "tsconfig.json",
            base_path: "/",
            all_file_list: &[
                (
                    "/tsconfig.base.json",
                    r#"{
  "compilerOptions": {
    "outFile": "${configDir}/outFile",
    "outDir": "${configDir}/outDir",
    "rootDir": "${configDir}/rootDir",
    "tsBuildInfoFile": "${configDir}/tsBuildInfoFile",
    "baseUrl": "${configDir}/baseUrl",
    "declarationDir": "${configDir}/declarationDir",
  }
}"#,
                ),
                ("/src/index.ts", ""),
                ("/src/app.ts", ""),
                ("/node_modules/module.ts", ""),
                ("/dist/output.js", ""),
            ],
        }],
    },
    ParseJsonConfigCase {
        title: "reports error for an unknown option",
        include_compiler_options: false,
        input: &[TestConfig {
            json_text: r#"{
			    "compilerOptions": {
				"unknown": true
			    }
			}"#,
            config_file_name: "tsconfig.json",
            base_path: "/",
            all_file_list: &[("/app.ts", "")],
        }],
    },
    ParseJsonConfigCase {
        title: "reports spelling suggestion for an unknown option",
        include_compiler_options: false,
        input: &[TestConfig {
            json_text: r#"{
			    "compilerOptions": {
				"targt": 1
			    }
			}"#,
            config_file_name: "tsconfig.json",
            base_path: "/",
            all_file_list: &[("/app.ts", "")],
        }],
    },
    ParseJsonConfigCase {
        title: "reports errors for wrong type option and invalid enum value",
        include_compiler_options: false,
        input: &[TestConfig {
            json_text: r#"{
			    "compilerOptions": {
				"target": "invalid value",
				"removeComments": "should be a boolean",
				"moduleResolution": "invalid value"
			    }
			}"#,
            config_file_name: "tsconfig.json",
            base_path: "/",
            all_file_list: &[("/app.ts", "")],
        }],
    },
    ParseJsonConfigCase {
        title: "reports errors for incorrectly cased option names",
        include_compiler_options: true,
        input: &[TestConfig {
            json_text: r#"{
			    "compilerOptions": {
				"sourcemap": true,
				"declarationmap": true,
				"nouncheckedindexedaccess": true,
				"exactoptionalpropertytypes": true,
				"verbatimmodulesyntax": true,
				"isolatedmodules": true,
				"nouncheckedsideeffectimports": true,
				"moduledetection": "force",
				"skiplibcheck": true,
				"checkjs": true
			    }
			}"#,
            config_file_name: "tsconfig.json",
            base_path: "/",
            all_file_list: &[("/app.ts", "")],
        }],
    },
    ParseJsonConfigCase {
        title: "handles empty types array",
        include_compiler_options: true,
        input: &[TestConfig {
            json_text: r#"{
			    "compilerOptions": {
					"types": []
				}
			}"#,
            config_file_name: "tsconfig.json",
            base_path: "/",
            all_file_list: &[("/app.ts", "")],
        }],
    },
    ParseJsonConfigCase {
        title: "issue 1267 scenario - extended files not picked up",
        include_compiler_options: true,
        input: &[TestConfig {
            json_text: r#"{
  "extends": "./tsconfig-base/backend.json",
  "compilerOptions": {
    "baseUrl": "./",
    "outDir": "dist",
    "rootDir": "src",
    "resolveJsonModule": true
  },
  "exclude": ["node_modules", "dist"],
  "include": ["src/**/*"]
}"#,
            config_file_name: "tsconfig.json",
            base_path: "/",
            all_file_list: &[
                (
                    "/tsconfig-base/backend.json",
                    r#"{
  "$schema": "https://json.schemastore.org/tsconfig",
  "display": "Backend",
  "compilerOptions": {
    "allowJs": true,
    "module": "nodenext",
    "removeComments": true,
    "emitDecoratorMetadata": true,
    "experimentalDecorators": true,
    "allowSyntheticDefaultImports": true,
    "target": "esnext",
    "lib": ["ESNext"],
    "incremental": false,
    "esModuleInterop": true,
    "noImplicitAny": true,
    "moduleResolution": "nodenext",
    "types": ["node", "vitest/globals"],
    "sourceMap": true,
    "strictPropertyInitialization": false
  },
  "files": [
    "types/ical2json.d.ts",
    "types/express.d.ts",
    "types/multer.d.ts",
    "types/reset.d.ts",
    "types/stripe-custom-typings.d.ts",
    "types/nestjs-modules.d.ts",
    "types/luxon.d.ts",
    "types/nestjs-pino.d.ts"
  ],
  "ts-node": {
    "files": true
  }
}"#,
                ),
                ("/tsconfig-base/types/ical2json.d.ts", "export {}"),
                ("/tsconfig-base/types/express.d.ts", "export {}"),
                ("/tsconfig-base/types/multer.d.ts", "export {}"),
                ("/tsconfig-base/types/reset.d.ts", "export {}"),
                (
                    "/tsconfig-base/types/stripe-custom-typings.d.ts",
                    "export {}",
                ),
                ("/tsconfig-base/types/nestjs-modules.d.ts", "export {}"),
                (
                    "/tsconfig-base/types/luxon.d.ts",
                    r#"declare module 'luxon' {
  interface TSSettings {
    throwOnInvalid: true
  }
}
export {}"#,
                ),
                ("/tsconfig-base/types/nestjs-pino.d.ts", "export {}"),
                ("/src/main.ts", "export {}"),
                ("/src/utils.ts", "export {}"),
            ],
        }],
    },
    ParseJsonConfigCase {
        title: "null overrides in extended tsconfig - array fields",
        include_compiler_options: true,
        input: &[TestConfig {
            json_text: r#"{
  "extends": "./tsconfig-base.json",
  "compilerOptions": {
    "types": null,
    "lib": null,
    "typeRoots": null
  }
}"#,
            config_file_name: "tsconfig.json",
            base_path: "/",
            all_file_list: &[
                (
                    "/tsconfig-base.json",
                    r#"{
  "compilerOptions": {
    "types": ["node", "@types/jest"],
    "lib": ["es2020", "dom"],
    "typeRoots": ["./types", "./node_modules/@types"]
  }
}"#,
                ),
                ("/app.ts", ""),
            ],
        }],
    },
    ParseJsonConfigCase {
        title: "null overrides in extended tsconfig - string fields",
        include_compiler_options: true,
        input: &[TestConfig {
            json_text: r#"{
  "extends": "./tsconfig-base.json",
  "compilerOptions": {
    "outDir": null,
    "baseUrl": null,
    "rootDir": null
  }
}"#,
            config_file_name: "tsconfig.json",
            base_path: "/",
            all_file_list: &[
                (
                    "/tsconfig-base.json",
                    r#"{
  "compilerOptions": {
    "outDir": "./dist",
    "baseUrl": "./src",
    "rootDir": "./src"
  }
}"#,
                ),
                ("/app.ts", ""),
            ],
        }],
    },
    ParseJsonConfigCase {
        title: "null overrides in extended tsconfig - mixed field types",
        include_compiler_options: true,
        input: &[TestConfig {
            json_text: r#"{
  "extends": "./tsconfig-base.json",
  "compilerOptions": {
    "types": null,
    "outDir": null,
    "strict": false,
    "lib": ["es2022"],
    "allowJs": null
  }
}"#,
            config_file_name: "tsconfig.json",
            base_path: "/",
            all_file_list: &[
                (
                    "/tsconfig-base.json",
                    r#"{
  "compilerOptions": {
    "types": ["node"],
    "lib": ["es2020", "dom"],
    "outDir": "./dist",
    "strict": true,
    "allowJs": true,
    "target": "es2020"
  }
}"#,
                ),
                ("/app.ts", ""),
            ],
        }],
    },
    ParseJsonConfigCase {
        title: "null overrides with multiple extends levels",
        include_compiler_options: true,
        input: &[TestConfig {
            json_text: r#"{
  "extends": "./tsconfig-middle.json",
  "compilerOptions": {
    "types": null,
    "lib": null
  }
}"#,
            config_file_name: "tsconfig.json",
            base_path: "/",
            all_file_list: &[
                (
                    "/tsconfig-middle.json",
                    r#"{
  "extends": "./tsconfig-base.json",
  "compilerOptions": {
    "types": ["jest"],
    "outDir": "./build"
  }
}"#,
                ),
                (
                    "/tsconfig-base.json",
                    r#"{
  "compilerOptions": {
    "types": ["node"],
    "lib": ["es2020"],
    "outDir": "./dist",
    "strict": true
  }
}"#,
                ),
                ("/app.ts", ""),
            ],
        }],
    },
    ParseJsonConfigCase {
        title: "null overrides in middle level of extends chain",
        include_compiler_options: true,
        input: &[TestConfig {
            json_text: r#"{
  "extends": "./tsconfig-middle.json",
  "compilerOptions": {
    "outDir": "./final"
  }
}"#,
            config_file_name: "tsconfig.json",
            base_path: "/",
            all_file_list: &[
                (
                    "/tsconfig-middle.json",
                    r#"{
  "extends": "./tsconfig-base.json",
  "compilerOptions": {
    "types": null,
    "lib": null,
    "outDir": "./middle"
  }
}"#,
                ),
                (
                    "/tsconfig-base.json",
                    r#"{
  "compilerOptions": {
    "types": ["node"],
    "lib": ["es2020"],
    "outDir": "./base",
    "strict": true
  }
}"#,
                ),
                ("/app.ts", ""),
            ],
        }],
    },
];

/// `TestParseTypeAcquisition`'s cases (tsconfigparsing_test.go:1558-1626): title,
/// config file name and config text.
pub(super) const TYPE_ACQUISITION: &[(&str, &str, &str)] = &[
    (
        "Convert correctly format tsconfig.json to typeAcquisition ",
        "tsconfig.json",
        r#"{
	"typeAcquisition": {
		"enable": true,
		"include": ["0.d.ts", "1.d.ts"],
		"exclude": ["0.js", "1.js"],
	},
}"#,
    ),
    (
        "Convert incorrect format tsconfig.json to typeAcquisition ",
        "tsconfig.json",
        r#"{
	"typeAcquisition": {
		"enableAutoDiscovy": true,
	}
}"#,
    ),
    (
        "Convert default tsconfig.json to typeAcquisition ",
        "tsconfig.json",
        "{}",
    ),
    (
        "Convert tsconfig.json with only enable property to typeAcquisition ",
        "tsconfig.json",
        r#"{
	"typeAcquisition": {
		"enable": true,
	},
}"#,
    ),
    (
        "Convert jsconfig.json to typeAcquisition ",
        "jsconfig.json",
        r#"{
	"typeAcquisition": {
		"enable": false,
		"include": ["0.d.ts"],
		"exclude": ["0.js"],
	},
}"#,
    ),
    (
        "Convert default jsconfig.json to typeAcquisition ",
        "jsconfig.json",
        "{}",
    ),
    (
        "Convert incorrect format jsconfig.json to typeAcquisition ",
        "jsconfig.json",
        r#"{
	"typeAcquisition": {
		"enableAutoDiscovy": true,
	},
}"#,
    ),
    (
        "Convert jsconfig.json with only enable property to typeAcquisition ",
        "jsconfig.json",
        r#"{
	"typeAcquisition": {
		"enable": false,
	},
}"#,
    ),
];
