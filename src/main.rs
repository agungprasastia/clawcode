mod adapters;
mod cli;
mod config;
mod notify;
mod persistence;
mod provider;
mod tui;
mod workspace;

fn main() {
    clawcode::core::init_tracing();
    cli::run();
}
