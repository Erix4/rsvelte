use crate::transform::{Node, node::NodeType};

/// Unmount function is the same for all fragment types
pub fn get_unmount_func(nodes: &Vec<Node>) -> proc_macro2::TokenStream {
    let unmount_code = nodes
        .iter()
        .flat_map(|node| node.get_unmount_code())
        .collect::<Vec<_>>();

    quote::quote! {
        fn unmount(&mut self) {
            #(#unmount_code)*
        }
    }
}

impl Node {
    pub fn get_unmount_code(&self) -> proc_macro2::TokenStream {
        let struct_field = &self.struct_field;
        match &self.content {
            NodeType::If(_, _, _, _)
            | NodeType::Each(_, _, _, _, _)
            | NodeType::Comp(_, _) => {
                // Unmounting of #if and #each fragments is done inside functions, so we just call those functions here
                // Note that unmount functions do not return a result
                quote::quote! {
                    self.#struct_field.unmount();
                }
            }
            _ => {
                // For most content types, just remove the node itself, which will also remove all children
                quote::quote! {
                    self.#struct_field.remove();
                }
            }
        }
    }
}
