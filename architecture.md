# Compiler & Reactivity Architecture

## Reactivity Model

RSvelte's reactivity model closely follows Svelte's, where state changes are entirely dependent on user events. When an element with an event listener (like `onclick={my_function}`) is mounted, an event listened is added which points to the top layer of the site's event handling tree: `handle_event`. From here, the event is propagated down, through the component tree, until it reaches the component with the element that has the event listener. This propagation is all done through the `proc()` function of each component. Once the element is found, the state-mutated user code is run.

Mutations in RSvelte are tracked, like Svelte, through the use of mutation flags in a global atomic `DIRTY_FLAGS` variable. Unlike Svelte, which identifies all user mutations of state and replaces them with getter and setter functions, RSvelte does not try to find all the ways in which you might mutate your state. Instead, it relies on Rust's built in dereferencing inference, which can automatically determine which variables need to be dereferenced mutably. All user settable state variables (which means `$props and $derived` are not included) are wrapped in a `MutationTracker<T>` struct, which implements `Deref` and `DerefMut` traits. When a variable is dereferenced mutably, the `DerefMut` implementation sets the corresponding mutation flag in `DIRTY_FLAGS`. This allows us to track mutations without needing to rewrite user code (apart from dereferencing where necessary), and also allows us to detect possible mutations if a mutable reference to state is passed into a function.

Once state has been mutated, the `apply()` function is called on that component. This is where the magic happens.

First, we propagate state changes to `derived` variables, setting the corresponding mutation flags if they are mutated. Then, we take a snapshot of the mutation flags and update all DOM elements in this component. Fragments for #if and #each blocks get their own functions which are called to determine when #if branches are switched or #each items need to updated (RSvelte copies Svelte's diffing method, using a hashing function instead of keys to identify retained items and minimize DOM manipulation). Then, for components whose props rely on the affected state, we reset the mutation flags and update the props for the component, setting corresponding flags for the ones set. Now we can propagate down to the child component's `apply()` function and repeat the process.

After downward propagation, we restore the mutation flags to their snapshot and exit the `apply()` and `proc()` functions for the targeted component. Now we traverse back up the call stack, and for each parent component, we check if the set mutation flags affect any bindable props. If so, we update the bindable props and set the corresponding mutation flags. Now we call `apply()` on that component as well, which also propagates down to its affected children. This process is repeated until we reach the top of the component tree, at which point all affected components have been updated.

Here is a diagram of the event handling and state update process:

```
User clicks <button onclick={increment}>
                        │
                        ▼
               ┌─────────────────┐
               │  handle_event() │
               └────────┬────────┘
                        │
               ┌────────▼────────┐      ┌─────────────────┐
               │ Page::proc()    │  ──> │ Page::apply()   │
               └────────┬────────┘      └────────┬────────┘
                        │                        │
               ┌────────▼────────┐      ┌────────▼────────┐
               │ Parent::proc()  │  ──> │ Parent::apply() │
               └────────┬────────┘      └────────┬────────┘
                        │                        │
    ┌───────────────────▼────────┐      ┌────────▼────────┐
    │ Child::proc()              │  ──> │ Child::apply()  │
    │                            │      └────────┬────────┘
    │ - run user code (set flags)│               │
    │ - call apply               │               │
    │ - restore flags, move up   │               │
    └────────────────────────────┘               │     
                                                 │
                ┌────────────────┐      ┌────────▼────────┐
 not called ->  │ Child2::proc() │  ──> │ Child2::apply() │
                └────────────────┘      └─────────────────┘
```

### Diffing an #each block

RSvelte, like Svelte, applies a diffing algorithm to arrays (or in RSvelte's case, any iterable) used in #each blocks to minimize the amount of DOM manipulation necessary. Whereas Svelte uses a user-specified key to identify unique items for more complex data types, RSvelte attempts to hash the items. This means any iterable's items must implement the `Hash` trait to be used in an #each block (which most simple types are by default). For most other types, it is easy to `#[derive(Hash)]` to add support, or, if you want more fine grained control, add a custom `Hash` implementation which only hashes the fields you want to use for comparison.

Once hashes are obtained for all items, RSvelte builds a map of sources at each index of the new iterable (with None for new items), unmounting any items that are no longer present. The source map is only built for the "middle batch", which is the group of items with the first and last increasing subsequence removed. From the middle batch, we also finds the longest increasing subsequence (LIS), which is the longest sequence of items that are in the same order in both the old and new iterables. Items in the LIS do not need to be moved, but all others in the middle batch will be unmounted and re-mounted according to the source map. Items outside of the middle batch likewise are not moved.

### Code generation structure

Code from the compiler is built into two files: `lib.rs`, which is entirely static on not dependent on user input. This includes things like the diffing algorithm and type definitions which are uniform across projects. The `state.rs` file is completely generated by the compiler and contains all generated reactive & user-written code. The `lib.rs` file also contains the statically owned top-level page state in a `RefCell`, which is borrowed and passed down when user events are handled.

## Compiler Architecture

### Broadstrokes

The compiler is split into three main stages:
1. **Parsing**: The `.rsvelte` file is parsed into several Abstract Syntax Trees (AST) that represents the structure of the components, including their HTML-like tags, expressions, and directives.
2. **Transformation**: The ASTs are transformed into an intermediate representation that captures the reactive dependencies and component structure in a way that can be easily converted to Rust code. This stage involves analyzing the AST to identify reactive statements, scoped variables, and the component hierarchy.
3. **Code Generation**: The intermediate representation is then converted into Rust code that defines each component's structure, event handling, and DOM reactivity. This code is designed to be compiled to WebAssembly for execution in the browser.

### Detailed Architecture

#### Parsing

First, we divide the `.rsvelte` file into its three main sections: the script, the template, and the styles.
- The script section contains Rust code that defines the component's logic, state, and functions.
- The template section contains the HTML-like structure of the component, along with any Svelte-like directives (e.g., `#if`, `#each`, etc.) and expressions.
- The styles section contains CSS that is scoped to the component.

Reading the script section is the easy part. Functions can be copied directly (and later moved onto the reactive state struct directly), and state variables (include $state, $props, $bindable, and $derived) are identified in the $state struct and stored. Other "agnostic code" like type definitions and imports are also stored verbatim, with the exception of component imports. At this stage, there is very little validation of syntax, just enough to parse the code. This is all done through the common Rust parsing library `syn`, which is often used for complex macros.

CSS is even easier, as we can simply read each style selector and corresponding rules into an array to be scoped and injected into the page later.

The template section (with the actual HTML) is most complex, and is in charge of building an Abstract Syntax Tree (AST) that represents all of the HTML tags and elements, Svelte components, as well as all the Svelte-specific structures like #if and #each blocks. It's important to note at this stage that the AST does *not* represent the final structure of the DOM, or of the fragments. Both of these things are flatter than the AST and flatten different nodes together to achieve their structure. Additionally, at this stage, there is no distinction between the functionality of attributes, only of their type (static string, expression, function call, or closure).

#### Transformation
This is the stage at which components are linked up, reactive expressions, function calls, and event listeners are identified, and any necessary modifications are made to user code. This is also where we assign mutation flags to each reactive variable.

When this stage is completed, we have a tree of `Node`s which are able to directly generate DOM manipulation code (specifically in the categories of creation, mounting, updating, and unmounting). A recursive function is called to walk down the tree, identify new fragments (when #if or #each blocks start), and build functions responsible for their DOM manipulation which can be called by their parents. Moving fragment DOM manipulation into functions allows us to reuse code and reduce the size of the compiled `.wasm` file, which is important to optimize load times.

Here, we also link the components together through their nested `proc()` functions, which directs user interactions to the listening event handler, propagates state changes (downward through `props` and upwards through `bindables` and callbacks), and applies the corresponding DOM updates to affected elements.

One tricky detail is that elements in #if and #each blocks need two things to be inserted into the DOM: it's parent node (aka the tag which is lives inside, which is not related to the fragment it's in), and the comment anchor (used to mark the position at which the element should be inserted). These things are pulled from two different trees, both of which are pulled out of the AST. While these trees are not explicitly built into a data structure, they emerge from the pattern of recursion and DOM construction.

The DOM tree completely ignores all #if and #each blocks, and several layers of nested blocks can all have the same parent and only be differentiated by their anchor. On the other hand, the fragment tree groups together all elements (parent and child) except for fragments. This is necessary to allow dynamic mounting, unmounting, and swapping of the internal elements. Inside fragments, all elements are once again grouped together, apart from other child fragments. Because the fragment structure is different from the DOM structure, and the parent node is required to mount new elements, parent nodes are passed down as arguments through the tree of fragment functions. Also passed down are scoped variables, aka the variables introduced in #each blocks which are dynamically generated based on a user expression.

Here is a diagram of the relationship between the AST, DOM tree, and fragment tree:

Given this `.rsvelte` template:
```html
<div>
  <h1>Title</h1>
  {#if show}
    <p>Visible</p>
    {#each items as item}
      <span>{item}</span>
    {/each}
  {/if}
  <footer>End</footer>
</div>
```

```
 AST                         DOM Tree                  Fragment Tree
 (mirrors source)            (ignores blocks)          (groups by fragment)

 <div>                       <div>                     Root Fragment
 ├─ <h1>                     ├─ <h1>                   ├─ <div>
 │  └─ "Title"               │  └─ "Title"             │  ├─ <h1>
 ├─ {#if show}               ├─ <p>                    │  │  └─ "Title"
 │  ├─ <p>                   │  └─ "Visible"           │  ├─ <!--if-anchor-->
 │  │  └─ "Visible"          ├─ <span>                 │  ├─ <footer>
 │  └─ {#each items}         │  └─ {item}              │  │  └─ "End"
 │     └─ <span>             ├─ <!--each-anchor-->     │  └────────────────
 │        └─ {item}          ├─ <!--if-anchor-->       │
 ├─ <footer>                 ├─ <footer>               └─ If Fragment (parent: <div>)
 │  └─ "End"                 │  └─ "End"                  ├─ <p>
 └────────────────           └────────────────            │  └─ "Visible"
                                                          ├─ <!--each-anchor-->
  Mirrors the source          All elements are            │
  structure exactly,          direct children of          └─ Each Fragment (parent: <div>)
  nesting blocks              their DOM parent.              └─ <span>
  inside each other.          <p> and <span> sit                └─ {item}  ← scoped var
                              next to their anchors,
                              not nested in blocks.    Fragments group elements
                                                       for mount/unmount. Parent
                                                       DOM node is passed as arg.
                                                       Each fragment is a separate
                                                       function.
```

#### Code Generation