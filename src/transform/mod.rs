use syn::Token;

use crate::{
    EVENTS,
    code_gen::CodeGenContext,
    parse::{self, ComponentAST, html_parse::AttrType},
    transform::{self, node::Node},
    utils::{CompileError, generic_error},
};

mod attr;
mod each_block;
mod expr;
mod if_block;
mod node;
mod patches;
mod utils;

pub struct EventHandler {
    pub event_type: syn::Type,
    pub js_event_type: String,
    pub target_id: u32,
    pub closure: syn::ExprCall,
    pub dirty_flags: u32,
}

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
}

fn transform_component(
    comp: ComponentAST,
    components: &Vec<ComponentAST>,
) -> Result<CodeGenContext, CompileError> {
    // Check that event attributes have matching functions,
    // and collect event handlers
    let html_events = comp.body.get_events();
    let mut event_handlers = Vec::new();
    let mut script = if let Some(script) = comp.script {
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

    // Transform html AST to Node tree
    let mut tuple_idx_counter = 0;
    let reactive_vars: Vec<ReactiveVar> = script
        .props
        .iter()
        .cloned()
        .map(Into::into)
        .chain(script.bindable_props.iter().cloned().map(Into::into))
        .chain(script.state_vars.iter().cloned().map(Into::into))
        .chain(script.derived_vars.iter().cloned().map(Into::into))
        .collect();
    let state_funcs = script
        .state_functions
        .iter()
        .map(|func| func.sig.ident)
        .collect();
    let node_tree = Node::from_element(
        comp.body,
        &mut tuple_idx_counter,
        &reactive_vars,
        &state_funcs,
    );

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

    Ok(CodeGenContext {
        event_handlers,
        body: ast.body,
        script_data: script,
        style: ast.style,
        patch_generators,
    })
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
}

pub fn add_reactive_vars_to_func(
    func: &mut FuncData,
    reactive_vars: &Vec<ReactiveVar>,
) {
    for rv in func.reactive_args.iter() {
        let reactive_var = reactive_vars
            .iter()
            .find(|r| &r.var.name == rv)
            .expect(&format!(
                "Reactive variable '{}' not found for function '{}'",
                rv, func.code.sig.ident
            ));
        if &reactive_var.var.name == rv {
            // create &mut version of type
            let mut_ty = syn::Type::Reference(syn::TypeReference {
                and_token: Token![&](proc_macro2::Span::call_site()),
                lifetime: None,
                mutability: Some(Token![mut](proc_macro2::Span::call_site())),
                elem: Box::new(reactive_var.var.ty.clone()),
            });

            let arg = syn::FnArg::Typed(syn::PatType {
                attrs: Vec::new(),
                pat: Box::new(syn::Pat::Ident(syn::PatIdent {
                    attrs: Vec::new(),
                    by_ref: None,
                    mutability: None,
                    ident: reactive_var.var.name.clone(),
                    subpat: None,
                })),
                colon_token: Token![:](proc_macro2::Span::call_site()),
                ty: Box::new(mut_ty),
            });
            func.code.sig.inputs.push(arg);
        }
    }
}
