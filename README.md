# RSvelte

> NOTE:
> This is a pre-router version of RSvelte, meaning:
> Only the top-level +page.rsvelte will be rendered at `/`, however components can still be imported

RSvelte is a compiler for RSvelte, a Rust-based Svelte-like framework that enables developers to build reactive web applications using Rust. This project focuses on compiling `.rsvelte` component files into Rust code, which can then be compiled to WebAssembly for web deployment.

## Reasons to imitate Svelte

It is the superior framework.

## Downsides of using Rust

- **Web Ecosystem**: Access to web-specific libraries and frameworks is sacrificed.
- **Single Wasm File**: Web Assembly is compiled into a single `.wasm` file, which leads larger file sizes and longer load times compared to JavaScript applications that can leverage code-splitting and lazy loading techniques.
- **DOM Manipulation**: You cannot call DOM APIs directly from Rust.
- **No Hot Reload**: Wasm is completely recompiled on every change, so hot reloading is not feasible.
- **State Ownership**: Managing state in a Rust-based framework can be more complex due to Rust's ownership and borrowing rules, which may require additional boilerplate code compared to JavaScript frameworks that use mutable state more freely.

## Upsides of using Rust

It's the superior language.

_(Faster, safer, strong typing, memory management, etc.)_

## Syntax

RSvelte components use a syntax similar to Svelte 5 (with runes), with a few key differences.

### Script code

Like Svelte, RSvelte components can include a `<script>` block for defining component logic. However, in RSvelte, the script code is written in Rust instead of JavaScript. The largest difference between Svelte and RSvelte's syntax comes from the way state and props are defined. To take advantage of Rust's powerful ownership system, state and props are defined in a special `$state` struct, which the user can implement functions on. A `$state` struct might look like:

```rs
struct $state {
    parent_prop: String = $prop(),

    counter = $state(0),
    my_struct = $state(MyStruct {
        a: 10,
        b: "hello".to_string(),
    }),

    counter_plus_one = $derived(counter + 1),
}
```

Note that the `$state` struct contains normal state variables, props, and derived variables. Variables can also be complex types either defined in the component or imported from a regular Rust file. Strong typing is maintained through component props and, like Rust, will throw errors on type, ownership, or lifetime problems.

To read or mutate state, a function must be defined inside an `impl $state` block. For example, to increment the `counter` variable, you would define a function like this:

```rs
impl $state {
    fn increment_counter(&mut self) {
        self.counter += 1;
    }
}
```

An advantage of Rust is that we can pass references to state variables into functions, allowing code to be defined and re-used outside of components. RSvelte will only check for state changes and update the DOM when you mutate a variable in a `$state` function or pass a mutable reference to a state variable to an external function. An example of passing a reference to a state variables is as follows:

```rs
impl $state {
    fn increment_counter(&mut self) {
        add(&mut self.counter, 5);
    }
}

fn add(a: &mut usize, b: usize) {
  *a += b;
}
```

You'll notice that `self.counter` was not explicitly typed in our `$state` declaration, but the arguments to our external function are. RSvelte will attempt to infer the types of state variables based on their initial value, but you may want to explicitly define their type to ensure it's what you expect. These two declarations are equivalent:

```rs
struct $state {
    counter = $state(0),

    counter: usize = $state(0),
}
```

Or, if you really don't want to specify a state variables type (or want more reusability), you can use generic functions, like so:

```rs
impl $state {
    fn increment_counter(&mut self) {
        self.counter = add(self.counter, 5);
    }
}

fn add<T: std::ops::Add<Output = T>>(a: T, b: T) -> T {
    a + b
}
```

One last important note is on importing other RSvelte components or code. RSvelte components can be imported into other components using the `use` keyword, similar to how modules are imported in Rust. For example, if you have a component defined in `src/components/MyComponent.rsvelte`, you can import it into another component like this:

```rs
use $components::MyComponent;
```

### HTML code

The HTML syntax is almost identical to Svelte, with a few exceptions to match Rust convention.

#### Expressions and bindings
Using state variables is the same:

```svelte
<p>The button was pressed {counter} times.</p>
```

Internally, the counter variable will be immutably borrowed and formatted into text using Rust's `format!()` macro. Because of this, variables which you want to display must implement the standard `Display` trait, as most common traits do.

Rsvelte does not support Javascript-like ternary operators, but any standard Rust expression can be used inside an HTML format block, like so:

```svelte
<p>The button was pressed {counter + 1} {if counter == 1 { "time" } else { "times" } }.</p>
```

Like Svelte, you can use actions to trigger functions and bindings to set values, like so:
```svelte
<button onclick={increment}>Click me!</button>
<input type="number" bind:value={counter} />
```

However, that version is actually syntax sugar and is converted before compilation to the more complete version (which you can also use for more control):
```svelte
<button onclick={increment()}>Click me!</button>
<input type="number" bind:value={|| counter, |v| counter = v;}>
```

Notice that the expressions use Rust syntax for closures (`|| {}`) rather than JavaScript syntax (`() => {}`).

#### Loops and conditionals

For loops use the following format:

```svelte
{#each list as item}
    <p>{item}</p>
{/each}
```

Like Rust for loops, you can deconstruct complex list items inside the loop:

```svelte
{#each list as (item_a, item_b, item_c)}
    <ul>
        <li>{item_a}</li>
        <li>{item_b}</li>
        <li>{item_c}</li>
    </ul>
{/each}
```

Each blocks at the item for each iteration to the local scope and can be used in any nested blocks.

If statements use this format, and can accept any Rust expression that evaluates to a boolean:

```svelte
{#if counter > 10}
    <p>The button was pressed more than 10 times!</p>
{:else if counter > 5}
    <p>The button was pressed more than 5 times!</p>
{:else}
    <p>The button was pressed {counter} times.</p>
{/if}
```

#### Snippets

Snippets can be defined and called like functions which return HTML blocks and accept arguments. Defining a snippet is done like so, and can use any locally scoped or state variables:

```svelte
<#snippet my_snip(a: u64)>
    <p>Pressed {counter}/{a} times.</p>
</snippet>
```

Note that the argument type for snippets must be defined.

A snippet definition can be used anywhere, even in other components (see the next sections). Any locally scoped or state variable, or expression using these variables, can be used as the argument to pass in to the snippet render call:

```svelte
{@render my_snip(counter_max + 1)}
```

 > Note: snippets can be used like regular state variables most places, but CANNOT be bindable props

#### Components

Components are imported in the script section using `use` statements:
`use $components::MyButton`

And then called using an uppercase tag with any props they have:

```svelte
<div>
    <MyButton max_counter={10} bind:bindable_prop={my_var}>
</div>
```

### Project structure

Like Svelte, RSvelte projects have a `src` directory where all `.rsvelte` component files are stored. It also has a built-in filesystem based router, which means that the file structure of the `src` directory determines the routes of the application. For example, a file at `src/pages/about/+page.rsvelte` would be accessible at the `/about` route. At minimum, a project must have a `src/routes/+page.rsvelte` and `src/routes/+layout.rsvelte` file, which serve as the root page and layout components, respectively.

You can also create nested routes by creating subdirectories within `src/routes`. For example, a file at `src/routes/blog/[slug]/+page.rsvelte` would be accessible at the `/blog/:slug` route, where `:slug` is a dynamic parameter that can be accessed within the component.

For components that are not pages (i.e., they are not directly associated with a route), you can create a `src/components` directory to store them. These components can then be imported and used within your page components as needed.

An example project structure might look like this:

```
src/
├── routes/ 
│   ├── +layout.rsvelte
│   ├── +page.rsvelte
│   ├── about/
│   │   ├── +page.rsvelte
│   │   └── team.rsvelte
│   └── blog/
│       ├── +page.rsvelte
│       └── [slug]/
│           └── +page.rsvelte
├── components/
│   ├── Header.rsvelte
│   └── Footer.rsvelte
├── app.html
└── lib.rs
Cargo.toml
```

## Development

To test the compiler for development, you can run one of the examples, like so:
```bash
cargo run --example show_output
```

You'll probably want to enable logging to see the compiler's progress:
```bash
RUST_LOG=info cargo run --example show_output
```