use proc_macro2::TokenStream;
use syn::{FnArg, Ident};

use crate::parse::html_parse::AttrType;

pub fn gen_expr(
    attr: &AttrType,
    funcs: &Vec<FuncData>,
    reactive_vars: &Vec<ReactiveVar>,
) -> syn::Result<(syn::ExprCall, u32)> {
    match attr {
        AttrType::Call(func_name) => {
            // Find the function in the list of functions
            let func = funcs
                .iter()
                .find(|f| f.code.sig.ident == *func_name)
                .expect("Function not found for event handler");
            if !func.is_event_handler {
                return Err(syn::Error::new(
                    func.code.sig.ident.span(),
                    format!("Function '{}' is not an event handler", func_name),
                ));
            }

            let mut dirty_flags = 0u32;
            let (mut closure_args, mut closure_locks): (Vec<Ident>, Vec<TokenStream>) = func
                .reactive_args
                .iter()
                .map(|arg| {
                    let var = reactive_vars
                        .iter()
                        .find(|rv| rv.var.name == *arg)
                        .expect("Reactive variable not found");
                    dirty_flags |= 1 << var.flag_pos;
                    let var_name_caps = syn::Ident::new(
                        &var.var.name.to_string().to_uppercase(),
                        var.var.name.span(),
                    );

                    (
                        syn::parse2(quote::quote! { #arg }).unwrap(),
                        quote::quote! { &mut #var_name_caps.lock().unwrap() },
                    )
                })
                .unzip();

            if let Some(event_arg) = &func.event_arg {
                let event_arg_ident = event_arg.clone();
                closure_args.push(event_arg_ident);
                closure_locks.push(quote::quote! { e });
            }

            let func_name = &func.code.sig.ident;
            let func_call = quote::quote! {
                #func_name(#(#closure_args),*)
            };

            let closure_out: syn::ExprCall = syn::parse2(
                quote::quote! {
                    (|#(#closure_args),*| {
                        #func_call
                    })(#(#closure_locks),*)
                }
                .into(),
            )?;

            Ok((closure_out, dirty_flags))
        }
        AttrType::Closure(attr_expr) => {
            let mut dirty_flags = 0u32;
            let (mut closure_args, mut closure_locks): (Vec<Ident>, Vec<TokenStream>) = attr_expr
                .args
                .iter()
                .map(|arg| {
                    let var = reactive_vars
                        .iter()
                        .find(|rv| rv.var.name == *arg)
                        .expect("Reactive variable not found");
                    dirty_flags |= 1 << var.flag_pos;
                    let var_name_caps = syn::Ident::new(
                        &var.var.name.to_string().to_uppercase(),
                        var.var.name.span(),
                    );

                    (
                        syn::parse2(quote::quote! { #arg }).unwrap(),
                        quote::quote! { &mut #var_name_caps.lock().unwrap() },
                    )
                })
                .unzip();

            if let Some(event_arg) = &attr_expr.event_arg {
                closure_args.push(event_arg.clone());
                closure_locks.push(quote::quote! { e });
            }

            let body = &attr_expr.body;
            let closure_out: syn::ExprCall = syn::parse2(
                quote::quote! {
                    (|#(#closure_args),*| {
                        #body
                    })(#(#closure_locks),*)
                }
                .into(),
            )?;

            Ok((closure_out, dirty_flags))
        }
        AttrType::Expr(expr) => {
            // Look for reactive variables in the expression
            let mut dirty_flags = 0u32;
            let reactive_vars_in_expr = get_reactive_vars_in_expr(expr, reactive_vars);
            for var in reactive_vars_in_expr {
                dirty_flags |= 1 << var.flag_pos;
            }

            let closure_out: syn::ExprCall = syn::parse2(
                quote::quote! {
                    (|| {
                        #expr
                    })()
                }
                .into(),
            )?;
            Ok((closure_out, dirty_flags))
        }
        _ => {
            panic!("Unsupported attribute type for event handler");
        }
    }
}

#[derive(Clone)]
pub struct AttrClosure {
    pub state_arg: bool,
    pub event_arg: Option<syn::Type>,
    pub body: syn::Expr,
}

pub fn parse_attr_expression(expr: &String, is_event: bool) -> syn::Result<AttrType> {
    let expr_chars = &mut expr.chars().peekable();
    if expr_chars.peek() == Some(&'|') {
        // Closure expression
        // Read until the closing '|'
        let mut closure_str = String::new();
        while let Some(&ch) = expr_chars.peek() {
            closure_str.push(ch);
            expr_chars.next();
            if ch == '|' && closure_str.len() > 1 {
                break;
            }
        }

        let mut state_arg = false;
        let mut event_arg = None;
        let closure_args_strs = closure_str
            .trim_matches('|')
            .split(',')
            .map(|s| s.trim())
            .collect::<Vec<&str>>();

        for arg in closure_args_strs {
            if arg == "$state" {
                state_arg = true;
            } else {
                if let None = event_arg {
                    if !is_event {
                        return Err(syn::Error::new(
                            proc_macro2::Span::call_site(),
                            "Non-event handler closures cannot have non-reactive arguments",
                        ));
                    }
                    event_arg = if let Ok(FnArg::Typed(arg)) = syn::parse_str::<syn::FnArg>(arg) {
                        Some(*arg.ty)
                    } else {
                        Some(syn::parse2(quote::quote! {web_sys::Event})?)
                    }
                } else {
                    return Err(syn::Error::new(
                        proc_macro2::Span::call_site(),
                        "Multiple event arguments found in closure; only one event argument is allowed",
                    ));
                }
            }
        }

        let remaining_expr: String = expr_chars.collect();
        log::info!("Parsing closure body: {}", remaining_expr);
        let body_expr: syn::Expr = syn::parse_str(&remaining_expr)?;

        Ok(AttrType::Closure(AttrClosure {
            state_arg,
            event_arg,
            body: body_expr,
        }))
    } else {
        // Regular expression
        let regular_expr: syn::Expr = syn::parse_str(expr)?;
        Ok(AttrType::Expr(regular_expr))
    }
}
