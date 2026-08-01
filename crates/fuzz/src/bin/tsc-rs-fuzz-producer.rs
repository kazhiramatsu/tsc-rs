#![forbid(unsafe_code)]

fn main() {
    if let Err(error) = tsc_fuzz::cli::run(std::env::args_os().skip(1)) {
        eprintln!("tsc-rs-fuzz-producer: {error}");
        std::process::exit(2);
    }
}
