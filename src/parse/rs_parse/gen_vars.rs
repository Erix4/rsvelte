use std::fmt::Debug;
use syn::{
    Ident, Token,
    parse::{ParseBuffer, ParseStream},
    spanned::Spanned,
};

use crate::parse::ScriptData;

#[derive(Clone)]
pub struct Prop {
    pub name: String,
    pub ty: syn::Type,
    pub default: Option<syn::Expr>,
    pub flag_pos: u8,
}

impl Debug for Prop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Prop {{ name: {}, ty: {}, default: {:?}, flag_pos: {} }}",
            self.name,
            quote::ToTokens::to_token_stream(&self.ty).to_string(),
            self.default
                .as_ref()
                .map(|d| quote::ToTokens::to_token_stream(d).to_string()),
            self.flag_pos
        )
    }
}

#[derive(Clone)]
pub struct StateVar {
    pub name: Ident,
    pub ty: syn::Type,
    pub default: syn::Expr,
    pub flag_pos: u8,
}

impl Debug for StateVar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "StateVar {{ name: {}, ty: {}, default: {}, flag_pos: {} }}",
            self.name,
            quote::ToTokens::to_token_stream(&self.ty).to_string(),
            quote::ToTokens::to_token_stream(&self.default).to_string(),
            self.flag_pos
        )
    }
}

fn parse_var(
    input: &mut ParseBuffer,
    script_data: &mut ScriptData,
    flag_pos: &mut u8,
) -> syn::Result<()> {
    let name: Ident = input.parse()?;

    let mut ty = None;
    if input.peek(Token![:]) {
        let _colon: Token![:] = input.parse()?;
        ty = Some(input.parse()?);
    }

    input.parse::<Token![=]>()?;
    input.parse::<Token![$]>()?;

    let var_type_ident: Ident = input.parse()?;

    let paren_content;
    syn::parenthesized!(paren_content in input);

    let mut default_expr = None;
    if !paren_content.is_empty() {
        let paren_content: syn::Expr = paren_content.parse()?;

        let inferred_ty = infer_type_from_expr(&paren_content).ok_or(syn::Error::new(
            paren_content.span(),
            "Could not infer type for reactive state variable; please specify type explicitly",
        ))?;
        ty = Some(inferred_ty.clone());

        if ("String" == quote::quote! { #inferred_ty }.to_string())
            && !matches!(paren_content, syn::Expr::Lit(_))
        {
            // Wrap in to_string() call
            default_expr = Some(syn::parse2(quote::quote! {
                #paren_content.to_string()
            })?);
        } else {
            default_expr = Some(paren_content);
        }
    }

    let ty = ty.ok_or(syn::Error::new(
        name.span(),
        "Type annotation is required if no default value is provided",
    ))?;

    let expr_as_res = |e: Option<syn::Expr>| {
        e.ok_or(syn::Error::new(
            name.span(),
            "Default value is required for reactive state variables",
        ))
    };

    match var_type_ident.to_string().as_str() {
        "prop" => {
            let prop = Prop {
                name: name.to_string(),
                ty: ty,
                default: default_expr,
                flag_pos: *flag_pos,
            };
            script_data.props.push(prop);
        }
        "bindable" => {
            let prop = Prop {
                name: name.to_string(),
                ty: ty,
                default: default_expr,
                flag_pos: *flag_pos,
            };
            script_data.bindable_props.push(prop);
        }
        "state" => {
            let state_var = StateVar {
                name: name.clone(),
                ty: ty,
                default: expr_as_res(default_expr)?,
                flag_pos: *flag_pos,
            };
            script_data.state_vars.push(state_var);
        }
        "derived" => {
            let state_var = StateVar {
                name: name.clone(),
                ty: ty,
                default: expr_as_res(default_expr)?,
                flag_pos: *flag_pos,
            };
            script_data.derived_vars.push(state_var);
        }
        _ => {
            return Err(syn::Error::new(
                var_type_ident.span(),
                "Expected 'prop' or 'bindable' after '$'",
            ));
        }
    }

    // Consume comma if present
    input.parse::<Token![,]>().ok();

    *flag_pos += 1;

    Ok(())
}

/// Parse variable declarations from the input stream
pub fn gen_vars(
    input: &mut ParseStream,
    script_data: &mut ScriptData,
) -> syn::Result<()> {
    input.parse::<Token![struct]>()?; // Consume 'struct'
    input.parse::<Token![$]>()?; // Consume '$'

    // consume 'state' identifier
    let state_ident: Ident = input.parse()?;
    if state_ident != "state" {
        return Err(syn::Error::new(
            state_ident.span(),
            "Expected 'state' after '$'",
        ));
    }

    let mut content;
    syn::braced!(content in input);

    let mut flag_pos = 0;
    while !content.is_empty() {
        parse_var(&mut content, script_data, &mut flag_pos)?;
    }

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
                    let type_str =
                        format!("[{}; {}]", quote::quote!(#elem_type), len);
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
                let type_str =
                    format!("&{}{}", mutability, quote::quote!(#inner_type));
                return syn::parse_str(&type_str).ok();
            }
            None
        }
        _ => None,
    }
}
