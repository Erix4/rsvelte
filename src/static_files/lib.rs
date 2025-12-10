use std::sync::atomic::AtomicU32;

use serde::Serialize;
use wasm_bindgen::prelude::*;

mod state;
use state::{affect_state, apply_state};

#[derive(Serialize)]
enum PatchOp {
    SetContent { value: String },
    SetAttribute { name: String, value: String },
}

#[derive(Serialize)]
pub struct Patch {
    target_id: u32,
    operation: PatchOp,
}

static DIRTY_FLAGS: AtomicU32 = AtomicU32::new(0);

#[wasm_bindgen]
pub fn first_patch() -> JsValue {
    state::init();
    // Set all flags to dirty on first patch
    DIRTY_FLAGS.store(u32::MAX, std::sync::atomic::Ordering::SeqCst);
    serde_wasm_bindgen::to_value(&apply_state()).unwrap()
}

#[wasm_bindgen]
pub fn handle_event(e: web_sys::Event, target: u32) -> JsValue {
    DIRTY_FLAGS.store(0, std::sync::atomic::Ordering::SeqCst);
    affect_state(e, target);
    serde_wasm_bindgen::to_value(&apply_state()).unwrap()
}
