use syn::{Ident, Token, parse::ParseStream, spanned::Spanned};
use std::fmt::Debug;

use super::parse_to;

#[derive(Clone)]
pub struct StateVar {
    pub name: Ident,
    pub ty: syn::Type,
    pub default: syn::Expr,
    mutable: bool,
}

impl Debug for StateVar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "StateVar {{ name: {}, ty: {}, default: {}, mutable: {} }}",
            self.name,
            quote::ToTokens::to_token_stream(&self.ty).to_string(),
            quote::ToTokens::to_token_stream(&self.default).to_string(),
            self.mutable
        )
    }
}

#[derive(Debug, Clone)]
pub struct ReactiveVar {
    pub var: StateVar,
    pub flag_pos: u8,
}

/// Parse variable declarations from the input stream
pub fn gen_vars(input: &mut ParseStream, reactive_vars: &mut Vec<ReactiveVar>, non_reactive_vars: &mut Vec<StateVar>) -> syn::Result<()> {
    // Parse variable declaration
    let mutable = input.parse::<Token![mut]>().is_ok();
    let name: Ident = input.parse()?;

    let mut ty = None;
    if input.peek(Token![:]) {
        let _colon: Token![:] = input.parse()?;
        ty = Some(input.parse()?);
    }

    input.parse::<Token![=]>()?;
    if input.peek(Token![$]) {
        // Reactive variable
        input.parse::<Token![$]>()?;
        let state_ident: Ident = input.parse()?;
        if state_ident != "state" {
            return Err(syn::Error::new(
                state_ident.span(),
                "Expected 'state' after '$'",
            ));
        }

        if !mutable {
            return Err(syn::Error::new(
                name.span(),
                "Reactive state variables must be mutable",
            ));
        }

        let content;
        syn::parenthesized!(content in input);

        let mut default_expr: syn::Expr = content.parse()?;
        let ty = if let Some(ty) = ty {
            ty
        } else {
            // Infer type from default expression
            ty = infer_type_from_expr(&default_expr);
            if let Some(inferred_ty) = ty {
                if ("String" == quote::quote! { #inferred_ty }.to_string())
                    && !matches!(default_expr, syn::Expr::Lit(_))
                {
                    // Wrap in to_string() call
                    default_expr = syn::parse2(quote::quote! {
                        #default_expr.to_string()
                    })
                    .unwrap();
                }
                inferred_ty
            } else {
                return Err(syn::Error::new(
                    default_expr.span(),
                    "Could not infer type for reactive state variable; please specify type explicitly",
                ));
            }
        };

        let state_var = StateVar {
            name: name.clone(),
            ty,
            default: default_expr,
            mutable,
        };

        let flag_pos = reactive_vars.len() as u8;
        let reactive_var = ReactiveVar {
            var: state_var,
            flag_pos,
        };
        reactive_vars.push(reactive_var);
    } else {
        // Non-reactive variable
        let default_expr: syn::Expr = input.parse()?;
        if let None = ty {
            // Infer type from default expression
            ty = infer_type_from_expr(&default_expr);
        }

        let state_var = StateVar {
            name: name.clone(),
            ty: ty.unwrap_or_else(|| syn::parse_str("_").unwrap()),
            default: default_expr,
            mutable,
        };

        non_reactive_vars.push(state_var);
    }
    let _ = parse_to(input, Token![;], true)?;

    Ok(())
}

/// Infer the type of a variable from its default expression
fn infer_type_from_expr(expr: &syn::Expr) -> Option<syn::Type> {
    match expr {
        // Integer literals -> i32 (Rust's default integer type)
        syn::Expr::Lit(expr_lit) => match &expr_lit.lit {
            syn::Lit::Int(lit_int) => {
                // Check if suffix is specified (e.g., 0u8, 0i64)
                if !lit_int.suffix().is_empty() {
                    syn::parse_str(lit_int.suffix()).ok()
                } else {
                    Some(syn::parse_str("i32").unwrap())
                }
            }
            syn::Lit::Float(lit_float) => {
                if !lit_float.suffix().is_empty() {
                    syn::parse_str(lit_float.suffix()).ok()
                } else {
                    Some(syn::parse_str("f64").unwrap())
                }
            }
            syn::Lit::Str(_) => Some(syn::parse_str("String").unwrap()),
            syn::Lit::Bool(_) => Some(syn::parse_str("bool").unwrap()),
            syn::Lit::Char(_) => Some(syn::parse_str("char").unwrap()),
            syn::Lit::Byte(_) => Some(syn::parse_str("u8").unwrap()),
            syn::Lit::ByteStr(_) => Some(syn::parse_str("Vec<u8>").unwrap()),
            _ => None,
        },
        // String macro (e.g., String::from("...") or "...".to_string())
        syn::Expr::MethodCall(method_call) => {
            if method_call.method == "to_string" {
                Some(syn::parse_str("String").unwrap())
            } else {
                None
            }
        }
        // Vec macro or Vec::new()
        syn::Expr::Macro(expr_macro) => {
            let path = &expr_macro.mac.path;
            if path.is_ident("vec") {
                // For vec![], we can't easily infer the element type
                // Return Vec<_> and let Rust infer it
                Some(syn::parse_str("Vec<_>").unwrap())
            } else {
                None
            }
        }
        // Array literals
        syn::Expr::Array(expr_array) => {
            let len = expr_array.elems.len();
            if let Some(first) = expr_array.elems.first() {
                if let Some(elem_type) = infer_type_from_expr(first) {
                    let type_str = format!("[{}; {}]", quote::quote!(#elem_type), len);
                    return syn::parse_str(&type_str).ok();
                }
            }
            None
        }
        // Tuple literals
        syn::Expr::Tuple(expr_tuple) => {
            let types: Vec<_> = expr_tuple
                .elems
                .iter()
                .filter_map(infer_type_from_expr)
                .collect();
            if types.len() == expr_tuple.elems.len() {
                let type_str = format!(
                    "({})",
                    types
                        .iter()
                        .map(|t| quote::quote!(#t).to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                return syn::parse_str(&type_str).ok();
            }
            None
        }
        // Reference expressions
        syn::Expr::Reference(expr_ref) => {
            if let Some(inner_type) = infer_type_from_expr(&expr_ref.expr) {
                let mutability = if expr_ref.mutability.is_some() {
                    "mut "
                } else {
                    ""
                };
                let type_str = format!("&{}{}", mutability, quote::quote!(#inner_type));
                return syn::parse_str(&type_str).ok();
            }
            None
        }
        _ => None,
    }
}
