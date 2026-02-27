use crate::{
    CompileOutput,
    parse::{
        StateVar,
        html_parse::{self, AttrType},
    },
    transform::{EventHandler, PatchOp},
    utils::CompileError,
};

pub enum ElementArrayItem {
    Element,
    If(Vec<ElementArrayItem>),
    Each(Vec<ElementArrayItem>),
}

pub struct CodeGenContext {
    pub element_array: Vec<ElementArrayItem>,
    //pub state_vars (props, bindables, reactive, derived)
    
    //pub children_state (top level)
    //pub fragment_state
    
    //pub mount_code
    //pub init_code
    pub agnostic_code: Vec<proc_macro2::TokenStream>,

    //pub event_handlers
    //pub bind_handlers
    //pub derived_handlers
    //pub patch_generators
    //pub child_propagators

    //pub html_body
    //pub styles
}

pub fn code_gen(context: CodeGenContext) -> Result<CompileOutput, CompileError> {
    // Parse context for state.rs generation
    let imports = context.script_data.imports;

    let init_code = if let Some(code) = context.script_data.init_code {
        code
    } else {
        quote::quote! {}
    };

    // Closure to generatic static variables from reactive and non-reactive vars
    let gen_static = |var: &StateVar| {
        let name = &var.name;
        let all_caps_name = syn::Ident::new(&name.to_string().to_uppercase(), name.span());
        let ty = &var.ty;
        let default = &var.default;
        quote::quote! {
            static ref #all_caps_name: Mutex<#ty> = Mutex::new(#default);
        }
    };

    let reactive_vars: Vec<proc_macro2::TokenStream> = context
        .script_data
        .reactive_vars
        .iter()
        .map(|var| gen_static(&var.var))
        .collect();

    let non_reactive_vars: Vec<proc_macro2::TokenStream> = context
        .script_data
        .non_reactive_vars
        .iter()
        .map(gen_static)
        .collect();

    let functions = context
        .script_data
        .functions
        .iter()
        .map(|func| func.code.clone());

    let event_branches = context.event_handlers.iter().map(|handler| {
        let event_name = &handler.js_event_type;
        let target_id = handler.target_id;
        let event_type = &handler.event_type;
        let closure_call = &handler.closure;
        let dirty_flags = handler.dirty_flags;
        quote::quote! {
            #event_name if target == #target_id => {
                let e = e.dyn_into::<#event_type>().unwrap();
                #closure_call;
                DIRTY_FLAGS.fetch_or(#dirty_flags, SeqCst);
            }
        }
    });

    let patch_generators = context.patch_generators.iter().map(|patch| {
        let flag_mask = patch.flag_mask;
        let target_id = patch.target_id;
        let expr = patch.expr.clone();

        let var_locks = patch.vars.iter().map(|var| {
            let var_name = &var.name;
            let var_name_caps =
                syn::Ident::new(&var.name.to_string().to_uppercase(), var.name.span());
            quote::quote! {
                let #var_name = #var_name_caps.lock().unwrap();
            }
        });

        let inner = match &patch.operation {
            PatchOp::SetContent => {
                quote::quote! {
                    PatchOp::SetContent { value: #expr },
                }
            }
            PatchOp::SetAttribute { name } => {
                quote::quote! {
                    PatchOp::SetAttribute {
                        name: #name.to_string(),
                        value: #expr,
                    },
                }
            }
        };

        quote::quote! {
            if DIRTY_FLAGS.fetch_and(#flag_mask, SeqCst) != 0 {
                #(#var_locks)*
                patches.push(Patch {
                    target_id: #target_id,
                    operation: #inner
                });
            }
        }
    });

    // Generate state.rs as tokens (to ensure valid syntax)
    let state_rs_tokens = quote::quote! {
        use std::{sync::{Mutex}};
        use std::sync::atomic::Ordering::SeqCst;
        use wasm_bindgen::JsCast;
        use crate::{Patch, PatchOp, DIRTY_FLAGS};

        #(#imports)*

        // Todo: Struct definitions & impl
        lazy_static::lazy_static! {
            #(#reactive_vars)*

            #(#non_reactive_vars)*
        }

        pub fn init() {
            #init_code
        }

        #(#functions)*

        pub fn affect_state(e: web_sys::Event, target: u32) {
            match e.type_().as_str() {
                #(
                    #event_branches
                )*
                _ => {}
            }
        }

        pub fn apply_state() -> Vec<Patch> {
            let mut patches = Vec::new();
            #(
                #patch_generators
            )*
            patches
        }
    };

    let state_rs_str = state_rs_tokens.to_string();

    // Parse context for startup.js generation
    let node_map_builder = gen_node_map_builder(&context.body);
    let event_listeners = gen_event_listeners(context.event_handlers);

    let startup_js_str = format!(
        "import {{ handle_js_event }} from './patches.js';
            export function buildNodeMap() {{
                let nodeMap = [];
                {}
                document.head.querySelector('style').textContent = `{}`;
                return nodeMap;
            }}
            export function setupEventListeners(nodeMap) {{
                {}
            }}",
        node_map_builder,
        context.style.unwrap_or_default(),
        event_listeners,
    );

    Ok(CompileOutput {
        state_rs: state_rs_str,
        startup_js: startup_js_str,
    })
}

pub fn gen_node_map_builder(ast: &html_parse::Element) -> String {
    let mut builder = String::new();
    builder.push_str(&format!(
        "nodeMap[{}] = document.createElement('{}');\n",
        ast.id, ast.tag.name
    ));
    for (attr_name, attr_val) in &ast.tag.attributes {
        log::info!("Generating attribute for node id {}: {}", ast.id, attr_name);
        if let AttrType::Str(attr_str) = attr_val {
            builder.push_str(&format!(
                "nodeMap[{}].setAttribute('{}', `{}`);\n",
                ast.id, attr_name, attr_str
            ));
            continue;
        }
    }

    match &ast.contents {
        html_parse::ContentType::Elem(children) => {
            for child in children {
                builder.push_str(&gen_node_map_builder(child));
                builder.push_str(&format!(
                    "nodeMap[{}].appendChild(nodeMap[{}]);\n",
                    ast.id, child.id
                ));
            }
        }
        html_parse::ContentType::Text(txt, exprs) => {
            if exprs.is_empty() {
                builder.push_str(&format!("nodeMap[{}].textContent = `{}`;\n", ast.id, txt));
            }
        }
        _ => {}
    }
    builder
}

pub fn gen_event_listeners(events: Vec<EventHandler>) -> String {
    let mut builder = String::new();
    for event in events {
        builder.push_str(&format!(
            "nodeMap[{}].addEventListener('{}', (e) => handle_js_event(e, nodeMap, {}));\n",
            event.target_id, event.js_event_type, event.target_id
        ));
    }
    builder
}
