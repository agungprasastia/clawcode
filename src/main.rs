fn main() {
    clawcode::core::init_tracing();
    if let Err(error) = clawcode::cli::run() {
        eprintln!("ERROR\n  {error}");
        std::process::exit(1);
    }
}
