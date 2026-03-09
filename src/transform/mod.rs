use std::collections::HashMap;

use syn::Token;

pub use crate::transform::derived::DerivedVar;
pub use crate::transform::node::Node;
use crate::{
    code_gen::CodeGenContext, parse::{self, ComponentAST, html_parse::AttrType}, transform::func::{transform_func, validate_event_handler_args}, utils::{CompileError, generic_error}
};

mod derived;
mod expr;
mod node;
mod patches;
mod event;
mod func;

struct ReactiveVar {
    name: syn::Ident,
    ty: syn::Type,
    flag_mask: u64,
}

impl Into<ReactiveVar> for parse::StateVar {
    fn into(self) -> ReactiveVar {
        ReactiveVar {
            name: self.name,
            ty: self.ty,
            flag_mask: 1 << self.flag_pos,
        }
    }
}

impl Into<ReactiveVar> for parse::Prop {
    fn into(self) -> ReactiveVar {
        ReactiveVar {
            name: syn::Ident::new(&self.name, proc_macro2::Span::call_site()),
            ty: self.ty,
            flag_mask: 1 << self.flag_pos,
        }
    }
}

pub fn transform(
    mut components: Vec<ComponentAST>,
) -> Result<CodeGenContext, CompileError> {
    // For now, just transform the first component (assumed to be the page)
    if components.is_empty() {
        return Err(generic_error("No components found in project"));
    }
    let page_component = components.remove(0);
    transform_component(page_component, &components)
    // TODO: also transform child components and store their CodeGenContexts in the parent context for code generation
}

fn transform_component(
    comp: ComponentAST,
    components: &Vec<ComponentAST>,
) -> Result<CodeGenContext, CompileError> {
    // Check that event attributes have matching functions,
    // and collect event handlers
    let html_events = comp.body.get_events();
    let script = if let Some(script) = comp.script {
        script
    } else if !html_events.is_empty() {
        return Err(generic_error(
            "Event attributes found in HTML but no <script> section present",
        ));
    } else {
        parse::ScriptData {
            props: Vec::new(),
            bindable_props: Vec::new(),
            state_vars: Vec::new(),
            derived_vars: Vec::new(),
            init_func: None,
            state_functions: Vec::new(),
            imports: Vec::new(),
            agnostic_code: Vec::new(),
        }
    };

    // Combine all reactive variables into a single list for easy lookup during transformation
    let state_vars: Vec<ReactiveVar> =
        script.state_vars.into_iter().map(Into::into).collect();
    let reactive_vars: Vec<ReactiveVar> = script
        .props
        .iter()
        .cloned()
        .map(Into::into)
        .chain(script.bindable_props.iter().cloned().map(Into::into))
        .chain(script.derived_vars.iter().cloned().map(Into::into))
        .collect();

    // Create map from component imports to their ASTs for easy lookup during transformation
    let mut component_map = HashMap::new();
    for comp in components {
        if let Some(import) = script
            .imports
            .iter()
            .find(|import| import.path == comp.source_path)
        {
            component_map.insert(&import.name, comp);
        } else {
            return Err(generic_error(&format!(
                "Component '{}' imported but not found in project",
                comp.source_path
            )));
        }
    }

    // Transform html AST to Node tree
    let mut tuple_idx_counter = 0;
    let state_funcs = script
        .state_functions
        .iter()
        .map(|func| &func.sig.ident)
        .collect();
    let node_tree = Node::from_element(
        comp.body,
        &mut tuple_idx_counter,
        &state_vars,
        &reactive_vars,
        &state_funcs,
        &component_map
    );

    // Build reactive var data & dependency graph for derived and bindables
    let derived_handlers = derived::build_derived_order(
        script.derived_vars.clone(),
        &state_vars,
        &reactive_vars,
    );

    // Transform init code
    let init_code = if let Some(init_func) = script.init_func {
        let init_func = transform_func(init_func, &reactive_vars);
        quote::quote! {
            #init_func
        }
    } else {
        // Add empty fn if not user provided
        quote::quote! {
            fn init(&mut self) {}
        }
    };
    let mut state_funcs = vec![init_code];

    // Find and transform event handlers (functions, callers, and closures), validating their arguments
    for func in script.state_functions {
        let new_func = transform_func(func, &reactive_vars);
        if !validate_event_handler_args(&new_func) {
            return Err(generic_error(&format!(
                "Event handler '{}' has invalid arguments. Event handlers must have a `&mut self` argument and optionally a single event argument of type `&Event` or `&MouseEvent` etc.",
                new_func.sig.ident
            )));
        }
        state_funcs.push(quote::quote! { #new_func });
    }
    // Generate event handling branches
    // event type, target id, function or closure call

    // Create list of child component state to store in the component struct

    // For child components with bindable props, add bind handlers

    // Add child component prop updaters & downward propagation

    // Scope CSS styles to the component

    // TODO: overhaul importing system to use Rust syntax

    Ok(CodeGenContext {
        node_tree,
        derived_handlers,
        state_funcs,
        agnostic_code: script.agnostic_code,
    })
}

/*
log::info!("Generating event handlers...");
for (target_id, event_name, attr) in html_events {
    let mut attr = attr.clone();
    transform::find_function_calls(&mut attr, &script.functions);

    let (closure, dirty_flags) =
        parse::gen_expr(&attr, &script.functions, &script.reactive_vars)?;

    let event_type = EVENTS
        .iter()
        .find(|(name, _, _)| *name == *event_name)
        .expect(&format!(
            "Event '{}' not found in EVENTS list",
            event_name
        ));
    event_handlers.push(EventHandler {
        event_type: syn::parse_str(event_type.1).unwrap(),
        js_event_type: event_type.2.to_string(),
        target_id: target_id,
        closure,
        dirty_flags,
    });
}

log::info!("Extracting patch generators...");
// Traverse body to find patch generators
let patch_generators = patches::extract_generators(
    &ast.body,
    &script.non_reactive_vars,
    &script.reactive_vars,
);

let mod_paths = Vec::new();
for import in &script.imports {
    if let syn::Item::Use(item_use) =
        syn::parse2(import.clone()).map_err(|e| {
            generic_error(&format!(
                "Failed to parse import statement: {}",
                e
            ))
        })?
    {
        for segment in item_use.tree.into_iter() {
            if let syn::UseTree::Path(use_path) = segment {
                mod_paths.push(use_path.ident.clone().into());
            }
        }
    }
}

pub fn find_function_calls(attr: &mut AttrType, functions: &Vec<FuncData>) {
    if let AttrType::Expr(expr) = attr.clone()
        && let syn::Expr::Path(path) = expr
        && path.path.segments.len() == 1
    {
        let func_name = &path.path.segments[0].ident;
        for func in functions {
            if func_name == &func.code.sig.ident {
                // Replace with closure expression
                *attr = AttrType::Call(func_name.clone());
            }
        }
    }
}*/