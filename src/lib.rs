//! RSvelte - A Rust-based Svelte-like compiler for WebAssembly applications
//!
//! This library provides functionality to compile `.rsvelte` files into
//! Rust and JavaScript code that can be run in the browser using WebAssembly.
//!
//! While the syntax is based on Svelte, the compiler is completely custom and
//! does not support all Svelte features. It also requires extra syntax in some
//! places to work with Rust's type system and ownership model.
//!
//! # Example syntax
//! ```rsvelte
//! <script>
//!  let counter = $state(0);
//!
//!  fn increment($counter) {
//!     *counter += 1;
//!  }
//! </script>
//! <div id="counter-app">
//!     <h1>Counter: {counter}</h1>
//!    <button onclick={increment}>Increment</button>
//! </div>
//! ```

mod parse;
mod transform;
mod code_gen;
mod utils;

use colored::Colorize;
use log::{error, info};
use tempdir::TempDir;
use utils::*;

use crate::parse::get_all_components;

/// List of supported events: (attribute name, web_sys type, JS event type)
///
/// Used for event handler generation
///
/// # Example syntax
/// ```rsvelte
/// <button onclick={handle_click}>Click me</button>
/// ```
/// Here, the `onclick` attribute name maps to `web_sys::MouseEvent` and the JS event type is `click`
pub static EVENTS: &[(&str, &str, &str)] = &[
    ("onclick", "web_sys::MouseEvent", "click"), // Svelte, web_sys type, JS
    ("onmousemove", "web_sys::MouseEvent", "mousemove"),
    ("onmousedown", "web_sys::MouseEvent", "mousedown"),
    ("onmouseup", "web_sys::MouseEvent", "mouseup"),
    ("ondblclick", "web_sys::MouseEvent", "dblclick"),
    ("onkeydown", "web_sys::KeyboardEvent", "keydown"),
    ("onkeyup", "web_sys::KeyboardEvent", "keyup"),
    ("onkeypress", "web_sys::KeyboardEvent", "keypress"),
    ("oninput", "web_sys::InputEvent", "input"),
    ("onchange", "web_sys::Event", "change"),
    ("onsubmit", "web_sys::Event", "submit"),
    ("onfocus", "web_sys::FocusEvent", "focus"),
    ("onblur", "web_sys::FocusEvent", "blur"),
    ("onmouseover", "web_sys::MouseEvent", "mouseover"),
    ("onmouseout", "web_sys::MouseEvent", "mouseout"),
    ("onwheel", "web_sys::WheelEvent", "wheel"),
    ("ontouchstart", "web_sys::TouchEvent", "touchstart"),
    ("ontouchend", "web_sys::TouchEvent", "touchend"),
    ("ontouchmove", "web_sys::TouchEvent", "touchmove"),
    ("ontouchcancel", "web_sys::TouchEvent", "touchcancel"),
    // Add more event-function pairs as needed
];

pub struct CompileOutput {
    pub state_rs: String,
    pub startup_js: String,
}

/// Main compile function
/// # Arguments
/// * `filepath` - Path to the root of the RSvelte project
/// # Returns
/// * `Result<CompileOutput>` - Generated code strings
pub fn compile(filepath: &str) -> Result<CompileOutput, CompileError> {
    // 1. Parse
    let components = match get_all_components(filepath) {
        Ok(comps) => {
            info!("Parsing {}", "SUCCEEDED".green());
            comps
        }
        Err(err) => {
            error!("Parsing {}: \n   {}", "FAILED".red(), err);
            return Err(err);
        }
    };

    // 2. Transform
    let context = match transform::transform(components) {
        Ok(ctx) => {
            info!("Transformation {}", "SUCCEEDED".green());
            ctx
        }
        Err(err) => {
            error!("Transformation {}: \n   {}", "FAILED".red(), err);
            return Err(err);
        }
    };

    // 3. Codegen
    match code_gen::code_gen(context) {
        Ok(comile_out) => {
            info!("Code Generation {}", "SUCCEEDED".green());
            Ok(comile_out)
        }
        Err(err) => {
            error!("Code Generation {}: \n   {}", "FAILED".red(), err);
            Err(err)
        }
    }
}

/// Compile .rsvelte file to WebAssembly module in provided output directory
/// 
/// 
pub fn compile_to_wasm(filepath: &str, output_path: &str) -> Result<(), CompileError> {
    let compile_out = compile(filepath)?;

    let temp_dir: TempDir = TempDir::new("rs_output")?;
    let compile_path = temp_dir.path();
    let src_path = compile_path.join("src");
    std::fs::create_dir(&src_path)?;
    std::fs::write(&src_path.join("lib.rs"), LIB_RS)?;
    std::fs::write(src_path.join("state.rs"), compile_out.state_rs)?;
    std::fs::write(compile_path.join("Cargo.toml"), CARGO_TOML)?;

    // Compile to WASM using cargo
    let status = std::process::Command::new("cargo")
        .arg("build")
        .arg("--target")
        .arg("wasm32-unknown-unknown")
        .current_dir(compile_path)
        .status()?;
    if !status.success() {
        return Err(generic_error("Cargo build failed"));
    }

    // Copy generated files to output_path
    let pkg_path = compile_path
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("debug");
    std::fs::create_dir_all(output_path)?;
    std::fs::copy(
        pkg_path.join("rs_output.wasm"),
        std::path::Path::new(output_path).join("main.wasm"),
    )?;

    Ok(())
}

cfg_if::cfg_if! {
    if #[cfg(target_os="windows")] {
        const LIB_RS: &str = include_str!(r"static_files\lib.rs");
        const PATCHES_JS: &str = include_str!(r"static_files\patches.js");
        const INDEX_HTML: &str = include_str!(r"static_files\index.html");
        const CARGO_TOML: &str = include_str!(r"static_files\Cargo.toml");
    } else if #[cfg(any(target_os="linux", target_os="macos"))] {
        const LIB_RS: &str = include_str!(r"static_files/lib.rs");
        const PATCHES_JS: &str = include_str!(r"static_files/patches.js");
        const INDEX_HTML: &str = include_str!(r"static_files/index.html");
        const CARGO_TOML: &str = include_str!(r"static_files/Cargo.toml");
    }
}

/// Setup output directory for manual development
///
/// Produces the following structure:
/// ```
/// output/
/// ├── src/
/// │   ├── lib.rs
/// │   └── state.rs*
/// ├── pkg/        (bindgen files)
/// ├── Cargo.toml
/// ├── startup.js*
/// ├── patches.js
/// └── index.html
/// ```
/// `*`= generated by the compiler and must be written separately
pub fn setup_dir(output_path: &str) -> Result<(), CompileError> {
    // Create output directory structure (delete if exists)
    let output_as_path = std::path::Path::new(output_path);
    if output_as_path.exists() {
        std::fs::remove_dir_all(output_as_path)?;
    }
    std::fs::create_dir_all(output_as_path)?;
    std::fs::create_dir(output_as_path.join("src"))?;
    std::fs::create_dir(output_as_path.join("pkg"))?;

    // Copy static files from static_files/
    std::fs::write(format!("{}/src/lib.rs", output_path), LIB_RS)?;
    std::fs::write(format!("{}/patches.js", output_path), PATCHES_JS)?;
    std::fs::write(format!("{}/index.html", output_path), INDEX_HTML)?;
    std::fs::write(format!("{}/Cargo.toml", output_path), CARGO_TOML)?;
    Ok(())
}

pub struct StaticFiles {
    pub lib_rs: String,
    pub patches_js: String,
    pub index_html: String,
    pub cargo_toml: String,
}

/// Get static files as strings
///
/// Note that index.html and patches.js must be put on level with
/// startup.js, and lib.rs must go into src/ with state.rs and Cargo.toml in the parent.
///
/// # Example structure
/// ```
/// output/
/// ├── rust-gen/
/// │   ├── src/
/// │   │   ├── lib.rs
/// │   │   └── state.rs
/// │   └── Cargo.toml
/// └── build/
///     ├── pkg/        (bindgen files)
///     ├── startup.js
///     ├── patches.js
///     └── index.html
/// ```
pub fn get_static_files() -> StaticFiles {
    StaticFiles {
        lib_rs: LIB_RS.to_string(),
        patches_js: PATCHES_JS.to_string(),
        index_html: INDEX_HTML.to_string(),
        cargo_toml: CARGO_TOML.to_string(),
    }
}
