mod notify;

fn main() {
    clawcode::core::init_tracing();
    if let Err(error) = clawcode::cli::run() {
        tracing::error!(%error, "terminal ui failed");
        std::process::exit(1);
    }
}
