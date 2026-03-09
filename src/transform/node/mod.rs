use std::collections::HashMap;

use quote::format_ident;
use syn::Ident;

mod attr;
mod each_block;
mod if_block;
mod utils;

use crate::{
    parse::{
        ComponentAST,
        html_parse::{AttrType, ContentType, Element},
    },
    transform::{
        ReactiveVar,
        expr::{infer_iter_item_type, transform_content_expr},
        node::{
            attr::transform_attr,
            each_block::{EachVar, get_each_element_functions},
            if_block::get_if_element_functions,
            utils::*,
        },
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

pub enum NodeType {
    Text(String),
    Expr(syn::Expr, u64), // Expression and its dirty flag mask
    Tag(String, Vec<TagAttribute>, Vec<Node>), // tag name, its attributes and its contents
    If(Vec<NodeIfBranch>, Option<Vec<Node>>, Ident, u64), // if branches, else branch, enum name, expression dirty flag mask
    Each(syn::Expr, EachVar, Vec<Node>, u64), // iterable expression, item var, contents, expression dirty flag mask
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
    pub mount_name: Ident,
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
    ) -> Self {
        let tuple_idx = *tuple_idx_counter;
        *tuple_idx_counter += 1;
        let mount_name = format_ident!("node_{}_", value.id);
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
                            mount_name,
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
                            )
                        })
                        .collect()
                });
                let enum_name = format_ident!("{}IfBranch", mount_name);
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
                        )
                    })
                    .collect();
                NodeType::Each(each_expr, each_var, content_nodes, flags)
            }
        };
        Node {
            id: value.id,
            tuple_idx,
            mount_name,
            content,
        }
    }

    /// Generate the type of the node for the elements tuple, for example:
    /// ```
    /// (Element, Text, IfElement<(Element, Element), (Element)>, EachElement<(Element, Element), i32>)
    /// ```
    fn as_tuple_type(&self) -> proc_macro2::TokenStream {
        if self.tuple_idx != 0 {
            panic!("as_tuple_type should only be called on top level nodes");
        }

        let types = self.get_tuple_types();
        quote::quote! { (#(#types),*) }
    }

    /// Helper function to recursively get the types of all nodes that should be included in the elements tuple, in the correct order
    pub fn get_tuple_types(&self) -> Vec<proc_macro2::TokenStream> {
        let mut types = Vec::new();
        match &self.content {
            NodeType::Text(_) | NodeType::Expr(_, _) => {
                types.push(quote::quote! { web_sys::Text });
            }
            NodeType::Tag(_, _, children) => {
                types.push(quote::quote! { web_sys::Element });
                for child in children {
                    types.extend(child.get_tuple_types());
                }
            }
            NodeType::If(_, _, enum_name, _) => {
                types.push(quote::quote! { IfElement<#enum_name> });
            }
            NodeType::Each(_, each_var, contents, _) => {
                let content_tuple_type = get_tuple_type_for_nodes(contents);
                let item_type = &each_var.ty;
                types.push(quote::quote! { EachElement<#content_tuple_type, #item_type> });
            }
            NodeType::Comp(comp_name, _) => {
                types.push(quote::quote! { #comp_name });
            }
        }
        types
    }

    /// Generate constructor for tuple after all nodes have been created, for example:
    /// ```
    /// (node_1, node_2, node_3)
    /// ```
    fn as_tuple_constructor(&self) -> proc_macro2::TokenStream {
        if self.tuple_idx != 0 {
            panic!(
                "Tuple constructor should only be called on top level nodes"
            );
        }

        let items = self.get_tuple_items();
        quote::quote! { (#(#items),*) }
    }

    /// Helper function to recursively get the names of all items that should be included
    /// in the tuple constructor for this node, in the correct order
    fn get_tuple_items(&self) -> Vec<proc_macro2::TokenStream> {
        let mut items = Vec::new();
        let var_name = &self.mount_name;
        match &self.content {
            NodeType::Text(_) | NodeType::Expr(_, _) => {
                items.push(quote::quote! { #var_name });
            }
            NodeType::Tag(_, _, children) => {
                items.push(quote::quote! { #var_name });
                for child in children {
                    items.extend(child.get_tuple_items());
                }
            }
            NodeType::If(_, _, _, _) => {
                items.push(quote::quote! { #var_name });
                // The contents of an if block are stored in a single item
            }
            NodeType::Each(_, _, _, _) => {
                items.push(quote::quote! { #var_name });
                // The contents of an each block are stored in a single item
            }
            NodeType::Comp(_, _) => {
                // The component itself is the only item stored for a component node
                items.push(quote::quote! { #var_name });
            }
        }
        items
    }

    /// Generate the code to create this node and assign it to its mount name, for example:
    /// ```
    /// let node_1 = document.create_element("div")?;
    /// let node_2 = document.create_text_node("Hello")?;
    /// let node_3 = document.create_text_node(format!("{}", some_expr))?;
    /// let node_4 = node_4_create(&state)?;
    /// ```
    /// This is done statefully, using page state and scoped variables (for #each blocks)
    pub fn get_create_code(
        &self,
        scoped_vars: &Vec<EachVar>,
    ) -> proc_macro2::TokenStream {
        let var_name = &self.mount_name;
        match &self.content {
            NodeType::Text(text) => {
                quote::quote! {
                    let #var_name = document.create_text_node(#text);
                }
            }
            NodeType::Expr(expr, _) => {
                quote::quote! {
                    let #var_name = document.create_text_node(&format!("{}", #expr));
                }
            }
            NodeType::Tag(tag_name, attributes, children) => {
                let children_code = children
                    .iter()
                    .map(|child| child.get_create_code(scoped_vars));
                let attribute_setters = attributes.iter().filter(|attr| attr.flag_mask.is_none()).map(|attr| {
                    let attr_name = &attr.name;
                    if let Some(attr_value) = match &attr.value {
                        AttrType::Str(val) => Some(quote::quote! { #val }),
                        AttrType::Expr(expr) => Some(quote::quote! { &format!("{}", #expr) }),
                        _ => None,
                    } {
                        quote::quote! {
                            #var_name.set_attribute(#attr_name, #attr_value)?;
                        }
                    } else {
                        quote::quote! {}
                    }
                });
                quote::quote! {
                    let #var_name = document.create_element(#tag_name)?;
                    #(#attribute_setters)*
                    #(#children_code)*
                }
            }
            NodeType::If(_, _, _, _) | NodeType::Each(_, _, _, _) => {
                // Creation of #if and #each fragments is done inside functions
                let create_func_name =
                    format_ident!("{}_create", self.mount_name);
                let scoped_params = scoped_vars_as_params(scoped_vars);
                quote::quote! {
                    let #var_name = #create_func_name(&state #scoped_params)?;
                }
            }
            NodeType::Comp(comp_name, _) => {
                quote::quote! {
                    let #var_name = #comp_name::new()?;
                }
                // Downward state propagation is done after creation in the apply() function
            }
        }
    }

    /// Generate code to mount this node and all its children,
    /// assuming that said nodes have already been created and assigned to a contents tuple.
    ///
    /// NOTE: this does not traverse through fragments,
    /// only mounts the elements in this fragments and calls functions
    /// to mount its children.
    fn get_mount_code<F>(
        &self,
        parent: &proc_macro2::TokenStream,
        contents_len: usize,
        add_method: F,
    ) -> proc_macro2::TokenStream
    where
        F: Fn(usize) -> proc_macro2::TokenStream,
    {
        let accessor = get_content_accessor(contents_len, self.tuple_idx);
        let add_line = add_method(self.tuple_idx);
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

                let child_add_method = |tuple_idx: usize| {
                    let child_accessor =
                        get_content_accessor(contents_len, tuple_idx);
                    quote::quote! {
                        #accessor.append_child(&#child_accessor)?;
                    }
                };

                for child in child_contents {
                    let child_code = child.get_mount_code(
                        &accessor,
                        contents_len,
                        child_add_method,
                    );
                    code.extend(child_code);
                }

                code
            }
            NodeType::If(_, _, _, _) | NodeType::Each(_, _, _, _) => {
                // Mounting of #if and #each fragments is done inside functions, so we just call those functions here
                let mount_func_name =
                    format_ident!("{}_mount", self.mount_name);
                quote::quote! {
                    #mount_func_name(&#parent, &#accessor)?;
                }
            }
            NodeType::Comp(_, _) => {
                let tuple_idx = self.tuple_idx;
                quote::quote! {
                    #accessor.mount(prepend_path(&parent_path, #tuple_idx), child_append_closure(&#parent))?;
                }
            }
        }
    }

    pub fn get_unmount_code(
        &self,
        contents_len: usize,
    ) -> proc_macro2::TokenStream {
        let accessor = get_content_accessor(contents_len, self.tuple_idx);
        match &self.content {
            NodeType::If(_, _, _, _) | NodeType::Each(_, _, _, _) => {
                // Unmounting of #if and #each fragments is done inside functions, so we just call those functions here
                let unmount_func_name =
                    format_ident!("{}_unmount", self.mount_name);
                // Note that unmount functions do not return a result
                quote::quote! {
                    #unmount_func_name(&#accessor);
                }
            }
            NodeType::Comp(_, _) => {
                quote::quote! {
                    #accessor.unmount();
                }
            }
            _ => {
                // For most content types, just remove the node itself, which will also remove all children
                quote::quote! {
                    #accessor.remove();
                }
            }
        }
    }

    /// Generate the code to update this node based on changes in state, for example:
    /// ```
    /// if flag_snapshot & 1 << 1 != 0 {
    ///   self.elements.1.set_text_content(Some(&new_value));
    /// }
    /// ```
    pub fn get_update_code(
        &self,
        parent: proc_macro2::TokenStream,
        contents_len: usize,
        scoped_args: &proc_macro2::TokenStream,
    ) -> proc_macro2::TokenStream {
        let accessor = get_content_accessor(contents_len, self.tuple_idx);
        match &self.content {
            NodeType::Text(_) => {
                // Text nodes only need to be updated if they contain an expression, so no update code is needed for static text
                quote::quote! {}
            }
            NodeType::Expr(expr, flag_mask) => {
                // Expression nodes need to be updated whenever their dependencies change, so we generate code for that
                let code = quote::quote! {
                    #accessor.set_text_content(Some(&format!("{}", #expr)));
                };
                quote::quote! {
                    if flags & #flag_mask != 0 {
                        #code
                    }
                }
            }
            NodeType::Tag(_, attributes, children) => {
                // For tags, we need to combine the update code of all children
                let mut code = quote::quote! {};
                for child in children {
                    code.extend(child.get_update_code(
                        quote::quote! { #accessor },
                        contents_len,
                        scoped_args,
                    ));
                }

                // Add checks for reactive attributes
                for attr in attributes.iter() {
                    if let Some(flag_mask) = attr.flag_mask
                        && let AttrType::Expr(expr) = &attr.value
                    {
                        let attr_name = &attr.name;
                        let update_code = quote::quote! {
                            #accessor.set_attribute(#attr_name, &format!("{}", #expr))?;
                        };
                        code.extend(quote::quote! {
                            if flags & #flag_mask != 0 {
                                #update_code
                            }
                        });
                    }
                }

                code
            }
            NodeType::If(_, _, _, _) | NodeType::Each(_, _, _, _) => {
                // Updating of #if and #each fragments is done inside functions, so we just call those functions here
                let update_func_name =
                    format_ident!("{}_update", self.mount_name);
                quote::quote! {
                    #update_func_name(&#parent, &state, &mut #accessor, flags #scoped_args)?;
                }
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
                                #accessor.#prop_name = #expr;
                                DIRTY_FLAGS.fetch_or(#child_comp_mask, SeqCst);
                            }
                        });
                        all_comp_flags |= flag_mask;
                    }
                }

                quote::quote! {
                    if flag_snapshot & #all_comp_flags != 0 {
                        DIRTY_FLAGS.store(0, SeqCst);

                        #(#prop_setters)*

                        #accessor.apply()?;
                    }
                }
            }
        }
    }
}

/// Use list of nodes to generate create, mount, update, and unmount functions
///
/// This assumes the collection of nodes is that content of a #if or #each fragment
/// The signatures are (Nodes & Comments are web_sys::):
///     - Create: fn <name_root>_create(state: &State, {scoped_vars}) -> Result<{tuple_type}, JsValue>
///     - Mount: fn <name_root>_mount(parent: &Node, comment: &Comment, contents: &{tuple_type}) -> Result<(), JsValue>
///     - Update: fn <name_root>_update(parent: &Node, state: &State, contents: &{tuple_type}, flags: u64, {scoped_vars}) -> Result<(), JsValue>
///     - Unmount: fn <name_root>_unmount(contents: &{tuple_type})
///
/// The #if and #each fragment signatures are:
///    - Create: fn <name_root>_create(state: &State, {scoped_vars}) -> Result<{tuple_type}, JsValue>
///    - Mount: fn <name_root>_mount(parent: &Node, frag: &Element<{tuple_type}) -> Result<(), JsValue>
///    - Update: fn <name_root>_update(parent: &Node, state: &State, frag: &Element<{tuple_type}>, flags: u64, {scoped_vars}) -> Result<(), JsValue>
///    - Unmount: fn <name_root>_unmount(frag: &Element<{tuple_type}>)
fn get_functions(
    name_root: &str,
    state_type: &syn::Ident,
    scoped_vars: &Vec<EachVar>,
    nodes: &Vec<Node>,
) -> proc_macro2::TokenStream {
    let mut functions = quote::quote! {};
    let tuple_type = nodes.iter().map(|node| node.get_tuple_types()).flatten();
    let tuple_type = quote::quote! { (#(#tuple_type),*) };
    let scoped_args = scoped_vars_as_args(scoped_vars);

    let add_method = |tuple_idx: usize| {
        let accessor = get_content_accessor(nodes.len(), tuple_idx);
        quote::quote! {
            parent.insert_before(&#accessor, Some(comment))?;
        }
    };

    // Create
    let create_actions =
        nodes.iter().map(|node| node.get_create_code(scoped_vars));
    let mount_names = nodes.iter().map(|node| &node.mount_name);
    let name_root_create = format_ident!("{}_create", name_root);
    functions.extend(quote::quote! {
        fn #name_root_create(state: &#state_type #scoped_args) -> Result<#tuple_type, JsValue> {
            let window = web_sys::window().expect("no global window exists");
            let document = window.document().expect("no document on window exists");

            #(#create_actions)*

            Ok((#(#mount_names),*))
        }
    });

    // Mount
    let mount_actions = nodes.iter().map(|node| {
        node.get_mount_code(&quote::quote! { parent }, nodes.len(), add_method)
    });
    let name_root_mount = format_ident!("{}_mount", name_root);
    functions.extend(quote::quote! {
        fn #name_root_mount(parent: &web_sys::Node, comment: &web_sys::Comment, contents: &#tuple_type ) -> Result<(), JsValue> {
            #(#mount_actions)*
            Ok(())
        }
    });

    // Update
    let update_maps = nodes.iter().map(|node| {
        node.get_update_code(
            quote::quote! { parent },
            nodes.len(),
            &scoped_args,
        )
    });
    let update_checks = update_maps
        .fold(quote::quote! {}, |acc, code| quote::quote! { #acc #code });
    let name_root_update = format_ident!("{}_update", name_root);
    functions.extend(quote::quote! {
        fn #name_root_update(parent: &web_sys::Node, state: &#state_type, contents: &#tuple_type, flag_snapshot: u64 #scoped_args) -> Result<(), JsValue> {
            #update_checks
            Ok(())
        }
    });

    // Unmount
    let unmount_actions =
        nodes.iter().map(|node| node.get_unmount_code(nodes.len()));
    let name_root_unmount = format_ident!("{}_unmount", name_root);
    functions.extend(quote::quote! {
        fn #name_root_unmount(contents: &#tuple_type) {
            #(#unmount_actions)*
            Ok(())
        }
    });

    // Recursively generate functions for child nodes that are #if or #each blocks
    let fragments = find_all_fragments(nodes);
    for node in fragments {
        match &node.content {
            NodeType::If(if_branches, else_branch, _, _) => {
                // Get functions for if block itself
                functions.extend(get_if_element_functions(
                    state_type,
                    scoped_vars.clone(),
                    node,
                ));

                // Get functions for branches of if block
                for (i, branch) in if_branches.iter().enumerate() {
                    let branch_functions = get_functions(
                        &format!("{}_branch_{}", node.mount_name, i),
                        state_type,
                        scoped_vars,
                        &branch.contents,
                    );
                    functions.extend(branch_functions);
                }
                if let Some(else_branch) = else_branch {
                    let else_functions = get_functions(
                        &format!(
                            "{}_branch_{}",
                            node.mount_name,
                            if_branches.len()
                        ),
                        state_type,
                        scoped_vars,
                        &else_branch,
                    );
                    functions.extend(else_functions);
                }
            }
            NodeType::Each(_, each_var, contents, _) => {
                // get functions for each block itself
                functions.extend(get_each_element_functions(
                    state_type,
                    scoped_vars.clone(),
                    node,
                ));

                // get functions for each contents
                let mut new_scoped_vars = scoped_vars.clone();
                new_scoped_vars.push(each_var.clone());
                functions.extend(get_functions(
                    &format!("{}_content", node.mount_name),
                    state_type,
                    &new_scoped_vars,
                    &contents,
                ));
            }
            _ => {}
        }
    }

    functions
}
