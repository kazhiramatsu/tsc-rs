#![forbid(unsafe_code)]

fn main() {
    if let Err(error) = tsrs2_fuzz::cli::run(std::env::args_os().skip(1)) {
        eprintln!("tsrs2-fuzz-producer: {error}");
        std::process::exit(2);
    }
}
