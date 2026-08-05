fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let output = tsc_compiler::run_cli(&arguments);
    print!("{}", output.stdout());
    eprint!("{}", output.stderr());
    std::process::exit(output.exit_code());
}
