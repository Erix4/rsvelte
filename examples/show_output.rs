use std::{
    fs::File,
    io::Write,
    process::{Command, Stdio},
};

use log::{Level, log_enabled};
use rsvelte::{compile, setup_dir, setup_dir_force};

fn main() {
    let output_path = "output";
    env_logger::init();
    let compile_out =
        compile("./test-prj1/src/+page.rsvelte").expect("Compilation failed");

    // Setup output directory
    setup_dir(output_path).expect("Failed to setup output directory");

    // Write generated files
    let mut state_rs_file =
        File::create(format!("{}/src/state.rs", output_path)).unwrap();
    state_rs_file
        .write_all(compile_out.state_rs.as_bytes())
        .unwrap();
    let mut css_file =
        File::create(format!("{}/style.css", output_path)).unwrap();
    css_file.write_all(compile_out.css.as_bytes()).unwrap();

    let fmt_status = Command::new("rustfmt")
        .arg("./src/state.rs")
        .current_dir(format!("./{}", output_path))
        .status()
        .expect("Failed to execute cargo fmt");
    if !fmt_status.success() {
        eprintln!("rustfmt failed");
    }

    let mut binding = Command::new("cargo");
    let compile_status = binding
        .arg("build")
        // ignore unused warnings
        .env("RUSTFLAGS", "-Awarnings")
        .current_dir(format!("./{}", output_path));

    let compile_status = if log_enabled!(Level::Debug) {
        compile_status
    } else {
        compile_status.stdout(Stdio::null())
    }
    .status()
    .expect("Failed to execute cargo build");

    if !compile_status.success() {
        eprintln!("cargo build failed");
    }
}
