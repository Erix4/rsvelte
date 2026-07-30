use crate::transform::{ReactiveVar, node::EachVar};
use quote::ToTokens;
use syn::visit_mut::VisitMut;

struct VarVisitor<'a> {
    state_vars: &'a Vec<ReactiveVar>,
    reactive_vars: &'a Vec<ReactiveVar>,
    scoped: &'a Vec<EachVar>,
    flags: u64,
}

/// Find all state variables in the expression, dereference them if necessary and add state accessor,
/// and extract a bitmask of which variables are used.
///
/// As an example, the {counter} expression (provided counter is a state variable),
/// would become: `*state.counter`, but {my_struct.a} would be `state.my_struct.a`.
/// There's no need to dereference if the state variable is already being accessed with dot notation.
/// Deferences are only needed on $state variables, as all others have manually set flags.
///
/// In addition to state variables, scoped variables also need to be adjusted, not to
/// dereference them but to replace them with their modified identifiers (modified to avoid
/// collisions with non-user code). This is done via similar logic to state variables.
pub fn transform_content_expr(
    mut expr: syn::Expr,
    state_vars: &Vec<ReactiveVar>,
    reactive_vars: &Vec<ReactiveVar>,
    scoped: &Vec<EachVar>,
) -> (syn::Expr, u64) {
    // Use a visitor to traverse the expression AST
    impl<'a, 'ast> syn::visit_mut::VisitMut for VarVisitor<'a> {
        fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
            log::info!(
                "Visiting expression {}",
                expr.to_token_stream().to_string()
            );
            if let syn::Expr::Path(path) = expr.clone() {
                log::info!(
                    "Found path expression {} segments long",
                    path.path.segments.len()
                );
                if path.path.segments.len() == 1 {
                    let ident = &path.path.segments[0].ident;
                    log::info!("checking for {}", ident);
                    for var in self.reactive_vars.iter() {
                        log::info!(
                            "checking reactive var {} against {}",
                            var.name,
                            ident
                        );
                        if var.name == *ident {
                            // Found a state variable, set the corresponding flag
                            self.flags |= var.flag_mask;
                            // Replace with state accessed version
                            *expr = syn::parse2(quote::quote! {
                                state.#ident
                            })
                            .unwrap();
                            break;
                        }
                    }
                    for var in self.state_vars.iter() {
                        if var.name == *ident {
                            // Found a state variable, set the corresponding flag, & add dereference
                            self.flags |= var.flag_mask;
                            // Replace with dereferenced state accessed version
                            *expr = syn::parse2(quote::quote! {
                                *state.#ident
                            })
                            .unwrap();
                            break;
                        }
                    }
                    for var in self.scoped.iter() {
                        if var.original_name == *ident {
                            // Found a scoped variable, replace with modified identifier
                            let modified_ident = &var.name;
                            *expr = syn::parse2(quote::quote! {
                                #modified_ident
                            })
                            .unwrap();
                            break;
                        }
                    }
                }
            }
            syn::visit_mut::visit_expr_mut(self, expr);
        }

        fn visit_macro_mut(&mut self, i: &mut syn::Macro) {
            // Doesn't traverse by default, override
            log::info!("Visiting macro {}", i.to_token_stream().to_string());

            // Manually visit the tokens of the macro, as they may contain expressions that need to be transformed
            let new_tokens = transform_token_stream(i.tokens.clone(), self);
            i.tokens = new_tokens;
        }
    }

    let mut visitor = VarVisitor {
        state_vars,
        reactive_vars,
        scoped,
        flags: 0,
    };

    visitor.visit_expr_mut(&mut expr);

    (expr, visitor.flags)
}

fn transform_token_stream(
    tokens: proc_macro2::TokenStream,
    visitor: &mut VarVisitor,
) -> proc_macro2::TokenStream {
    let mut new_tokens = proc_macro2::TokenStream::new();
    for token in tokens {
        match token {
            proc_macro2::TokenTree::Group(group) => {
                let new_group = proc_macro2::Group::new(
                    group.delimiter(),
                    transform_token_stream(group.stream(), visitor),
                );
                new_tokens.extend(std::iter::once(
                    proc_macro2::TokenTree::Group(new_group),
                ));
            }
            proc_macro2::TokenTree::Punct(punct) => {
                new_tokens.extend(std::iter::once(
                    proc_macro2::TokenTree::Punct(punct),
                ));
            }
            proc_macro2::TokenTree::Literal(literal) => {
                new_tokens.extend(std::iter::once(
                    proc_macro2::TokenTree::Literal(literal),
                ));
            }
            proc_macro2::TokenTree::Ident(ident) => {
                // Check if the identifier is a reactive variable and replace it with state access if so
                let ident_str = ident.to_string();
                let mut replaced = false;
                for var in visitor.reactive_vars.iter() {
                    if var.name == ident_str {
                        // Found a reactive variable, replace with state access
                        new_tokens.extend(quote::quote! {
                            state.#ident
                        });
                        replaced = true;
                        break;
                    }
                }
                if !replaced {
                    new_tokens.extend(std::iter::once(
                        proc_macro2::TokenTree::Ident(ident),
                    ));
                }
            }
        }
    }
    new_tokens
}

/// Similar to transform_content_expr, but also keep list of which reactive variables are used
pub fn transform_derived_expr(
    expr: syn::Expr,
    state_vars: &Vec<ReactiveVar>,
    reactive_vars: &Vec<ReactiveVar>,
) -> (syn::Expr, Vec<syn::Ident>) {
    let mut used_vars = Vec::new();

    struct VarVisitor<'a> {
        state_vars: &'a Vec<ReactiveVar>,
        reactive_vars: &'a Vec<ReactiveVar>,
        used_vars: &'a mut Vec<syn::Ident>,
    }

    impl<'a, 'ast> syn::visit_mut::VisitMut for VarVisitor<'a> {
        fn visit_expr_mut(&mut self, i: &mut syn::Expr) {
            if let syn::Expr::Path(path) = i.clone() {
                if path.path.segments.len() == 1 {
                    let ident = &path.path.segments[0].ident;
                    for var in self.reactive_vars.iter() {
                        if var.name == *ident {
                            // Found a state variable, add to used vars
                            if !self.used_vars.contains(ident) {
                                self.used_vars.push(ident.clone());
                            }
                            // Replace with state accessed version (no dereference for non-state vars)
                            *i = syn::parse2(quote::quote! {
                                state.#ident
                            })
                            .unwrap();
                            break;
                        }
                    }
                    for var in self.state_vars.iter() {
                        if var.name == *ident {
                            // Found a state variable, add to used vars, & add dereference
                            if !self.used_vars.contains(ident) {
                                self.used_vars.push(ident.clone());
                            }
                            // Replace with dereferenced state accessed version
                            *i = syn::parse2(quote::quote! {
                                *state.#ident
                            })
                            .unwrap();
                            break;
                        }
                    }
                } else {
                    // Check if the first segment is a reactive variable, if so add to used vars (handles dot notation case)
                    let first_ident = &path.path.segments[0].ident;
                    for var in self.reactive_vars.iter() {
                        if var.name == *first_ident {
                            if !self.used_vars.contains(first_ident) {
                                self.used_vars.push(first_ident.clone());
                            }
                        }
                    }
                }
            }
            syn::visit_mut::visit_expr_mut(self, i);
        }
    }

    let mut visitor = VarVisitor {
        state_vars,
        reactive_vars,
        used_vars: &mut used_vars,
    };

    let mut expr_mut = expr.clone();
    visitor.visit_expr_mut(&mut expr_mut);

    (expr_mut, used_vars)
}
