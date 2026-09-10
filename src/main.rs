mod adapters;
mod cli;
mod config;
mod core;
mod notify;
mod persistence;
mod provider;
mod tui;
mod workspace;

fn main() {
    cli::run();
}
