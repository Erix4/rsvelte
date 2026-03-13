use crate::{
    code_gen::scope::ScopeData, parse::html_parse::AttrType, transform::{Node, NodeType}
};

/// Generates the `update` function for root fragments
pub fn get_update_func_root(
    nodes: &Vec<Node>,
    scope: &ScopeData,
) -> proc_macro2::TokenStream {
    get_update_func_ex(nodes, quote::quote! { () }, scope, false)
}

/// Generates the `update` function for if branch and each fragments
pub fn get_update_func_block(
    nodes: &Vec<Node>,
    scope: &ScopeData,
) -> proc_macro2::TokenStream {
    get_update_func_ex(nodes, quote::quote! { Self::Scope<'_> }, scope, true)
}

pub fn get_update_func_each(
    nodes: &Vec<Node>,
    scope: &ScopeData,
) -> proc_macro2::TokenStream {
    get_update_func_ex(
        nodes,
        quote::quote! { (Self::Scope<'_>, &Self::Item) },
        scope,
        true,
    )
}

fn get_update_func_ex(
    nodes: &Vec<Node>,
    scope_type: proc_macro2::TokenStream,
    scope: &ScopeData,
    parent_arg: bool,
) -> proc_macro2::TokenStream {
    let parent_arg = if parent_arg {
        quote::quote! { parent: &web_sys::Element, }
    } else {
        quote::quote! {}
    };

    let scope_destructor = scope.get_destructor();

    let update_code = nodes
        .iter()
        .flat_map(|node| node.get_update_code(quote::quote! {update}))
        .collect::<Vec<_>>();

    quote::quote! {
        fn update(&mut self, #parent_arg state: &Self::State, scope: #scope_type, flags: u64) -> Result<(), crate::JsValue> {
            let #scope_destructor = scope;

            // update code
            #(#update_code)*

            Ok(())
        }
    }
}

impl Node {
    /// Generate the code to update this node based on changes in state, for example:
    /// ```
    /// if flag_snapshot & 1 << 1 != 0 {
    ///   self.elements.1.set_text_content(Some(&new_value));
    /// }
    /// ```
    pub fn get_update_code(
        &self,
        parent: proc_macro2::TokenStream,
    ) -> Vec<proc_macro2::TokenStream> {
        let mut code = Vec::new();
        let struct_field = &self.struct_field;
        match &self.content {
            NodeType::Expr(expr, flag_mask) => {
                // Expression nodes need to be updated whenever their dependencies change, so we generate code for that
                code.push(quote::quote! {
                    if flags & #flag_mask != 0 {
                        self.#struct_field.set_text_content(Some(&format!("{}", #expr)));
                    }
                });
            }
            NodeType::Tag(_, attributes, children) => {
                // For tags, we need to combine the update code of all children
                for child in children {
                    code.extend(child.get_update_code(
                        quote::quote! { &self.#struct_field },
                    ));
                }

                // Check for reactive attributes
                for attr in attributes.iter() {
                    if let Some(flag_mask) = attr.flag_mask
                        && let AttrType::Expr(expr) = &attr.value
                    {
                        let attr_name = &attr.name;
                        code.push(quote::quote! {
                            if flags & #flag_mask != 0 {
                                self.#struct_field.set_attribute(#attr_name, &format!("{}", #expr))?;
                            }
                        });
                    }
                }
            }
            NodeType::If(_, _, _, _) | NodeType::Each(_, _, _, _, _) => {
                // Updating of #if and #each fragments is done inside functions, so we just call those functions here
                code.push(quote::quote! {
                    self.#struct_field.update(#parent, state, scope, flags)?;
                });
            }
            NodeType::Comp(_, props) => {
                let mut all_comp_flags = 0;

                let mut prop_setters = Vec::new();
                for (prop, child_comp_mask) in props.iter() {
                    if let Some(flag_mask) = prop.flag_mask
                        && let AttrType::Expr(expr) = &prop.value
                    {
                        let prop_name = &prop.name;
                        prop_setters.push(quote::quote! {
                            if flags & #flag_mask != 0 {
                                self.#struct_field.#prop_name = #expr;
                                DIRTY_FLAGS.fetch_or(#child_comp_mask, SeqCst);
                            }
                        });
                        all_comp_flags |= flag_mask;
                    }
                }

                code.push(quote::quote! {
                    if flags & #all_comp_flags != 0 {
                        DIRTY_FLAGS.store(0, SeqCst);

                        #(#prop_setters)*

                        self.#struct_field.apply()?;
                    }
                });
            }
            _ => {}
        }
        code
    }
}
