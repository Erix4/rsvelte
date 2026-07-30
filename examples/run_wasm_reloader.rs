use std::{
    fs::File,
    io::Write,
    path::Path,
    process::Command,
    sync::mpsc,
    time::{Duration, Instant},
};

use notify::{RecursiveMode, Watcher};
use rsvelte::compile;

fn run_compile(output_path: &str) {
    println!("Compiling...");
    let compile_out = match compile("./test-prj1/src/+page.rsvelte") {
        Ok(out) => out,
        Err(e) => {
            eprintln!("Compilation failed: {}", e);
            return;
        }
    };

    // Write generated files
    let mut state_rs_file = File::create(format!("{}/src/state.rs", output_path)).unwrap();
    state_rs_file
        .write_all(compile_out.state_rs.as_bytes())
        .unwrap();
    let mut css_file = File::create(format!("{}/style.css", output_path)).unwrap();
    css_file.write_all(compile_out.css.as_bytes()).unwrap();
    println!("Compilation complete.");
}

fn main() {
    let output_path = "output";
    env_logger::init();

    // Initial compile and launch
    run_compile(output_path);

    // The wasm-reloader tool watches for changes and reloads the browser automatically, so we just need to launch it once here
    Command::new("wasm-reloader")
        .current_dir(format!("./{}", output_path))
        .spawn()
        .expect("Failed to spawn wasm-reloader");

    // Watch the test-prj1 folder for changes
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    })
    .expect("Failed to create file watcher");

    watcher
        .watch(Path::new("./test-prj1"), RecursiveMode::Recursive)
        .expect("Failed to watch test-prj1 directory");

    println!("Watching test-prj1/ for changes...");

    // Debounce: wait for events to settle before recompiling
    let debounce = Duration::from_millis(300);
    loop {
        // Block until at least one event arrives
        let _ = rx.recv();
        // Drain any additional events within the debounce window
        let deadline = Instant::now() + debounce;
        while let Ok(_) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {}

        println!("\nFile change detected, recompiling...");
        run_compile(output_path);
    }
}
