use syn::Ident;

use crate::transform::{Node, NodeType};

/// Get mounting function for the root fragment
pub fn get_mount_func_root(nodes: &Vec<Node>) -> proc_macro2::TokenStream {
    get_mount_func_ex(nodes, false)
}

/// Get mounting function for if branch and each fragments
pub fn get_mount_func_block(nodes: &Vec<Node>) -> proc_macro2::TokenStream {
    get_mount_func_ex(nodes, true)
}

fn get_mount_func_ex(
    nodes: &Vec<Node>,
    parent_arg: bool,
) -> proc_macro2::TokenStream {
    let parent_arg = if parent_arg {
        quote::quote! { parent: &web_sys::Element, }
    } else {
        quote::quote! {}
    };

    let add_method = |struct_field: &Ident| {
        quote::quote! { add_method(&self.#struct_field); }
    };
    let mounts = nodes
        .iter()
        .map(|node| node.get_mount_code(&None, &add_method))
        .collect::<Vec<_>>();

    quote::quote! {
        fn mount(&self, #parent_arg add_method: impl crate::AddMethod) -> Result<(), crate::JsValue> {
            // mounts
            #(#mounts)*

            Ok(())
        }
    }
}

impl Node {
    /// Generate code to mount this node and all its children,
    /// assuming that said nodes have already been created and assigned to a contents tuple.
    ///
    /// NOTE: this does not traverse through fragments,
    /// only mounts the elements in this fragments and calls functions
    /// to mount its children.
    ///
    /// When calling from root level of a fragment, parent should be None and add_method
    /// should simple return `add_method(&self.#struct_field)`.
    fn get_mount_code(
        &self,
        parent: &Option<&Ident>,
        add_method: &dyn Fn(&Ident) -> proc_macro2::TokenStream,
    ) -> proc_macro2::TokenStream {
        let struct_field = &self.struct_field;
        let add_line = add_method(&self.struct_field);
        match &self.content {
            NodeType::Text(_) | NodeType::Expr(_, _) => {
                quote::quote! {
                    #add_line
                }
            }
            NodeType::Tag(_, _, child_contents) => {
                let mut code = quote::quote! {
                    #add_line
                };

                let child_add_method= |child_struct_field: &Ident| {
                    quote::quote! {
                        self.#struct_field.append_child(&self.#child_struct_field)?;
                    }
                };

                for child in child_contents {
                    let child_code = child
                        .get_mount_code(&Some(struct_field), &child_add_method);
                    code.extend(child_code);
                }

                code
            }
            NodeType::If(_, _, _, _) | NodeType::Each(_, _, _, _, _) => {
                // Mounting of #if and #each fragments is done inside functions, so we just call those functions here
                let add_method = if let Some(parent) = parent {
                    quote::quote! { child_append_closure(&self.#parent) }
                } else {
                    quote::quote! {quote::quote! { add_method } }
                };
                quote::quote! {
                    self.#struct_field.mount(&self.#parent, #add_method)?;
                }
            }
            NodeType::Comp(_, _) => {
                let add_method = if let Some(parent) = parent {
                    quote::quote! { child_append_closure(&self.#parent) }
                } else {
                    quote::quote! {quote::quote! { add_method } }
                };
                quote::quote! {
                    self.#struct_field.mount(#add_method)?;
                }
            }
        }
    }
}
