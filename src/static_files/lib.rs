use std::{
    cell::RefCell,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicU64, Ordering::SeqCst},
    vec,
};

use wasm_bindgen::{
    prelude::{wasm_bindgen, Closure},
    JsCast, JsValue,
};
use web_sys::{Comment, Node};

use crate::state::Page;

mod state;

pub static DIRTY_FLAGS: AtomicU64 = AtomicU64::new(0);

pub struct MutateTracker<T> {
    value: T,
    id: u32,
}

impl<T> MutateTracker<T> {
    pub fn new(value: T, id: u32) -> Self {
        Self { value, id }
    }
}

impl<T> Deref for MutateTracker<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T> DerefMut for MutateTracker<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        web_sys::console::log_1(&format!("DerefMut called for id {}", self.id).into());
        DIRTY_FLAGS.fetch_or(1 << self.id, std::sync::atomic::Ordering::SeqCst);
        &mut self.value
    }
}

pub struct IfElement<T> {
    pub comment: Comment,
    pub active_branch: u64,
    pub content_enum: T,
}

/// Represents the content of an each block, which may have multiple instances
/// Each instance is identified by a unique key produced by a hash function
pub struct EachElement<T> {
    pub comment: Comment,
    pub content: Vec<(u64, T)>,
}

fn hash_item<T: std::hash::Hash>(item: &T) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;

    let mut hasher = DefaultHasher::new();
    item.hash(&mut hasher);
    hasher.finish()
}

/// Diffs the existing content of an each block with the new items,
/// unmounting removed items, mounting new items, and moving existing items as needed to match the new order.
/// Returns the new list of (hash, content) pairs in the correct order.
/// 
/// Uses the Longest Increasing Subsequence algorithm to minimize moves of existing items.
fn diff_each_content<T: std::hash::Hash, U: Clone>(
    existing: &Vec<(u64, U)>,
    new_items: Vec<T>,
    parent: &Node,
    mount_anchor: Comment,
    unmount_fn: impl Fn(&U),
    create_fn: impl Fn(T) -> Result<U, JsValue>,
    mount_fn: impl Fn(&Node, &Node, &U) -> Result<(), JsValue>, // returns the new anchor after mounting
) -> Result<Vec<(u64, U)>, JsValue>
where
    Node: From<U>,
{
    let new_hashes: Vec<u64> = new_items.iter().map(|item| hash_item(item)).collect();
    let mut takeable_new_items: Vec<Option<T>> = new_items.into_iter().map(Some).collect();

    // Find head and tail of middle batch
    let mut head = 0;
    while head < existing.len() && head < new_hashes.len() && existing[head].0 == new_hashes[head] {
        head += 1;
    }
    let mut tail = 0;
    while tail < existing.len() - head
        && tail < new_hashes.len() - head
        && existing[existing.len() - 1 - tail].0 == new_hashes[new_hashes.len() - 1 - tail]
    {
        tail += 1;
    }

    // Build a map of new hashes to their indices for quick lookup
    let mut new_item_source_map = std::collections::HashMap::new();
    for i in head..(new_hashes.len() - tail) {
        new_item_source_map.insert(new_hashes[i], i);
    }

    // Create a new array to hold the source indices for the new items
    let mut new_item_source_array = vec![None; new_hashes.len()];
    for i in 0..head {
        new_item_source_array[i] = Some(i);
    }
    for i in head..(existing.len() - tail) {
        if let Some(&new_index) = new_item_source_map.get(&existing[i].0) {
            new_item_source_array[new_index] = Some(i);
        } else {
            // Unmount code for removed item
            unmount_fn(&existing[i].1);
        }
    }
    for i in (new_hashes.len() - tail)..new_hashes.len() {
        new_item_source_array[i] = Some(existing.len() - (new_hashes.len() - i));
    }

    // Find longest increasing subsequence of source indices in new_item_source_array
    let mut subs = vec![Vec::new()]; // list of all increasing subsequences found so far
    let mut last_index: i32 = -1; // index of the last item in the longest increasing subsequence
    for (new_index, source_index_opt) in new_item_source_array.iter().enumerate() {
        let current_sub = subs.last_mut().unwrap();
        if let Some(source_index) = source_index_opt {
            if current_sub.is_empty() || *source_index as i32 > last_index {
                current_sub.push(new_index);
            } else {
                subs.push(vec![new_index]);
            }
            last_index = *source_index as i32;
        }
    }
    let longest_sub = subs
        .into_iter()
        .max_by_key(|sub| sub.len())
        .unwrap_or_default();

    let mut anchor: Node = mount_anchor.into();
    let mut new_list = Vec::new();
    for (new_index, source_index_opt) in new_item_source_array.into_iter().enumerate().rev() {
        if let Some(source_index) = source_index_opt {
            if !longest_sub.contains(&new_index) {
                // Move existing item to correct position
                let item = &existing[source_index].1;
                unmount_fn(item);
                mount_fn(&parent, &anchor, &item)?;
                new_list.push((existing[source_index].0, item.clone()));
                anchor = item.clone().into();
            } else {
                // Item is already in correct position, just update anchor for next iteration
                anchor = existing[source_index].1.clone().into();
                new_list.push((existing[source_index].0, existing[source_index].1.clone()));
            }
        } else {
            // Mount new item
            let new_item = create_fn(takeable_new_items[new_index].take().unwrap())?;
            mount_fn(&parent, &anchor, &new_item)?;
            new_list.push((new_hashes[new_index], new_item.clone()));
            anchor = new_item.into();
        }
    }
    Ok(new_list.into_iter().rev().collect())
}

pub fn prepend_path(base: &Vec<u32>, addition: u32) -> Vec<u32> {
    let mut out = vec![addition];
    out.extend_from_slice(base);
    out
}

pub fn add_listener(
    el: &web_sys::Element,
    event: &str,
    target_path: Vec<u32>,
) -> Result<(), JsValue> {
    let closure = Closure::wrap(Box::new(move |e: web_sys::Event| {
        handle_event(e, target_path.clone());
    }) as Box<dyn FnMut(_)>);
    el.add_event_listener_with_callback(event, closure.as_ref().unchecked_ref())?;
    closure.forget();
    Ok(())
}

/// Simple pattern matching: "/users/:id" matches "/users/42"
pub fn match_pattern(
    pattern: &str,
    path: &str,
    param_names: &[&str],
) -> Option<Vec<(String, String)>> {
    let pattern_parts: Vec<&str> = pattern.split('/').collect();
    let path_parts: Vec<&str> = path.split('/').collect();

    if pattern_parts.len() != path_parts.len() {
        return None;
    }

    let mut params = Vec::new();
    let mut param_idx = 0;

    for (p, actual) in pattern_parts.iter().zip(path_parts.iter()) {
        if p.starts_with(':') {
            params.push((param_names[param_idx].to_string(), actual.to_string()));
            param_idx += 1;
        } else if p != actual {
            return None;
        }
    }

    Some(params)
}

thread_local! {
    pub static PAGE: RefCell<Option<Page>> = RefCell::new(None);
}

#[wasm_bindgen]
pub fn mount() -> Result<(), JsValue> {
    web_sys::console::log_1(&"Mounting application".into());
    PAGE.with(|page| {
        *page.borrow_mut() = Some(Page::new(vec![])?);
        web_sys::console::log_1(&"Page component mounted".into());
        Ok(())
    })
}

pub fn handle_event(e: web_sys::Event, target: Vec<u32>) {
    DIRTY_FLAGS.store(0, SeqCst);
    let _ = PAGE.with(|page| {
        let page = &mut *page.borrow_mut();
        let page = page.as_mut().expect("Page component should be initialized");
        page.proc(e, target, ())
            .or_else(|e| {
                web_sys::console::error_1(&format!("Error processing event: {:?}", e).into());
                Err(e)
            })
            .ok();
    });
}
