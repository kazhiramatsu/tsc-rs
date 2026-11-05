//! M8 micro-batch (observation only, never merged): a symbol-table lookup by a text key (`NameKey` for `&str`:
//! the text is interned on every lookup) against the same lookup by a name interned once (`known_name!`), on the
//! checker's hottest pattern: a module's exports probed for `export=` (absent or present, alternately).
//! Usage: textkey_batch [--rounds N] [--mode text|name|loop] under `/usr/bin/time -l`; per lookup =
//! (instructions(mode) - instructions(loop)) / (rounds * LOOKUPS), modes sharing their setup.
use std::hint::black_box;
use tsc_binder::SymbolTable;
use tsc_types::{EscapedName, InternalSymbolName, SymbolId};

const LOOKUPS: u64 = 1_000_000;

fn main() {
    let mut rounds = 10u64;
    let mut mode = String::from("text");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--rounds" => rounds = args.next().expect("count").parse().expect("count"),
            "--mode" => mode = args.next().expect("mode"),
            other => panic!("unknown argument {other}"),
        }
    }
    // A module's exports: forty names; `export=` absent in one table and present in the other.
    let mut absent = SymbolTable::new();
    for index in 0..40u32 {
        let name: &'static str = Box::leak(format!("exported_name_{index}").into_boxed_str());
        absent.insert(EscapedName::internal(name), SymbolId::new(index + 1));
    }
    let mut present = absent.clone();
    present.insert(EscapedName::internal(InternalSymbolName::EXPORT_EQUALS), SymbolId::new(100));
    let tables = [absent, present];
    let mut found = 0u64;
    match mode.as_str() {
        "text" => {
            for _ in 0..rounds {
                for index in 0..LOOKUPS {
                    let table = black_box(&tables[(index & 1) as usize]);
                    found += u64::from(table.get(black_box(InternalSymbolName::EXPORT_EQUALS)).is_some());
                }
            }
        }
        "name" => {
            for _ in 0..rounds {
                for index in 0..LOOKUPS {
                    let table = black_box(&tables[(index & 1) as usize]);
                    found += u64::from(table.get(black_box(tsc_types::known_name!("export="))).is_some());
                }
            }
        }
        "loop" => {
            for _ in 0..rounds {
                for index in 0..LOOKUPS {
                    let table = black_box(&tables[(index & 1) as usize]);
                    found += u64::from(black_box(table).is_empty());
                }
            }
        }
        other => panic!("unknown mode {other}"),
    }
    println!("mode={mode} rounds={rounds} lookups={} found={found}", rounds * LOOKUPS);
}
