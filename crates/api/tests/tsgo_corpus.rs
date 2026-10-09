//! Compares the encoder with tsgo's over a corpus (run by hand):
//!
//! ```text
//! scripts/api_encoder_dump.py corpus <dir>
//! TSRS_API_CORPUS=<dir>/corpus.tsv TSRS_API_TSGO=<dir> TSRS_API_REPORT=<file> \
//!     cargo test -p tsc-rs-api --test tsgo_corpus -- --ignored --nocapture
//! ```
//!
//! Each list line is `<index>\t<file name>\t<path on disk>`; `<dir>/<index>.bin`
//! is tsgo's `encoder.EncodeSourceFile` of the file parsed under that name
//! (with its content hash set, as tsgo's API session sets it). The test
//! encodes the same file and compares the bytes; for a difference it
//! reports the first differing section and, in the nodes, the first
//! differing record of both sides.

mod common;

use std::collections::BTreeMap;
use std::fmt::Write as _;

use common::{difference, dump, encode, read_u32, sections};

#[test]
#[ignore = "needs tsgo's encodings of a corpus (see the module documentation)"]
fn tsgo_corpus() {
    let list = std::env::var("TSRS_API_CORPUS").expect("TSRS_API_CORPUS");
    let tsgo = std::env::var("TSRS_API_TSGO").expect("TSRS_API_TSGO");
    let report_path = std::env::var("TSRS_API_REPORT").expect("TSRS_API_REPORT");
    let mut same = 0;
    let mut categories: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let list = std::fs::read_to_string(list).unwrap();
    for line in list.lines() {
        let mut fields = line.split('\t');
        let (index, name, disk) = (
            fields.next().unwrap(),
            fields.next().unwrap(),
            fields.next().unwrap(),
        );
        let Ok(text) = String::from_utf8(std::fs::read(disk).unwrap()) else {
            // tsc-rs's files are text; a UTF-16 or binary case file is
            // decoded by the harness before parsing.
            categories
                .entry("skipped: not UTF-8".to_owned())
                .or_default()
                .push((name.to_owned(), String::new()));
            continue;
        };
        let theirs = std::fs::read(format!("{tsgo}/{index}.bin")).unwrap();
        let ours = std::panic::catch_unwind(|| encode(name, &text));
        if let (Ok(ours), Ok(filter)) = (&ours, std::env::var("TSRS_API_DUMP")) {
            if name.contains(&filter) {
                std::fs::write(format!("{report_path}.{index}.tsrs.txt"), dump(ours)).unwrap();
                std::fs::write(format!("{report_path}.{index}.tsgo.txt"), dump(&theirs)).unwrap();
            }
        }
        match ours {
            Ok(ours) if ours == theirs => same += 1,
            Ok(ours) => {
                let (category, detail) = difference(&ours, &theirs);
                categories
                    .entry(category)
                    .or_default()
                    .push((name.to_owned(), detail));
            }
            Err(panic) => {
                let message = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                categories
                    .entry(format!("panic {message}"))
                    .or_default()
                    .push((name.to_owned(), String::new()));
            }
        }
    }
    let total: usize = list.lines().count();
    let mut report = String::new();
    writeln!(report, "identical {same} / {total}").unwrap();
    let mut ordered: Vec<_> = categories.iter().collect();
    ordered.sort_by_key(|(_, files)| std::cmp::Reverse(files.len()));
    for (category, files) in &ordered {
        writeln!(report, "{:6} {category}", files.len()).unwrap();
    }
    for (category, files) in &ordered {
        writeln!(report, "\n== {category} ({})", files.len()).unwrap();
        for (name, detail) in files.iter().take(3) {
            writeln!(report, "  {name}\n    {detail}").unwrap();
        }
    }
    std::fs::write(&report_path, &report).unwrap();
    println!("{}", report.lines().take(40).collect::<Vec<_>>().join("\n"));
}

/// Prints the strings and extended data that differ for the files named
/// in TSRS_API_STRINGS (`index,index,...`).
#[test]
#[ignore = "needs tsgo's encodings of a corpus (see the module documentation)"]
fn tsgo_corpus_strings() {
    let list = std::fs::read_to_string(std::env::var("TSRS_API_CORPUS").unwrap()).unwrap();
    let tsgo = std::env::var("TSRS_API_TSGO").unwrap();
    let wanted: Vec<String> = std::env::var("TSRS_API_STRINGS")
        .unwrap()
        .split(',')
        .map(str::to_owned)
        .collect();
    for line in list.lines() {
        let mut fields = line.split('\t');
        let (index, name, disk) = (
            fields.next().unwrap(),
            fields.next().unwrap(),
            fields.next().unwrap(),
        );
        if !wanted.iter().any(|w| w == index) {
            continue;
        }
        let text = std::fs::read_to_string(disk).unwrap();
        let theirs = std::fs::read(format!("{tsgo}/{index}.bin")).unwrap();
        let ours = encode(name, &text);
        let strings = |buffer: &[u8]| {
            let s = sections(buffer);
            (0..s.string_offsets.len() / 8)
                .map(|i| {
                    let start = read_u32(s.string_offsets, i * 8) as usize;
                    let end = read_u32(s.string_offsets, i * 8 + 4) as usize;
                    (
                        start,
                        end,
                        String::from_utf8_lossy(&s.string_data[start..end]).into_owned(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let (a, b) = (strings(&ours), strings(&theirs));
        println!("== {name}");
        for (i, (x, y)) in a.iter().zip(&b).enumerate() {
            if x != y {
                println!("  string {i}: tsc-rs {x:?}\n            tsgo   {y:?}");
            }
        }
        let (x, y) = (sections(&ours).extended, sections(&theirs).extended);
        for i in 0..x.len().min(y.len()) / 4 {
            if read_u32(x, i * 4) != read_u32(y, i * 4) {
                println!(
                    "  extended word {i}: tsc-rs {:#x} tsgo {:#x}",
                    read_u32(x, i * 4),
                    read_u32(y, i * 4)
                );
            }
        }
    }
}
