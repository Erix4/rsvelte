use syn::Ident;

use crate::{
    parse::html_parse::{AttrType, Tag},
    transform::{
        ReactiveVar, expr::transform_content_expr, node::TagAttribute,
    },
};

/// distinguish reactive expressions from function calls and static values,
/// and populate flag masks for reactive expressions
pub fn transform_attr(
    tag: Tag,
    state_vars: &Vec<ReactiveVar>,
    reactive_vars: &Vec<ReactiveVar>,
    state_funcs: &Vec<&Ident>,
) -> (String, Vec<TagAttribute>) {
    let mut attrs_out = Vec::new();

    for (name, attr) in tag.attributes {
        match attr {
            AttrType::Str(str) => attrs_out.push(TagAttribute {
                name,
                value: AttrType::Str(str),
                flag_mask: None,
            }),
            AttrType::Closure(closure) => attrs_out.push(TagAttribute {
                name,
                value: AttrType::Closure(closure),
                flag_mask: None,
            }),
            AttrType::Expr(expr) => {
                let (expr, flag_mask) =
                    transform_content_expr(expr, state_vars, reactive_vars);
                if flag_mask == 0 {
                    // No reactive vars, check if it's a function call to a state function
                    let attr_type = if let syn::Expr::Path(path) = expr {
                        if state_funcs.iter().any(|v| path.path.is_ident(*v)) {
                            AttrType::Call(path.path.segments[0].ident.clone())
                        } else {
                            AttrType::Expr(syn::Expr::Path(path))
                        }
                    } else {
                        AttrType::Expr(expr)
                    };

                    // Dynamic expression, needs to be updated in patch function
                    attrs_out.push(TagAttribute {
                        name,
                        value: attr_type,
                        flag_mask: Some(flag_mask),
                    })
                }
            }
            _ => {} // parsing does not generate calls
        }
    }

    (tag.name, attrs_out)
}
