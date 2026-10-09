//! tsgo `parseAPIFlags` (cmd/tsc/api.go) with Go's `flag` package, pinned
//! to tsgo's messages at 19dadef8.

use tsc_api::server::{parse_api_flags, ServerOptions};

fn parse(arguments: &[&str]) -> Result<ServerOptions, Option<String>> {
    let arguments = arguments
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect::<Vec<_>>();
    parse_api_flags(&arguments, "/default")
}

#[test]
fn the_flags_take_go_forms() {
    assert_eq!(
        parse(&[]).unwrap(),
        ServerOptions {
            current_directory: "/default".to_owned(),
            ..ServerOptions::default()
        }
    );
    assert_eq!(
        parse(&[
            "--async",
            "-timing=true",
            "--cwd=/work",
            "-pipe",
            "/tmp/api.sock",
            "--callbacks=readFile,fileExists",
            "--runExternalCode",
        ])
        .unwrap(),
        ServerOptions {
            current_directory: "/work".to_owned(),
            pipe_path: Some("/tmp/api.sock".to_owned()),
            callbacks: vec!["readFile".to_owned(), "fileExists".to_owned()],
            async_protocol: true,
            collect_timing: true,
        }
    );
    // Parsing stops at the first argument that is not a flag.
    assert!(parse(&["--async=0", "file.ts", "--nope"]).is_ok_and(|options| !options.async_protocol));
}

#[test]
fn invalid_flags_fail_with_go_messages() {
    for (arguments, expected) in [
        (
            &["--nope"][..],
            Some("flag provided but not defined: -nope"),
        ),
        (&["--cwd"], Some("flag needs an argument: -cwd")),
        (
            &["--async=maybe"],
            Some("invalid boolean value \"maybe\" for -async: parse error"),
        ),
        (&["---x"], Some("bad flag syntax: ---x")),
        (&["-h"], None),
        (&["--help"], None),
    ] {
        assert_eq!(
            parse(arguments).unwrap_err().as_deref(),
            expected,
            "{arguments:?}"
        );
    }
}
