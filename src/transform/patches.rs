use syn::{parse::Parser, parse2};

use crate::parse::{ReactiveVar, StateVar, html_parse};
use crate::EVENTS;

pub enum PatchOp {
    SetContent,
    SetAttribute { name: String },
}

pub struct Patch {
    pub flag_mask: u32,
    pub target_id: u32,
    pub expr: syn::Expr,
    pub operation: PatchOp,
    pub vars: Vec<StateVar>,
}

pub fn extract_generators(
    ast: &html_parse::Element,
    state_vars: &Vec<StateVar>,
    reactive_vars: &Vec<ReactiveVar>,
) -> Vec<Patch> {
    let mut patches = Vec::new();

    let all_vars: Vec<StateVar> = reactive_vars
        .iter()
        .map(|rv| rv.var.clone())
        .chain(state_vars.iter().cloned())
        .collect();
    ast.tag.attributes.iter().for_each(|(attr_name, attr_val)| {
        if EVENTS
            .iter()
            .any(|(event, _, _)| *event == attr_name.as_str())
        {
            // Skip event attributes
            return;
        }
        if let html_parse::AttrType::Expr(expr) = attr_val {
            // Get all state vars used in this expression
            let used_vars = get_vars_in_expr(expr, &all_vars, |v| &v.name);
            let dirty_flags = get_vars_in_expr(expr, &reactive_vars, |v| &v.var.name)
                .iter()
                .fold(0u32, |acc, rv| acc | (1 << rv.flag_pos));
            log::info!(
                "Generating attribute expression for node id {}: {}, {} reactive vars",
                ast.id,
                quote::quote! { #expr },
                used_vars.len()
            );

            patches.push(Patch {
                flag_mask: dirty_flags,
                target_id: ast.id,
                expr: expr.clone(),
                operation: PatchOp::SetAttribute {
                    name: attr_name.clone(),
                },
                vars: used_vars,
            })
        } else if let html_parse::AttrType::Closure(_) = attr_val {
            panic!("Non-event attribute closures not allowed");
        }
    });

    match &ast.contents {
        html_parse::ContentType::Text(text, exprs) => {
            if !exprs.is_empty() {
                let exprs = exprs;

                let used_vars = exprs
                    .iter()
                    .flat_map(|expr| get_vars_in_expr(expr, &all_vars, |v| &v.name))
                    .collect::<Vec<StateVar>>();
                let dirty_flags = exprs
                    .iter()
                    .flat_map(|expr| get_vars_in_expr(expr, &reactive_vars, |v| &v.var.name))
                    .fold(0u32, |acc, rv| acc | (1 << rv.flag_pos));

                // Create patch generator using expressions
                let str_expr = parse2(quote::quote! {
                    format!(#text, #(#exprs),*)
                })
                .unwrap();

                patches.push(Patch {
                    flag_mask: dirty_flags,
                    target_id: ast.id,
                    expr: str_expr,
                    operation: PatchOp::SetContent,
                    vars: used_vars,
                })
            }
        }
        html_parse::ContentType::Elem(children) => {
            for child in children {
                let mut child_patches = extract_generators(child, state_vars, reactive_vars);
                patches.append(&mut child_patches);
            }
        }
        _ => {}
    }

    patches
}

/// Get vars in expression using visitor
fn get_vars_in_expr<T: Clone, U: Fn(&T) -> &syn::Ident>(
    expr: &syn::Expr,
    vars: &Vec<T>,
    get_ident: U,
) -> Vec<T> {
    struct VarVisitor<'a, T, U> {
        vars: &'a Vec<T>,
        found_vars: Vec<T>,
        get_ident: U,
    }

    impl<'a, 'ast, T: Clone, U: Fn(&T) -> &syn::Ident> syn::visit::Visit<'ast>
        for VarVisitor<'a, T, U>
    {
        fn visit_ident(&mut self, ident: &'ast syn::Ident) {
            for var in self.vars.iter() {
                if (self.get_ident)(var) == ident {
                    self.found_vars.push(var.clone());
                }
            }
            syn::visit::visit_ident(self, ident);
        }

        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            // Try to parse macro tokens as comma-separated expressions
            let tokens = mac.tokens.clone();

            // Parse as punctuated expressions (handles format!("...", a, b, c))
            if let Ok(args) = syn::parse2::<MacroArgs>(tokens.clone()) {
                for expr in args.exprs {
                    syn::visit::visit_expr(self, &expr);
                }
            } else {
                // Fallback: try to parse individual expressions from the token stream
                // This handles cases where the macro args aren't standard
                let parser =
                    syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
                if let Ok(exprs) = parser.parse2(tokens) {
                    for expr in exprs.iter() {
                        syn::visit::visit_expr(self, expr);
                    }
                }
            }

            syn::visit::visit_macro(self, mac);
        }
    }

    let mut visitor = VarVisitor {
        vars,
        found_vars: Vec::new(),
        get_ident,
    };
    syn::visit::visit_expr(&mut visitor, expr);
    visitor.found_vars
}

/// Helper struct to parse macro arguments
struct MacroArgs {
    exprs: Vec<syn::Expr>,
}

impl syn::parse::Parse for MacroArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut exprs = Vec::new();
        
        // Skip the format string if present (for format!, println!, etc.)
        if input.peek(syn::LitStr) {
            let _: syn::LitStr = input.parse()?;
            if input.peek(syn::Token![,]) {
                let _: syn::Token![,] = input.parse()?;
            }
        }
        
        // Parse remaining expressions
        while !input.is_empty() {
            let expr: syn::Expr = input.parse()?;
            exprs.push(expr);
            
            if input.peek(syn::Token![,]) {
                let _: syn::Token![,] = input.parse()?;
            }
        }
        
        Ok(MacroArgs { exprs })
    }
}
