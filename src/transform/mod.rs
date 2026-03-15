use std::collections::HashMap;

use quote::format_ident;
use syn::Ident;

pub use crate::transform::derived::DerivedVar;
pub use crate::transform::node::{
    Node, NodeElseBranch, NodeIfBranch, NodeType, TagAttribute,
};
use crate::transform::state::get_state_code_getter;
use crate::{
    code_gen::CodeGenContext,
    parse::{self, ComponentAST},
    transform::func::{transform_func, validate_event_handler_args},
    utils::{CompileError, generic_error},
};

mod derived;
mod expr;
mod func;
mod node;
mod state;

pub struct ReactiveVar {
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

    let root_comp = components.remove(0);
    let root_context = transform_component(root_comp, &components)?;
    let mut comp_contexts = Vec::new();
    for _ in 0..components.len() {
        let comp = components.remove(0);
        comp_contexts.push(transform_component(comp, &components)?);
    }

    Ok(CodeGenContext {
        root_comp: root_context,
        comps: comp_contexts,
    })
}

pub struct CompContext {
    pub comp_id: String,
    pub root_node: Node,
    pub state_type: syn::Ident,
    pub state_code_getter: Box<dyn Fn(&Ident) -> proc_macro2::TokenStream>,
    //pub state_vars (props, bindables, reactive, derived)

    //pub children_state (top level)
    //pub element/fragment_state

    //pub mount_code
    pub state_funcs: Vec<syn::ItemFn>,
    pub agnostic_code: Vec<proc_macro2::TokenStream>,

    //pub bind_handlers
    pub derived_handlers: Vec<DerivedVar>, //pub child_propagators

                                           //pub html_body
                                           //pub styles: Option<String>,
}

fn transform_component(
    comp: ComponentAST,
    components: &Vec<ComponentAST>,
) -> Result<CompContext, CompileError> {
    // Check that event attributes have matching functions,
    // and collect event handlers
    log::info!("{:?}", comp.body);
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
        script.state_vars.iter().cloned().map(Into::into).collect();
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
    let mut frag_field_idx_counter = 0;
    let state_funcs = script
        .state_functions
        .iter()
        .map(|func| &func.sig.ident)
        .collect();
    let node_tree = Node::from_element(
        comp.body,
        &mut frag_field_idx_counter,
        &state_vars,
        &reactive_vars,
        &state_funcs,
        &component_map,
        &comp.id_hash,
    );

    // Build reactive var data & dependency graph for derived and bindables
    let derived_handlers = derived::build_derived_order(
        script.derived_vars.clone(),
        &state_vars,
        &reactive_vars,
    );

    let state_code_getter = get_state_code_getter(&script, &derived_handlers);

    let mut state_funcs = Vec::new();

    // Find and transform event handlers (functions, callers, and closures), validating their arguments
    for func in script.state_functions {
        let new_func = transform_func(func, &reactive_vars);
        if !validate_event_handler_args(&new_func) {
            return Err(generic_error(&format!(
                "Event handler '{}' has invalid arguments. Event handlers must have a `&mut self` argument and optionally a single event argument of type `&Event` or `&MouseEvent` etc.",
                new_func.sig.ident
            )));
        }
        state_funcs.push(new_func);
    }

    // Create list of child component state to store in the component struct

    // For child components with bindable props, add bind handlers

    // Add child component prop updaters & downward propagation

    // Scope CSS styles to the component

    // TODO: overhaul importing system to use Rust syntax

    let state_type = format_ident!("C{}State", comp.id_hash);

    Ok(CompContext {
        comp_id: comp.id_hash,
        root_node: node_tree,
        state_type,
        derived_handlers,
        state_funcs,
        state_code_getter,
        agnostic_code: script.agnostic_code,
    })
}
