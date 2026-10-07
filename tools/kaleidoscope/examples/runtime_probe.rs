//! Developer-only headless hosting proof; this is not shipped in launcher packages.
use std::path::Path;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        eprintln!("runtime_probe APPLICATION HOSTFXR STARTUP_BRIDGE");
        std::process::exit(2);
    }
    match kaleidoscope::launcher::host::run_managed(
        Path::new(&args[0]),
        Path::new(&args[1]),
        Path::new(&args[2]),
    ) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}
