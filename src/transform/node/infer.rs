use crate::transform::{ReactiveVar, node::EachVar};

/// The kind of type of the expression
pub enum ExprType {
    Vec,
    Iterable,
    Array,
}

/// Infer the full type of an iterable expression,
/// for example `Vec<i32>` from `(0..10).collect()`` or `Range<u64>`
/// from `0u64..100u64`. Where reactive or scoped variables are involved,
/// use their types to inform the inference. Panic if a type cannot be inferred,
/// and do not return placeholder types like `_`.
/// 
/// It can be assumed that the expression will either be an iterable, Vec, or array type,
/// and panic otherwise.
pub fn infer_each_expr_type(
    expr: &syn::Expr,
    reactive_vars: &[ReactiveVar],
    scoped: &Vec<EachVar>,
) -> syn::Type {
    match expr {
        // Parenthesized expression: unwrap and recurse
        syn::Expr::Paren(paren) => {
            infer_each_expr_type(&paren.expr, reactive_vars, scoped)
        }

        // Group expression: unwrap and recurse
        syn::Expr::Group(group) => {
            infer_each_expr_type(&group.expr, reactive_vars, scoped)
        }

        // Range expression (e.g. 0..10, 0u64..100u64) -> Range<T>
        syn::Expr::Range(range) => {
            let bound_type = range
                .start
                .as_ref()
                .or(range.end.as_ref())
                .map(|e| infer_scalar_type(e, reactive_vars, scoped))
                .unwrap_or_else(|| panic!("Cannot infer type of range with no bounds"));
            syn::parse_str(&format!("std::ops::Range<{}>", quote::quote!(#bound_type)))
                .expect("Failed to parse Range type")
        }

        // Method call (e.g. (0..counter).collect()) -> infer from receiver + method
        syn::Expr::MethodCall(method_call) => {
            if method_call.method == "collect" {
                // Infer the item type from the receiver, which should be an iterable
                let receiver_type = infer_each_expr_type(&method_call.receiver, reactive_vars, scoped);
                // Extract the item type from the iterable type (e.g. Range<i32> -> i32)
                let item_type = extract_iterable_item_type(&receiver_type);
                syn::parse_str(&format!("Vec<{}>", quote::quote!(#item_type)))
                    .expect("Failed to parse Vec type")
            } else {
                // Other method calls: try to infer from the receiver
                infer_each_expr_type(&method_call.receiver, reactive_vars, scoped)
            }
        }

        // Array literal (e.g. [1, 2, 3]) -> [T; N]
        syn::Expr::Array(array) => {
            let len = array.elems.len();
            let elem_type = array
                .elems
                .first()
                .map(|e| infer_scalar_type(e, reactive_vars, scoped))
                .unwrap_or_else(|| panic!("Cannot infer type of empty array"));
            syn::parse_str(&format!("[{}; {}]", quote::quote!(#elem_type), len))
                .expect("Failed to parse array type")
        }

        // Vec macro (e.g. vec![1, 2, 3]) -> Vec<T>
        syn::Expr::Macro(mac) => {
            if mac.mac.path.is_ident("vec") {
                // Parse the macro tokens to find the first element
                let tokens = &mac.mac.tokens;
                let elem: syn::Expr = syn::parse2(tokens.clone())
                    .unwrap_or_else(|_| panic!("Cannot parse vec! macro contents"));
                let elem_type = infer_scalar_type(&elem, reactive_vars, scoped);
                syn::parse_str(&format!("Vec<{}>", quote::quote!(#elem_type)))
                    .expect("Failed to parse Vec type")
            } else {
                panic!("Unsupported macro in #each expression: {:?}", mac.mac.path.get_ident())
            }
        }

        // Variable reference: look up in reactive_vars or scoped
        syn::Expr::Path(path) => {
            if let Some(ident) = path.path.get_ident() {
                // Check scoped vars first
                for var in scoped {
                    if var.name == *ident {
                        return (var.ty).clone();
                    }
                }
                // Check reactive vars
                for var in reactive_vars {
                    if var.name == *ident {
                        return var.ty.clone();
                    }
                }
            }
            panic!("Cannot infer type of path expression: {}", quote::quote!(#path))
        }

        // Reference expression: unwrap and recurse
        syn::Expr::Reference(reference) => {
            infer_each_expr_type(&reference.expr, reactive_vars, scoped)
        }

        _ => panic!("Cannot infer type of #each expression: {}", quote::quote!(#expr)),
    }
}

/// Infer the scalar type of an expression (used for range bounds, array elements, etc.)
fn infer_scalar_type(
    expr: &syn::Expr,
    reactive_vars: &[ReactiveVar],
    scoped: &Vec<EachVar>,
) -> syn::Type {
    match expr {
        syn::Expr::Lit(lit) => match &lit.lit {
            syn::Lit::Int(int) => {
                if !int.suffix().is_empty() {
                    syn::parse_str(int.suffix()).expect("Failed to parse int suffix")
                } else {
                    syn::parse_str("i32").unwrap()
                }
            }
            syn::Lit::Float(float) => {
                if !float.suffix().is_empty() {
                    syn::parse_str(float.suffix()).expect("Failed to parse float suffix")
                } else {
                    syn::parse_str("f64").unwrap()
                }
            }
            syn::Lit::Str(_) => syn::parse_str("String").unwrap(),
            syn::Lit::Bool(_) => syn::parse_str("bool").unwrap(),
            syn::Lit::Char(_) => syn::parse_str("char").unwrap(),
            _ => panic!("Cannot infer type of literal: {}", quote::quote!(#lit)),
        },
        syn::Expr::Path(path) => {
            if let Some(ident) = path.path.get_ident() {
                for var in scoped {
                    if var.name == *ident {
                        return (var.ty).clone();
                    }
                }
                for var in reactive_vars {
                    if var.name == *ident {
                        return var.ty.clone();
                    }
                }
            }
            panic!("Cannot infer type of path expression: {}", quote::quote!(#path))
        }
        syn::Expr::Paren(paren) => infer_scalar_type(&paren.expr, reactive_vars, scoped),
        syn::Expr::Group(group) => infer_scalar_type(&group.expr, reactive_vars, scoped),
        syn::Expr::Reference(reference) => infer_scalar_type(&reference.expr, reactive_vars, scoped),
        syn::Expr::Unary(unary) => infer_scalar_type(&unary.expr, reactive_vars, scoped),
        _ => panic!("Cannot infer scalar type of expression: {}", quote::quote!(#expr)),
    }
}

/// Extract the item type from an iterable type (e.g. Range<i32> -> i32, Vec<String> -> String)
fn extract_iterable_item_type(ty: &syn::Type) -> syn::Type {
    match ty {
        syn::Type::Path(path) => {
            let last_segment = path.path.segments.last()
                .expect("Empty path in iterable type");
            match &last_segment.arguments {
                syn::PathArguments::AngleBracketed(args) => {
                    if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                        inner.clone()
                    } else {
                        panic!("Cannot extract item type from: {}", quote::quote!(#ty))
                    }
                }
                _ => panic!("Cannot extract item type from non-generic type: {}", quote::quote!(#ty)),
            }
        }
        syn::Type::Array(array) => *array.elem.clone(),
        _ => panic!("Cannot extract item type from: {}", quote::quote!(#ty)),
    }
}