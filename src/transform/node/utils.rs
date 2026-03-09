use std::collections::HashMap;

use crate::transform::node::{Node, NodeType, each_block::EachVar};

pub fn map_to_update(
    update_map: HashMap<u64, proc_macro2::TokenStream>,
) -> proc_macro2::TokenStream {
    let mut checks = quote::quote! {};
    for (flag_mask, code) in update_map {
        checks.extend(quote::quote! {
            if flags & #flag_mask != 0 {
                #code
            }
        });
    }
    checks
}

pub fn get_tuple_type_for_nodes(nodes: &Vec<Node>) -> proc_macro2::TokenStream {
    let types = nodes
        .iter()
        .map(|node| node.get_tuple_types())
        .flatten()
        .collect::<Vec<_>>();
    if types.len() == 1 {
        // Rust doesn't like tuples with one item, so we can just return the type itself in that case
        types.into_iter().next().unwrap()
    } else {
        quote::quote! { (#(#types),*) }
    }
}

pub fn get_content_accessor(
    node_len: usize,
    idx: usize,
) -> proc_macro2::TokenStream {
    if node_len == 1 {
        // If there's only one node, the content is not actually a tuple, so we can just return it directly
        quote::quote! { contents }
    } else {
        let idx = syn::Index::from(idx);
        quote::quote! { contents.#idx }
    }
}

pub fn merge_hashmaps(
    map1: HashMap<u64, proc_macro2::TokenStream>,
    map2: HashMap<u64, proc_macro2::TokenStream>,
) -> HashMap<u64, proc_macro2::TokenStream> {
    let mut merged = map1;
    for (flag_mask, code) in map2 {
        merge_into_hashmap(&mut merged, flag_mask, code);
    }
    merged
}

pub fn merge_into_hashmap(
    map: &mut HashMap<u64, proc_macro2::TokenStream>,
    flag_mask: u64,
    code: proc_macro2::TokenStream,
) {
    map.entry(flag_mask)
        .and_modify(|existing_code: &mut proc_macro2::TokenStream| {
            existing_code.extend(code.clone())
        })
        .or_insert(code);
}

/// Formats scoped variables as arguments to functions that create/mount/update #if and #each blocks
/// NOTE: includes a leading comma if there are any scoped variables, so that it can be appended to the end of the state argument
pub fn scoped_vars_as_args(
    scoped_vars: &Vec<EachVar>,
) -> proc_macro2::TokenStream {
    if scoped_vars.is_empty() {
        return quote::quote! {};
    }
    let args = scoped_vars.iter().map(|var| var.to_arg());
    quote::quote! { , #(#args),* }
}

/// Like `scoped_vars_as_args` but for function parameters instead of arguments.
/// This means it just returns the variable names without the types.
/// NOTE: includes the leading comma, so it can be used directly in function signatures after the state parameter
pub fn scoped_vars_as_params(
    scoped_vars: &Vec<EachVar>,
) -> proc_macro2::TokenStream {
    if scoped_vars.is_empty() {
        return quote::quote! {};
    }
    let params = scoped_vars.iter().map(|var| &var.name);
    quote::quote! { , #(#params),* }
}

pub fn find_all_fragments(nodes: &Vec<Node>) -> Vec<&Node> {
    let mut fragments = Vec::new();
    for node in nodes {
        match &node.content {
            NodeType::If(_, _, _, _) | NodeType::Each(_, _, _, _) => {
                fragments.push(node);
            }
            NodeType::Tag(_, _, children) => {
                let child_fragments = find_all_fragments(children);
                fragments.extend(child_fragments);
            }
            _ => {}
        }
    }
    fragments
}