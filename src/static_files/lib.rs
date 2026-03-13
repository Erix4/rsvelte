use crate::state::PageRootFrag;
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
use web_sys::{Comment, Element, Node};

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

trait ComponentState {
    fn new() -> Self;
    fn init(&mut self);
    fn update_derived(&mut self);
}

struct Component<T: RootFragment> {
    contents: T,
    state: T::State,
}

impl<T: RootFragment> Component<T> {
    fn new(current_path: &Vec<u32>) -> Result<Self, JsValue> {
        web_sys::console::log_1(&"Initializing Page component".into());

        let state = T::State::new();
        let contents = T::new(&state, (), current_path)?;
        let mut new_page = Self { contents, state };

        DIRTY_FLAGS.store(u64::MAX, SeqCst); // mark all as dirty for initial render
        new_page.apply()?;

        Ok(new_page)
    }

    fn mount(&self, add_method: impl AddMethod) -> Result<(), JsValue> {
        self.contents.mount(add_method)
    }

    /// Process an event and return patches to apply to the DOM
    ///
    /// The target_path describes where the event took place.
    /// This function will check if the current component is the target,
    /// and if so run the corresponding user code for that event.
    /// Otherwise, it will propagate the event to the correct child component to process.
    ///
    /// If handled by a child, the function checks for changes in bindable props and updates
    /// the state of the current component accordingly, and marking them to be excluded from
    /// propagation back down to the child to avoid feedback loops.
    ///
    /// Finally, it runs the apply function, to derived, generate patches,
    /// and propagate any changes to children as needed.
    fn proc(&mut self, e: web_sys::Event, target_path: Vec<u32>, _: ()) -> Result<(), JsValue> {
        web_sys::console::log_1(
            &format!(
                "Processing event: {}, target path: {:?}",
                e.type_(),
                target_path
            )
            .into(),
        );
        // Event handling
        self.contents.proc(&mut self.state, (), e, target_path)?;

        self.apply()?;

        Ok(())
    }

    /// Apply changes to the DOM based on the current state and dirty flags
    fn apply(&mut self) -> Result<(), JsValue> {
        let state = &mut self.state;

        // update derived
        state.update_derived();

        // generate patches based on dirty flags
        let flag_snapshot = DIRTY_FLAGS.load(SeqCst);

        self.contents.update(state, (), flag_snapshot)?;

        // Restore snapshot
        DIRTY_FLAGS.store(flag_snapshot, SeqCst);

        Ok(())
    }
}

trait RootFragment {
    type State: ComponentState;

    fn new(state: &Self::State, scope: (), current_path: &Vec<u32>) -> Result<Self, JsValue>
    where
        Self: Sized;
    fn mount(&self, add_method: impl AddMethod) -> Result<(), JsValue>;
    fn proc(
        &mut self,
        state: &mut Self::State,
        scope: (),
        e: web_sys::Event,
        target_path: Vec<u32>,
    ) -> Result<(), JsValue>;
    fn update(&mut self, state: &mut Self::State, scope: (), flags: u64) -> Result<(), JsValue>;
    fn unmount(&self);
}

trait GenericFragment {
    type State;
    type Scope<'a>: Copy; // this can implement Copy 'cause it's all references

    fn new(state: &Self::State, scope: Self::Scope<'_>, current_path: &Vec<u32>) -> Result<Self, JsValue>
    where
        Self: Sized;
    fn mount(&self, parent: &Element, add_method: impl AddMethod) -> Result<(), JsValue>;
    fn proc(
        &mut self,
        state: &mut Self::State,
        scope: Self::Scope<'_>,
        e: web_sys::Event,
        target_path: Vec<u32>,
    ) -> Result<(), JsValue>;
    fn update(
        &mut self,
        parent: &Element,
        state: &Self::State,
        scope: Self::Scope<'_>,
        flags: u64,
    ) -> Result<(), JsValue>;
    fn unmount(&self);
}

struct IfElement<T: IfContentTrait> {
    pub comment: Comment,
    pub content_enum: T,
    current_path: Vec<u32>,
}

impl<T> Clone for IfElement<T>
where
    T: IfContentTrait + Clone,
{
    fn clone(&self) -> Self {
        Self {
            comment: self.comment.clone(),
            content_enum: self.content_enum.clone(),
            current_path: self.current_path.clone(),
        }
    }
}

trait IfContentTrait {
    type State;
    type Scope<'a>: Copy; // this can implement Copy 'cause it's all references

    // State, Scope (internal references in nested tuples)
    fn branch_changed(&self, state: &Self::State, _scope: Self::Scope<'_>, flags: u64) -> bool;
    fn new(state: &Self::State, scope: Self::Scope<'_>, current_path: &Vec<u32>) -> Result<Self, JsValue>
    where
        Self: Sized;
    fn mount(&self, parent: &Element, add_method: impl AddMethod) -> Result<(), JsValue>;
    fn proc(
        &mut self,
        state: &mut Self::State,
        scope: Self::Scope<'_>,
        e: web_sys::Event,
        target_path: Vec<u32>,
    ) -> Result<(), JsValue>;
    fn update(
        &mut self,
        parent: &Element,
        state: &Self::State,
        scope: Self::Scope<'_>,
        flags: u64,
    ) -> Result<(), JsValue>;
    fn unmount(&self);
}

impl<T: IfContentTrait> GenericFragment for IfElement<T> {
    type State = T::State;
    type Scope<'a> = T::Scope<'a>;

    fn new(state: &Self::State, scope: Self::Scope<'_>, current_path: &Vec<u32>) -> Result<Self, JsValue> {
        let window = web_sys::window().expect("no global window exists");
        let document = window.document().expect("no document on window exists");

        Ok(Self {
            comment: document.create_comment(""),
            content_enum: T::new(state, scope, current_path)?,
            current_path: current_path.clone(),
        })
    }

    fn mount(&self, parent: &Element, add_method: impl AddMethod) -> Result<(), JsValue> {
        add_method(&self.comment)?;
        self.content_enum
            .mount(parent, comment_insert_closure(&self.comment, parent))
    }

    fn proc(
        &mut self,
        state: &mut Self::State,
        scope: Self::Scope<'_>,
        e: web_sys::Event,
        target_path: Vec<u32>,
    ) -> Result<(), JsValue> {
        self.content_enum.proc(state, scope, e, target_path)
    }

    fn update(
        &mut self,
        parent: &Element,
        state: &Self::State,
        scope: Self::Scope<'_>,
        flags: u64,
    ) -> Result<(), JsValue> {
        if self.content_enum.branch_changed(state, scope, flags) {
            // Unmount old content
            self.content_enum.unmount();

            // Mount new content
            self.content_enum = T::new(state, scope, &self.current_path)?;
            self.content_enum
                .mount(parent, comment_insert_closure(&self.comment, parent))?;
        }
        self.content_enum.update(parent, state, scope, flags)
    }

    fn unmount(&self) {
        self.content_enum.unmount();
        self.comment.remove();
    }
}

/// Represents the content of an each block, which may have multiple instances
/// Each instance is identified by a unique key produced by a hash function
struct EachElement<T: EachContentTrait> {
    pub comment: web_sys::Comment,
    pub content: Vec<(u64, T, T::Item)>, // (hash, DOM ref, item)
    current_path: Vec<u32>,
}

/// Functions which the fragment inside an each block must implement to be used as content for an EachElement
trait EachContentTrait {
    type State;
    type Scope<'a>: Copy; // this can implement Copy 'cause it's all references

    // State, Scope (internal references in nested tuples)
    type Item: std::hash::Hash;

    fn generate(state: &Self::State, scope: Self::Scope<'_>, flags: u64)
        -> Option<Vec<Self::Item>>;
    fn new(state: &Self::State, scope: (Self::Scope<'_>, &Self::Item), current_path: &Vec<u32>) -> Result<Self, JsValue>
    where
        Self: Sized;
    fn mount(&self, parent: &Element, add_method: impl AddMethod) -> Result<(), JsValue>;
    fn proc(
        &self,
        state: &Self::State,
        scope: (Self::Scope<'_>, &Self::Item),
        e: web_sys::Event,
        target_path: Vec<u32>,
    ) -> Result<(), JsValue>;
    fn update(
        &mut self,
        parent: &Element,
        state: &Self::State,
        scope: (Self::Scope<'_>, &Self::Item),
        flags: u64,
    ) -> Result<(), JsValue>;
    fn unmount(&self);
}

impl<T: EachContentTrait + Clone> GenericFragment for EachElement<T> {
    type State = T::State;
    type Scope<'a> = T::Scope<'a>;

    fn new(state: &Self::State, scope: Self::Scope<'_>, current_path: &Vec<u32>) -> Result<Self, JsValue> {
        let window = web_sys::window().expect("no global window exists");
        let document = window.document().expect("no document on window exists");

        Ok(Self {
            comment: document.create_comment(""),
            content: T::generate(state, scope, u64::MAX)
                .unwrap()
                .into_iter()
                .map(|item| {
                    let hash = hash_item(&item);
                    let content = T::new(state, (scope, &item), current_path).expect("Failed to create content");
                    (hash, content, item)
                })
                .collect(),
            current_path: current_path.clone(),
        })
    }

    fn mount(&self, parent: &Element, add_method: impl AddMethod) -> Result<(), JsValue> {
        add_method(&self.comment)?;
        for (_, content, _) in &self.content {
            content.mount(parent, &comment_insert_closure(&self.comment, parent))?;
        }
        Ok(())
    }

    fn proc(
        &mut self,
        state: &mut Self::State,
        scope: Self::Scope<'_>,
        e: web_sys::Event,
        target_path: Vec<u32>,
    ) -> Result<(), JsValue> {
        for (_, content, item) in &self.content {
            content.proc(state, (scope, item), e.clone(), target_path.clone())?;
        }
        Ok(())
    }

    fn update(
        &mut self,
        parent: &Element,
        state: &Self::State,
        scope: Self::Scope<'_>,
        flags: u64,
    ) -> Result<(), JsValue> {
        // Diff & update each list if necessary
        if let Some(new_items) = T::generate(state, scope, flags) {
            let new_hashes: Vec<u64> = new_items.iter().map(|item| hash_item(item)).collect();
            let mut takeable_new_items: Vec<Option<T::Item>> =
                new_items.into_iter().map(Some).collect();

            // Find head and tail of middle batch
            let mut head = 0;
            while head < self.content.len()
                && head < new_hashes.len()
                && self.content[head].0 == new_hashes[head]
            {
                head += 1;
            }
            let mut tail = 0;
            while tail < self.content.len() - head
                && tail < new_hashes.len() - head
                && self.content[self.content.len() - 1 - tail].0
                    == new_hashes[new_hashes.len() - 1 - tail]
            {
                tail += 1;
            }

            // Build a map of new hashes to their indices for quick lookup
            let mut new_item_source_map = std::collections::HashMap::new();
            for i in head..(new_hashes.len() - tail) {
                new_item_source_map.insert(new_hashes[i], i);
            }

            // Create a new array to hold the source indices for the new items
            // TODO: items before and after middle batch should be handled separately to avoid unnecessary moves
            let mut new_item_source_array = vec![None; new_hashes.len()];
            for i in 0..head {
                new_item_source_array[i] = Some(i);
            }
            for i in head..(self.content.len() - tail) {
                if let Some(&new_index) = new_item_source_map.get(&self.content[i].0) {
                    new_item_source_array[new_index] = Some(i);
                } else {
                    // Unmount code for removed item
                    self.content[i].1.unmount();
                }
            }
            for i in (new_hashes.len() - tail)..new_hashes.len() {
                new_item_source_array[i] = Some(self.content.len() - (new_hashes.len() - i));
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

            let mut new_list = Vec::new();
            for (new_index, source_index_opt) in new_item_source_array.into_iter().enumerate() {
                if let Some(source_index) = source_index_opt {
                    if !longest_sub.contains(&new_index) {
                        // Move existing item to correct position
                        let new_item = takeable_new_items[new_index].take().unwrap();
                        let contents = &self.content[source_index].1;
                        contents.unmount();
                        contents.mount(parent, comment_insert_closure(&self.comment, parent))?;
                        new_list.push((self.content[source_index].0, contents.clone(), new_item));
                    } else {
                        // Item is already in correct position, just update anchor for next iteration
                        let new_item = takeable_new_items[new_index].take().unwrap();
                        new_list.push((
                            self.content[source_index].0,
                            self.content[source_index].1.clone(),
                            new_item,
                        ));
                    }
                } else {
                    // Mount new item
                    let new_item = takeable_new_items[new_index].take().unwrap();
                    let new_contents = T::new(state, (scope, &new_item), &self.current_path)?;
                    new_contents.mount(parent, comment_insert_closure(&self.comment, parent))?;
                    new_list.push((new_hashes[new_index], new_contents.clone(), new_item));
                }
            }
            self.content = new_list.into_iter().collect();
        }

        // Update all items (including moved ones) with new scope and flags
        for (_, content, item) in &mut self.content {
            content.update(parent, state, (scope, item), flags)?;
        }

        Ok(())
    }

    fn unmount(&self) {
        for (_, content, _) in &self.content {
            content.unmount();
        }
        self.comment.remove();
    }
}

fn hash_item<T: std::hash::Hash>(item: &T) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;

    let mut hasher = DefaultHasher::new();
    item.hash(&mut hasher);
    hasher.finish()
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

/// Each route maps a path pattern to a component constructor
/*pub struct Route {
    pattern: &'static str,
    // Params extracted from URL (e.g., /users/:id)
    param_names: &'static [&'static str],
}*/

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

pub trait AddMethod: Fn(&Node) -> Result<(), JsValue> {}
impl<F: Fn(&Node) -> Result<(), JsValue>> AddMethod for F {}

fn child_append_closure(parent: &Element) -> impl AddMethod + '_ {
    let closure = move |el: &Node| {
        parent.append_child(el)?;
        Ok(())
    };
    closure
}

fn comment_insert_closure<'a>(comment: &'a Comment, parent: &'a Element) -> impl AddMethod + 'a {
    let closure = move |el: &Node| {
        parent.insert_before(el, Some(comment))?;
        Ok(())
    };
    closure
}

thread_local! {
    pub static PAGE: RefCell<Option<Component<PageRootFrag>>> = RefCell::new(None);
}

#[wasm_bindgen]
pub fn mount() -> Result<(), JsValue> {
    web_sys::console::log_1(&"Mounting application".into());
    PAGE.with(|page| {
        let window = web_sys::window().expect("no global window exists");
        let document = window.document().expect("no document on window");
        let body = document.body().expect("document should have a body");

        let new_page = Component::<PageRootFrag>::new(&vec![])?;
        new_page.mount(child_append_closure(&body))?;
        *page.borrow_mut() = Some(new_page);
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
