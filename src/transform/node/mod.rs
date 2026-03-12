use std::collections::HashMap;

use quote::format_ident;
use syn::Ident;

mod attr;
mod fragment;
mod if_funcs;
mod mount_func;
mod new_func;
mod proc_func;
mod scope;
mod unmount_func;
mod update_func;
mod utils;

use crate::{
    parse::{
        ComponentAST,
        html_parse::{AttrType, ContentType, Element},
    },
    transform::{
        ReactiveVar,
        expr::{infer_iter_item_type, transform_content_expr},
        node::{attr::transform_attr, utils::*},
    },
};

struct NodeIfBranch {
    pub condition: syn::Expr,
    pub contents: Vec<Node>,
    pub name: Ident, // Name enum branch
}

pub struct TagAttribute {
    pub name: String,
    pub value: AttrType,
    pub flag_mask: Option<u64>, // Only set for reactive attributes
}

pub struct EachVar {
    pub name: Ident,
    pub ty: syn::Type,
}

pub enum NodeType {
    Text(String),
    Expr(syn::Expr, u64), // Expression and its dirty flag mask
    Tag(String, Vec<TagAttribute>, Vec<Node>), // tag name, its attributes and its contents
    If(Vec<NodeIfBranch>, Option<Vec<Node>>, Ident, u64), // if branches, else branch, enum name, expression dirty flag mask
    Each(syn::Expr, EachVar, Vec<Node>, Ident, u64), // iterable expression, item var, contents, fragment name, expression dirty flag mask
    Comp(String, Vec<(TagAttribute, u64)>), // component name and its props & their child comp masks
}

/// Represents a node in the transformed AST, which can be used for code generation.
///
/// Nodes are interacted with as such:
///
/// ## Creating
/// Creation of the node, and assignment to mount_name for storage in elements tuple
/// ```
/// let node_1 = document.create_element("div")?;
/// ```
/// Creation is done without state, so reactive nodes must be updated afterwards.
/// Use cases:
///   - Normal creation of new nodes
///   - Creation of if block branch on condition change
///
/// ## Mounting
/// Insertion of the node into the DOM
/// ```
/// parent.append_child(&node_1)?;
/// ```
/// Separate from creation because sometimes nodes are moved (like in #each blocks).
/// Use cases:
///     - Normal mounting of new nodes
///     - Mounting of if block branch on condition change
///     - Moving nodes in #each blocks when the diffing calls for it
/// The style of parent depends on the context. In initial creation, the parent is
/// a static ident, whereas elsewhere it is an item in the tuple.
///
/// ## Updating
/// Updating the content of an existing node as needed based on changes in state
/// ```
/// if flag_snapshot & 1 << 1 != 0 {
///   self.elements.1.set_text_content(Some(&new_value));
/// }
/// ```
/// This is done through accessing the elements tuple.
/// Use cases:
///   - Updating text nodes and expression nodes when their content changes
///
/// ## Unmounting
/// Removing nodes from the DOM when they are no longer needed, for example when an if block condition becomes false
/// ```
/// if self.if_branch_1 {
///     self.elements.1.if_content
/// }
/// ```
/// Use cases:
///   - Unmounting if block branches when they become inactive
///   - Unmounting #each block content when it becomes inactive
pub struct Node {
    pub id: u32,
    pub tuple_idx: usize,
    pub struct_field: Ident,
    pub content: NodeType,
}

impl Node {
    /// Converts an Element into a Node, assigning tuple indices and mount names as needed.
    pub fn from_element(
        value: Element,
        tuple_idx_counter: &mut usize,
        state_vars: &Vec<ReactiveVar>,
        reactive_vars: &Vec<ReactiveVar>,
        state_funcs: &Vec<&Ident>,
        component_map: &HashMap<&String, &ComponentAST>,
        comp_id_hash: &String,
    ) -> Self {
        let tuple_idx = *tuple_idx_counter;
        *tuple_idx_counter += 1;
        let struct_field = format_ident!("{}", num_to_letter(tuple_idx));
        let content = match value.content {
            ContentType::Text(txt) => NodeType::Text(txt),
            ContentType::Expr(expr) => {
                let (expr, flag_mask) =
                    transform_content_expr(expr, state_vars, reactive_vars);
                NodeType::Expr(expr, flag_mask)
            }
            ContentType::Tag(tag, children) => {
                let (tag_name, attributes): (String, Vec<TagAttribute>) =
                    transform_attr(tag, state_vars, reactive_vars, state_funcs);
                if tag_name.starts_with(char::is_uppercase) {
                    // check if this is a valid component
                    if let Some(comp_ast) = component_map.get(&tag_name) {
                        // Get comp type from comp id hash
                        let comp_name = format!("C{}", comp_ast.id_hash);
                        let attributes = if let Some(props) =
                            comp_ast.script.as_ref().map(|script| &script.props)
                        {
                            attributes
                                .into_iter()
                                .map(|attr| {
                                    let flag_mask = props
                                        .iter()
                                        .find(|prop| prop.name == attr.name)
                                        .map(|prop| 1 << prop.flag_pos).expect("Component props must be defined in the component script");
                                    (attr, flag_mask)
                                })
                                .collect()
                        } else {
                            Vec::new()
                        };
                        return Node {
                            id: value.id,
                            tuple_idx,
                            struct_field,
                            content: NodeType::Comp(comp_name, attributes),
                        };
                    } else {
                        panic!("Component {} not found in imports", tag_name);
                    }
                }
                let child_nodes = children
                    .into_iter()
                    .map(|child| {
                        Node::from_element(
                            child,
                            tuple_idx_counter,
                            state_vars,
                            reactive_vars,
                            state_funcs,
                            component_map,
                            comp_id_hash,
                        )
                    })
                    .collect();
                NodeType::Tag(tag_name, attributes, child_nodes)
            }
            ContentType::If(if_branches, else_branch) => {
                let mut flag_mask = 0;
                let node_if_branches = if_branches
                    .into_iter()
                    .enumerate()
                    .map(|(idx, branch)| {
                        let (condition, flags) = transform_content_expr(
                            branch.condition,
                            state_vars,
                            reactive_vars,
                        );
                        flag_mask |= flags;

                        // Reset tuple index counter for branches so that they start at 0 and don't include parent nodes
                        let mut branch_tuple_idx_counter = 0;

                        NodeIfBranch {
                            condition,
                            contents: branch
                                .contents
                                .into_iter()
                                .map(|child| {
                                    Node::from_element(
                                        child,
                                        &mut branch_tuple_idx_counter,
                                        state_vars,
                                        reactive_vars,
                                        state_funcs,
                                        component_map,
                                        comp_id_hash,
                                    )
                                })
                                .collect(),
                            name: format_ident!("Branch{}", idx),
                        }
                    })
                    .collect();

                // Reset tuple index counter for else branch so that it starts at 0 and doesn't include parent nodes
                let mut else_tuple_idx_counter = 0;
                let node_else_branch = else_branch.map(|else_contents| {
                    else_contents
                        .into_iter()
                        .map(|child| {
                            Node::from_element(
                                child,
                                &mut else_tuple_idx_counter,
                                state_vars,
                                reactive_vars,
                                state_funcs,
                                component_map,
                                comp_id_hash,
                            )
                        })
                        .collect()
                });
                let enum_name = format_ident!("C{}IfBranch{}", comp_id_hash, value.id);
                NodeType::If(
                    node_if_branches,
                    node_else_branch,
                    enum_name,
                    flag_mask,
                )
            }
            ContentType::Each(iter_expr, item_name, children) => {
                let each_var = EachVar {
                    name: format_ident!("{}", item_name),
                    ty: infer_iter_item_type(&iter_expr, reactive_vars),
                };
                let (each_expr, flags) = transform_content_expr(
                    iter_expr,
                    state_vars,
                    reactive_vars,
                );

                // Reset tuple index counter for each content so that it starts at 0 and doesn't include parent nodes
                let mut each_tuple_idx_counter = 0;
                let content_nodes = children
                    .into_iter()
                    .map(|child| {
                        Node::from_element(
                            child,
                            &mut each_tuple_idx_counter,
                            state_vars,
                            reactive_vars,
                            state_funcs,
                            component_map,
                            comp_id_hash,
                        )
                    })
                    .collect();
                let frag_name = format_ident!("C{}EachFrag{}", comp_id_hash, value.id);
                NodeType::Each(each_expr, each_var, content_nodes, frag_name, flags)
            }
        };
        Node {
            id: value.id,
            tuple_idx,
            struct_field,
            content,
        }
    }

    /// Helper function to recursively get the types of all nodes that should be included in the elements tuple, in the correct order
    pub fn get_field_types(&self) -> Vec<proc_macro2::TokenStream> {
        let mut types = Vec::new();
        match &self.content {
            NodeType::Text(_) | NodeType::Expr(_, _) => {
                types.push(quote::quote! { web_sys::Text });
            }
            NodeType::Tag(_, _, children) => {
                types.push(quote::quote! { web_sys::Element });
                for child in children {
                    types.extend(child.get_field_types());
                }
            }
            NodeType::If(_, _, enum_name, _) => {
                types.push(quote::quote! { IfElement<#enum_name> });
            }
            NodeType::Each(_, _, _, frag_name, _) => {
                types.push(quote::quote! { EachElement<#frag_name> });
            }
            NodeType::Comp(comp_name, _) => {
                types.push(quote::quote! { Component<#comp_name> });
            }
        }
        types
    }

    pub fn get_fields(&self) -> Vec<proc_macro2::TokenStream> {
        let mut fields = Vec::new();
        let struct_field = &self.struct_field;
        match &self.content {
            NodeType::Tag(_, _, children) => {
                fields.push(quote::quote! { #struct_field });
                for child in children {
                    fields.extend(child.get_fields());
                }
            }
            _ => {
                // Any thing not a tag has a single field
                fields.push(quote::quote! { #struct_field });
            }
        }
        fields
    }
}
