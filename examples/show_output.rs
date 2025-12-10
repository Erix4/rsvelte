use std::{fs::File, io::Write, process::{Command, Stdio}};

use rsvelte::{compile, setup_dir};

fn main() {
    let output_path = "output";
    env_logger::init();
    let compile_out = compile("./+page.rsvelte").expect("Compilation failed");

    // Setup output directory
    setup_dir(output_path).expect("Failed to setup output directory");

    // Write generated files
    let mut state_rs_file = File::create(format!("{}/src/state.rs", output_path)).unwrap();
    state_rs_file.write_all(compile_out.state_rs.as_bytes()).unwrap();

    let mut startup_js_file = File::create(format!("{}/startup.js", output_path)).unwrap();
    startup_js_file.write_all(compile_out.startup_js.as_bytes()).unwrap();

    let fmt_status = Command::new("rustfmt")
        .arg("./src/state.rs")
        .current_dir(format!("./{}", output_path))
        .status()
        .expect("Failed to execute cargo fmt");
    if !fmt_status.success() {
        eprintln!("rustfmt failed");
    }

    let compile_status = Command::new("cargo")
        .arg("build")
        .current_dir(format!("./{}", output_path))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("Failed to execute cargo build");
    if !compile_status.success() {
        eprintln!("cargo build failed");
    }
}
