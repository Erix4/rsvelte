use syn::FnArg;

use crate::parse::html_parse::AttrType;

#[derive(Clone)]
pub struct AttrClosure {
    pub event_arg: Option<(syn::Ident, syn::Type)>,
    pub body: syn::Expr,
}

pub fn parse_attr_expression(
    expr: &str,
    is_event: bool,
) -> syn::Result<AttrType> {
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

        let mut event_arg = None;
        let closure_args_strs = closure_str
            .trim_matches('|')
            .split(',')
            .map(|s| s.trim())
            .filter(|a| !a.is_empty())
            .collect::<Vec<&str>>();

        for arg in closure_args_strs {
            if let None = event_arg {
                if !is_event {
                    return Err(syn::Error::new(
                        proc_macro2::Span::call_site(),
                        "Non-event handler closures cannot have non-reactive arguments",
                    ));
                }
                event_arg = if let Ok(FnArg::Typed(arg)) =
                    syn::parse_str::<syn::FnArg>(arg)
                    && let syn::Pat::Ident(ident) = *arg.pat
                {
                    Some((ident.ident, *arg.ty))
                } else {
                    // If no type annotation, assume it's an event argument of type `web_sys::Event`
                    // and parse the argument as an identifier
                    Some((
                        syn::Ident::new(arg, proc_macro2::Span::call_site()),
                        syn::parse2(quote::quote! {web_sys::Event})?,
                    ))
                }
            } else {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "Multiple event arguments found in closure; only one event argument is allowed",
                ));
            }
        }

        let remaining_expr: String = expr_chars.collect();
        let body_expr: syn::Expr = syn::parse_str(&remaining_expr)?;

        Ok(AttrType::Closure(AttrClosure {
            event_arg,
            body: body_expr,
        }))
    } else {
        // Regular expression
        let regular_expr: syn::Expr = syn::parse_str(expr)?;
        Ok(AttrType::Expr(regular_expr))
    }
}
