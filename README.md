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

TODO: some stuff here

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