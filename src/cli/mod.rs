pub fn run() {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--version")) {
        println!("clawcode {}", env!("CARGO_PKG_VERSION"));
    }
}
