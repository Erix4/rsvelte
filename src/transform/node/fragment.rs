use quote::format_ident;
use syn::Ident;

use crate::transform::Node;

pub fn get_all_fragment_code(comp_id: String, root_node: &Node) -> proc_macro2::TokenStream {
    let root_fragment_name = format_ident!("C{}RootFrag", comp_id);

    //
}

fn get_struct_declaration(fragment_name: &Ident, nodes: &Vec<Node>) -> proc_macro2::TokenStream {
    let struct_fields = nodes
        .iter()
        .map(|node| node.get_fields())
        .flatten();
    let struct_types = nodes
        .iter()
        .map(|node| node.get_field_types())
        .flatten();
    let struct_field_declarations = struct_fields
        .zip(struct_types)
        .map(|(field, ty)| quote::quote! { #field: #ty })
        .collect::<Vec<_>>();

    quote::quote! {
        struct #fragment_name {
            #(#struct_field_declarations),*
        }
    }
}
