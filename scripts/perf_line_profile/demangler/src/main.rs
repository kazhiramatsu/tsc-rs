use std::io::{self, BufRead, Write};

fn main() -> io::Result<()> {
    let mut output = io::BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        writeln!(output, "{:#}", rustc_demangle::demangle(&line?))?;
    }
    output.flush()
}
