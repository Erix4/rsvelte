use std::collections::HashMap;

use crate::transform::node::{Node, NodeType};

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

pub fn find_all_fragments(nodes: &Vec<Node>) -> Vec<&Node> {
    let mut fragments = Vec::new();
    for node in nodes {
        match &node.content {
            NodeType::If(_, _, _, _) | NodeType::Each(_, _, _, _, _) => {
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

/// Converts a number to a string like "a", "b", ..., "z", "aa", "ab", etc.
/// for generating field names for fragment structs
pub fn num_to_letter(num: usize) -> String {
    let mut result = String::new();
    let mut n = num + 1; // Start from 1 instead of 0 to get "a" for 0, "b" for 1, etc.
    while n > 0 {
        n -= 1; // Adjust for 0-indexing
        let letter = (b'a' + (n % 26) as u8) as char;
        result.insert(0, letter);
        n /= 26;
    }
    result
}