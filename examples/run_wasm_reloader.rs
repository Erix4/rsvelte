use std::{fs::File, io::Write, process::Command};

use rsvelte::{compile, setup_dir};

fn main() {
    let output_path = "output";
    env_logger::init();
    let compile_out = compile("./test-prj1/src/+page.rsvelte").expect("Compilation failed");

    // Setup output directory
    //setup_dir(output_path).expect("Failed to setup output directory");

    // Write generated files
    let mut state_rs_file = File::create(format!("{}/src/state.rs", output_path)).unwrap();
    state_rs_file
        .write_all(compile_out.state_rs.as_bytes())
        .unwrap();

    Command::new("wasm-reloader")
        .current_dir(format!("./{}", output_path))
        .status()
        .expect("Failed to execute cargo build");
}
