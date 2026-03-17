use std::collections::HashMap;

use quote::{ToTokens, format_ident};
use syn::Ident;

mod attr;
mod infer;
mod utils;

use crate::{
    parse::{
        ComponentAST,
        html_parse::{AttrType, ContentType, Element},
    },
    transform::{
        ReactiveVar,
        expr::transform_content_expr,
        node::{attr::transform_attr, infer::infer_each_expr_type, utils::*},
    },
};

pub struct NodeIfBranch {
    pub condition: syn::Expr,
    pub contents: Vec<Node>,
    pub name: Ident, // Name enum branch
}

pub struct NodeElseBranch {
    pub contents: Vec<Node>,
    pub name: Ident, // Name enum branch
}

pub struct TagAttribute {
    pub name: String,
    pub value: AttrType,
    pub flag_mask: Option<u64>, // Only set for reactive attributes
}

#[derive(Clone)]
pub struct EachVar {
    pub name: Ident,
    pub original_name: Ident,
    pub ty: syn::Type,
}

pub enum NodeType {
    Text(String),
    Expr(syn::Expr, u64), // Expression and its dirty flag mask
    Tag(String, Vec<TagAttribute>, Vec<Node>), // tag name, its attributes and its contents
    If(Vec<NodeIfBranch>, Option<NodeElseBranch>, Ident, u64), // if branches, else branch, enum name, expression dirty flag mask
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
    pub frag_field_idx: usize,
    pub struct_field: Ident,
    pub content: NodeType,
}

impl Node {
    /// Converts an Element into a Node, assigning tuple indices and mount names as needed.
    pub fn from_element(
        value: Element,
        frag_field_idx_counter: &mut usize,
        state_vars: &Vec<ReactiveVar>,
        reactive_vars: &Vec<ReactiveVar>,
        state_funcs: &Vec<&Ident>,
        component_map: &HashMap<&String, &ComponentAST>,
        comp_id_hash: &String,
        scoped_vars: &Vec<EachVar>,
    ) -> Self {
        let frag_field_idx = *frag_field_idx_counter;
        *frag_field_idx_counter += 1;
        let struct_field = format_ident!("{}", num_to_letter(frag_field_idx));
        let content = match value.content {
            ContentType::Text(txt) => NodeType::Text(txt),
            ContentType::Expr(expr) => {
                let (expr, flag_mask) =
                    transform_content_expr(expr, state_vars, reactive_vars, scoped_vars);
                NodeType::Expr(expr, flag_mask)
            }
            ContentType::Tag(tag, children) => {
                // TODO: handle binds here
                let (tag_name, attributes): (String, Vec<TagAttribute>) =
                    transform_attr(tag, state_vars, reactive_vars, state_funcs, scoped_vars);
                let attributes = add_css_scope_to_class_attr(attributes, comp_id_hash);
                if tag_name.starts_with(char::is_uppercase) {
                    // check if this is a valid component
                    if let Some(comp_ast) = component_map.get(&tag_name) {
                        // Get comp type from comp id hash
                        let comp_name = format!("C{}", comp_ast.id_hash);
                        let attributes = if let Some(props) =
                            comp_ast.script.as_ref().map(|script| &script.props)
                        {
                            let mut new_attributes = Vec::new();
                            for attr in attributes {
                                let child_flag_mask = props
                                        .iter()
                                        .find(|prop| prop.name == attr.name)
                                        .map(|prop| 1 << prop.flag_pos).expect("Component props must be defined in the component script");

                                new_attributes.push((attr, child_flag_mask));
                            }
                            new_attributes
                        } else {
                            Vec::new()
                        };
                        return Node {
                            id: value.id,
                            frag_field_idx,
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
                            frag_field_idx_counter,
                            state_vars,
                            reactive_vars,
                            state_funcs,
                            component_map,
                            comp_id_hash,
                            scoped_vars,
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
                            scoped_vars,
                        );
                        flag_mask |= flags;

                        // Reset tuple index counter for branches so that they start at 0 and don't include parent nodes
                        let mut branch_frag_field_idx_counter = 0;

                        NodeIfBranch {
                            condition,
                            contents: branch
                                .contents
                                .into_iter()
                                .map(|child| {
                                    Node::from_element(
                                        child,
                                        &mut branch_frag_field_idx_counter,
                                        state_vars,
                                        reactive_vars,
                                        state_funcs,
                                        component_map,
                                        comp_id_hash,
                                        scoped_vars,
                                    )
                                })
                                .collect(),
                            name: format_ident!(
                                "C{}If{}Branch{}",
                                comp_id_hash,
                                value.id,
                                idx
                            ),
                        }
                    })
                    .collect();

                // Reset tuple index counter for else branch so that it starts at 0 and doesn't include parent nodes
                let mut else_frag_field_idx_counter = 0;
                let node_else_branch =
                    else_branch.map(|else_contents| NodeElseBranch {
                        contents: else_contents
                            .into_iter()
                            .map(|child| {
                                Node::from_element(
                                    child,
                                    &mut else_frag_field_idx_counter,
                                    state_vars,
                                    reactive_vars,
                                    state_funcs,
                                    component_map,
                                    comp_id_hash,
                                    scoped_vars,
                                )
                            })
                            .collect(),
                        name: format_ident!(
                            "C{}If{}ElseBranch",
                            comp_id_hash,
                            value.id
                        ),
                    });
                let enum_name =
                    format_ident!("C{}If{}", comp_id_hash, value.id);
                NodeType::If(
                    node_if_branches,
                    node_else_branch,
                    enum_name,
                    flag_mask,
                )
            }
            ContentType::Each(each_expr, item_name, children) => {
                // TODO: get scoped vars
                let inferred_expr_type =
                    infer_each_expr_type(&each_expr, reactive_vars, scoped_vars);
                log::info!(
                    "Inferred type of #each expression {} is {}",
                    each_expr.to_token_stream().to_string(),
                    inferred_expr_type.to_token_stream().to_string()
                );
                let (each_expr, item_type) = each_expr_to_vec_and_item_type(
                    each_expr,
                    inferred_expr_type,
                );
                let each_var = EachVar {
                    name: format_ident!("{}_scope", item_name),
                    original_name: format_ident!("{}", item_name),
                    ty: item_type,
                };
                let (each_expr, flags) = transform_content_expr(
                    each_expr,
                    state_vars,
                    reactive_vars,
                    scoped_vars,
                );
                let mut scoped_vars = scoped_vars.clone();
                scoped_vars.push(each_var.clone());

                // Reset tuple index counter for each content so that it starts at 0 and doesn't include parent nodes
                let mut each_frag_field_idx_counter = 0;
                let content_nodes = children
                    .into_iter()
                    .map(|child| {
                        Node::from_element(
                            child,
                            &mut each_frag_field_idx_counter,
                            state_vars,
                            reactive_vars,
                            state_funcs,
                            component_map,
                            comp_id_hash,
                            &scoped_vars,
                        )
                    })
                    .collect();
                let frag_name =
                    format_ident!("C{}EachFrag{}", comp_id_hash, value.id);
                NodeType::Each(
                    each_expr,
                    each_var,
                    content_nodes,
                    frag_name,
                    flags,
                )
            }
        };
        Node {
            id: value.id,
            frag_field_idx,
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
                types.push(quote::quote! { crate::IfElement<#enum_name> });
            }
            NodeType::Each(_, _, _, frag_name, _) => {
                types.push(quote::quote! { crate::EachElement<#frag_name> });
            }
            NodeType::Comp(comp_name, _) => {
                types.push(quote::quote! { crate::Component<#comp_name> });
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

/// Take an each expression and its inferred type (array, vec, or iterable),
/// and convert the expression to be a vector and return the item type
fn each_expr_to_vec_and_item_type(
    each_expr: syn::Expr,
    expr_type: syn::Type,
) -> (syn::Expr, syn::Type) {
    match &expr_type {
        syn::Type::Array(array) => {
            let item_type = *array.elem.clone();
            let each_expr = syn::parse_quote! { #each_expr.to_vec() };
            (each_expr, item_type)
        }
        syn::Type::Path(path) => {
            // Check if iterator & collect if so
            let last_segment = path.path.segments.last().unwrap();
            if last_segment.ident == "Vec" {
                let item_type = match &last_segment.arguments {
                    syn::PathArguments::AngleBracketed(args) => {
                        if let Some(syn::GenericArgument::Type(ty)) =
                            args.args.first()
                        {
                            ty.clone()
                        } else {
                            panic!("Vec must have an element type");
                        }
                    }
                    _ => panic!("Unsupported Vec type"),
                };
                (each_expr, item_type)
            } else {
                // Assume it's an iterable and call into_iter().collect()
                let item_type =
                    syn::parse_quote! { <#expr_type as IntoIterator>::Item };
                let each_expr = syn::parse_quote! { (#each_expr).into_iter().collect::<Vec<_>>() };
                (each_expr, item_type)
            }
        }
        _ => panic!("Unsupported type for #each expression"),
    }
}

fn add_css_scope_to_class_attr(attributes: Vec<TagAttribute>, scope_id: &String) -> Vec<TagAttribute> {
    let mut new_attributes = Vec::new();
    let scope_class = format!("C{}", scope_id);
    let mut has_class_attr = false;
    for attr in attributes {
        if attr.name == "class" {
            has_class_attr = true;
            let new_value = match &attr.value {
                AttrType::Str(s) => {
                    let new_str = format!("{} {}", s, scope_class);
                    AttrType::Str(new_str)
                }
                AttrType::Expr(expr) => {
                    let new_expr = syn::parse_quote! { format!("{} {}", #expr, #scope_class) };
                    AttrType::Expr(new_expr)
                }
                _ => panic!("Unsupported class attribute type"),
            };
            new_attributes.push(TagAttribute {
                name: attr.name,
                value: new_value,
                flag_mask: attr.flag_mask,
            });
        } else {
            new_attributes.push(attr);
        }
    }
    if !has_class_attr {
        new_attributes.push(TagAttribute {
            name: "class".to_string(),
            value: AttrType::Str(scope_class),
            flag_mask: None,
        });
    }
    new_attributes
}