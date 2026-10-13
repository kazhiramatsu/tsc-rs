//! Go's `flag` package as tsgo's `--lsp` and `--api` command lines use it;
//! the expected texts are tsgo's (`tsgo --lsp ...` at aa8149273).

use tsc_host::flag::{self, Flag, Kind, Value};

const FLAGS: &[Flag] = &[
    Flag {
        name: "stdio",
        kind: Kind::Bool,
        usage: "use stdio for communication",
    },
    Flag {
        name: "pprofDir",
        kind: Kind::String,
        usage: "Generate pprof CPU/memory profiles to the given directory.",
    },
    Flag {
        name: "pipe",
        kind: Kind::String,
        usage: "use named pipe for communication",
    },
    Flag {
        name: "socket",
        kind: Kind::String,
        usage: "use socket for communication",
    },
    Flag {
        name: "clientProcessId",
        kind: Kind::Int,
        usage: "use the given PID for the parent process watchdog",
    },
];

fn parse(arguments: &[&str]) -> Result<Vec<(&'static str, Value)>, Option<String>> {
    let arguments = arguments
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect::<Vec<_>>();
    flag::parse(&arguments, FLAGS)
}

fn int(text: &str) -> Result<i64, String> {
    match parse(&[&format!("--clientProcessId={text}")]) {
        Ok(values) => match values.as_slice() {
            [(_, Value::Int(value))] => Ok(*value),
            other => panic!("unexpected values {other:?}"),
        },
        Err(message) => Err(message.unwrap_or_default()),
    }
}

#[test]
fn usage_is_tsgos() {
    assert_eq!(
        flag::usage("lsp", FLAGS, |_| None),
        "Usage of lsp:
  -clientProcessId int
    \tuse the given PID for the parent process watchdog
  -pipe string
    \tuse named pipe for communication
  -pprofDir string
    \tGenerate pprof CPU/memory profiles to the given directory.
  -socket string
    \tuse socket for communication
  -stdio
    \tuse stdio for communication
"
    );
}

#[test]
fn values_in_order() {
    assert_eq!(
        parse(&[
            "--stdio",
            "-pipe",
            "x",
            "--clientProcessId=12",
            "--stdio=false"
        ]),
        Ok(vec![
            ("stdio", Value::Bool(true)),
            ("pipe", Value::String("x".to_owned())),
            ("clientProcessId", Value::Int(12)),
            ("stdio", Value::Bool(false)),
        ])
    );
}

#[test]
fn parsing_stops_at_the_first_argument_that_is_not_a_flag() {
    assert_eq!(
        parse(&["--stdio", "file.ts", "--pipe"]),
        Ok(vec![("stdio", Value::Bool(true))])
    );
    assert_eq!(parse(&["--", "--stdio"]), Ok(vec![]));
    assert_eq!(parse(&["-", "--stdio"]), Ok(vec![]));
}

#[test]
fn errors_are_gos() {
    assert_eq!(parse(&["-h"]), Err(None));
    assert_eq!(parse(&["--help"]), Err(None));
    assert_eq!(
        parse(&["--nope"]),
        Err(Some("flag provided but not defined: -nope".to_owned()))
    );
    assert_eq!(
        parse(&["---stdio"]),
        Err(Some("bad flag syntax: ---stdio".to_owned()))
    );
    assert_eq!(
        parse(&["-=x"]),
        Err(Some("bad flag syntax: -=x".to_owned()))
    );
    assert_eq!(
        parse(&["--pipe"]),
        Err(Some("flag needs an argument: -pipe".to_owned()))
    );
    assert_eq!(
        parse(&["--stdio=maybe"]),
        Err(Some(
            "invalid boolean value \"maybe\" for -stdio: parse error".to_owned()
        ))
    );
    assert_eq!(
        int("x"),
        Err("invalid value \"x\" for flag -clientProcessId: parse error".to_owned())
    );
    assert_eq!(
        int("9223372036854775808"),
        Err(
            "invalid value \"9223372036854775808\" for flag -clientProcessId: value out of range"
                .to_owned()
        )
    );
}

/// Go's `strconv.ParseInt(text, 0, 64)`.
#[test]
fn ints_take_gos_prefixes_and_underscores() {
    assert_eq!(int("42"), Ok(42));
    assert_eq!(int("-42"), Ok(-42));
    assert_eq!(int("+7"), Ok(7));
    assert_eq!(int("0x1F"), Ok(31));
    assert_eq!(int("0o17"), Ok(15));
    assert_eq!(int("017"), Ok(15));
    assert_eq!(int("0b101"), Ok(5));
    assert_eq!(int("1_000"), Ok(1000));
    assert_eq!(int("0x_1F"), Ok(31));
    assert_eq!(int("-9223372036854775808"), Ok(i64::MIN));
    assert!(int("08").is_err());
    assert!(int("0x").is_err());
    assert!(int("1__0").is_err());
    assert!(int("_1").is_err());
    assert!(int("1_").is_err());
    assert!(int("").is_err());
}
