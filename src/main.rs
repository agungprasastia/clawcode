mod adapters;
mod cli;
mod notify;
mod persistence;
mod provider;
mod workspace;

fn main() {
    clawcode::core::init_tracing();
    if let Err(error) = cli::run() {
        tracing::error!(%error, "terminal ui failed");
        std::process::exit(1);
    }
}
