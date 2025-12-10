use syn::Token;

use crate::{
    EVENTS,
    code_gen::CodeGenContext,
    parse::{self, ComponentAST, FuncData, ReactiveVar, html_parse::AttrType},
    transform,
    utils::{CompileError, generic_error},
};

mod patches;
pub use patches::{Patch, PatchOp};

pub struct EventHandler {
    pub event_type: syn::Type,
    pub js_event_type: String,
    pub target_id: u32,
    pub closure: syn::ExprCall,
    pub dirty_flags: u32,
}


pub fn transform(ast: ComponentAST) -> Result<CodeGenContext, CompileError> {
    // Check that event attributes have matching functions,
    // and collect event handlers
    let html_events = ast.body.get_events();
    let mut event_handlers = Vec::new();
    let mut script = if let Some(script) = ast.script {
        script
    } else if !html_events.is_empty() {
        return Err(generic_error(
            "Event attributes found in HTML but no <script> section present",
        ));
    } else {
        return Ok(CodeGenContext {
            event_handlers: Vec::new(),
            body: ast.body,
            script_data: parse::ScriptData {
                reactive_vars: Vec::new(),
                non_reactive_vars: Vec::new(),
                functions: Vec::new(),
                init_code: None,
                imports: Vec::new(),
            },
            style: ast.style,
            patch_generators: Vec::new(),
        });
    };

    log::info!("Adding reactive vars to functions...");
    // Add reactive vars as params to functions
    for func in &mut script.functions {
        add_reactive_vars_to_func(func, &script.reactive_vars);
    }

    log::info!("Generating event handlers...");
    for (target_id, event_name, attr) in html_events {
        let mut attr = attr.clone();
        transform::find_function_calls(&mut attr, &script.functions);

        let (closure, dirty_flags) =
            parse::gen_expr(&attr, &script.functions, &script.reactive_vars)?;

        let event_type = EVENTS
            .iter()
            .find(|(name, _, _)| *name == *event_name)
            .expect(&format!("Event '{}' not found in EVENTS list", event_name));
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
    let patch_generators =
        patches::extract_generators(&ast.body, &script.non_reactive_vars, &script.reactive_vars);

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

pub fn add_reactive_vars_to_func(func: &mut FuncData, reactive_vars: &Vec<ReactiveVar>) {
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
