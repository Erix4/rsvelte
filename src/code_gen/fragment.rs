use quote::format_ident;
use syn::{Ident, ItemFn};

use crate::{
    code_gen::{
        if_funcs::{
            get_branch_changed_func_if, get_mount_func_if, get_new_func_if,
            get_proc_func_if, get_unmount_func_if, get_update_func_if,
        },
        mount_func::{get_mount_func_block, get_mount_func_root},
        new_func::{
            get_new_func_each, get_new_func_if_branch, get_new_func_root,
        },
        proc_func::{
            get_proc_func_each, get_proc_func_if_branch, get_proc_func_root,
        },
        scope::{self, ScopeData},
        unmount_func::get_unmount_func,
        update_func::{
            get_update_func_block, get_update_func_each, get_update_func_root,
        },
    },
    transform::{Node, NodeElseBranch, NodeIfBranch, NodeType},
};

/// Get all fragment code for a component
///
/// Because component are basically just wrappers around a root fragment
/// with some generic code, this function generates almost all reactive code,
/// apart from state code for each component.
///
/// The different types of fragments generated are:
/// - Root fragment: base of the component, always starts with a root tag node
/// - If enum fragment: basically just a switcher between branches, used by the
///     generic #if branch struct to generate the correct branch fragment
/// - If branch fragment: contains the content of a single #if branch
/// - Each block fragment: contains the content of a #each block, as well as a
///     special function used to generate the iterable used by the generic #each block struct
///
/// Each fragment has their own trait for function definitions, but technically the
/// if branch fragment doesn't need to implement it's trait (`GenericFragment`), because
/// it's never used in a generic context.
pub fn get_all_fragment_code(
    comp_id: String,
    state_type: &Ident,
    root_node: Node,
    state_funcs: &Vec<ItemFn>,
) -> proc_macro2::TokenStream {
    let root_fragment_name = format_ident!("C{}RootFrag", comp_id);
    let root_node_vec = vec![root_node];
    let root_fragment = get_root_fragment(
        &root_fragment_name,
        state_type,
        &vec![],
        &root_node_vec,
    );

    let child_fragments = root_node_vec
        .iter()
        .flat_map(|node| {
            node.get_fragments(state_type, &ScopeData::new(), state_funcs)
        })
        .collect::<Vec<_>>();
    log::info!("Generated {} child fragments", child_fragments.len());

    quote::quote! {
        #root_fragment

        #(#child_fragments)*
    }
}

impl Node {
    fn get_fragments(
        &self,
        state_type: &Ident,
        scope: &ScopeData,
        state_funcs: &Vec<ItemFn>,
    ) -> Vec<proc_macro2::TokenStream> {
        match &self.content {
            //#tag nodes can contain fragments, so we need to check their children
            NodeType::Tag(_, _, children) => children
                .iter()
                .flat_map(|child| {
                    child.get_fragments(state_type, scope, state_funcs)
                })
                .collect(),
            //#if and #each nodes are fragments, so we generate their code and also check their children
            NodeType::If(branches, else_branch, enum_name, mask) => {
                let mut code = Vec::new();
                for branch in branches {
                    code.push(get_if_branch_fragment(
                        &branch.name,
                        &branch.contents,
                        scope,
                        state_type,
                        state_funcs,
                    ));
                }
                if let Some(else_branch) = else_branch {
                    code.push(get_if_branch_fragment(
                        &else_branch.name,
                        &else_branch.contents,
                        scope,
                        state_type,
                        state_funcs,
                    ));
                }
                code.push(get_if_enum_fragment(
                    enum_name,
                    branches,
                    else_branch,
                    *mask,
                    scope,
                    state_type,
                ));
                code
            }
            NodeType::Each(expr, each_var, nodes, struct_name, mask) => {
                let mut code = Vec::new();
                code.push(get_each_block_fragment(
                    struct_name,
                    nodes,
                    &each_var.ty,
                    &each_var.name,
                    &scope,
                    state_type,
                    *mask,
                    expr,
                    state_funcs,
                ));
                for node in nodes {
                    let mut child_code = node.get_fragments(
                        state_type,
                        &scope.wrap(each_var.name.clone(), each_var.ty.clone()),
                        state_funcs,
                    );
                    code.append(&mut child_code);
                }
                code
            }
            // Other nodes cannot contain fragments, so we just return an empty vector
            _ => Vec::new(),
        }
    }
}

fn get_root_fragment(
    frag_name: &Ident,
    state_type: &Ident,
    state_funcs: &Vec<ItemFn>,
    root_node_vec: &Vec<Node>,
) -> proc_macro2::TokenStream {
    let scope = ScopeData::new();

    let struct_decl = get_struct_declaration(frag_name, root_node_vec);
    let new_func = get_new_func_root(root_node_vec, &scope);
    let mount_func = get_mount_func_root(root_node_vec);
    let proc_func = get_proc_func_root(root_node_vec, state_funcs, &scope);
    let update_func = get_update_func_root(root_node_vec, &scope);
    let unmount_func = get_unmount_func(root_node_vec);

    quote::quote! {
        #struct_decl

        impl crate::RootFragment for #frag_name {
            type State = #state_type;

            #new_func
            #mount_func
            #proc_func
            #update_func
            #unmount_func
        }
    }
}

fn get_if_enum_fragment(
    enum_name: &Ident,
    branches: &Vec<NodeIfBranch>,
    else_branch: &Option<NodeElseBranch>,
    mask: u64,
    scope: &ScopeData,
    state_type: &Ident,
) -> proc_macro2::TokenStream {
    let scope_type = scope.get_type();

    let enum_branches = branches.iter().enumerate().map(|(i, branch)| {
        let branch_name = format_ident!("Branch{}", i);
        let branch_struct = &branch.name;
        quote::quote! { #branch_name(#branch_struct) }
    });
    let else_enum_branch = if let Some(else_branch) = else_branch {
        let else_branch_struct = &else_branch.name;
        quote::quote! { Else(#else_branch_struct) }
    } else {
        quote::quote! {}
    };

    let branch_changed_func = get_branch_changed_func_if(mask, branches, scope);
    let new_func = get_new_func_if(
        enum_name,
        branches,
        else_branch.as_ref().map(|b| &b.name),
        scope
    );
    let mount_func = get_mount_func_if(branches.len(), else_branch.is_some());
    let proc_func = get_proc_func_if(branches.len(), else_branch.is_some());
    let update_func = get_update_func_if(branches.len(), else_branch.is_some());
    let unmount_func =
        get_unmount_func_if(branches.len(), else_branch.is_some());

    quote::quote! {
        pub enum #enum_name {
            #(#enum_branches),*,
            #else_enum_branch
        }

        impl crate::IfContentTrait for #enum_name {
            type Scope<'a> = #scope_type;
            type State = #state_type;

            #branch_changed_func
            #new_func
            #mount_func
            #proc_func
            #update_func
            #unmount_func
        }
    }
}

fn get_if_branch_fragment(
    struct_name: &Ident,
    nodes: &Vec<Node>,
    scope: &ScopeData,
    state_type: &Ident,
    state_funcs: &Vec<ItemFn>,
) -> proc_macro2::TokenStream {
    let struct_decl = get_struct_declaration(struct_name, nodes);
    let scope_type = scope.get_type();

    let new_func = get_new_func_if_branch(nodes, scope);
    let mount_func = get_mount_func_block(nodes);
    let proc_func = get_proc_func_if_branch(nodes, state_funcs, scope);
    let update_func = get_update_func_block(nodes, scope);
    let unmount_func = get_unmount_func(nodes);

    quote::quote! {
        #struct_decl

        impl crate::GenericFragment for #struct_name {
            type State = #state_type;
            type Scope<'a> = #scope_type;

            #new_func
            #mount_func
            #proc_func
            #update_func
            #unmount_func
        }
    }
}

fn get_each_block_fragment(
    struct_name: &Ident,
    nodes: &Vec<Node>,
    item_type: &syn::Type,
    item_name: &Ident,
    scope: &ScopeData,
    state_type: &Ident,
    mask: u64,
    expr: &syn::Expr,
    state_funcs: &Vec<ItemFn>,
) -> proc_macro2::TokenStream {
    let struct_decl = get_struct_declaration(struct_name, nodes);
    let scope_type = scope.get_type();
    let wrapped_scope = scope.wrap(item_name.clone(), item_type.clone());

    let generate_func = get_generate_func_each(mask, expr, scope);
    let new_func = get_new_func_each(nodes, &wrapped_scope);
    let mount_func = get_mount_func_block(nodes);
    let proc_func = get_proc_func_each(nodes, state_funcs, &wrapped_scope);
    let update_func = get_update_func_each(nodes, &wrapped_scope);
    let unmount_func = get_unmount_func(nodes);

    quote::quote! {
        #struct_decl

        impl crate::EachContentTrait for #struct_name {
            type Item = #item_type;
            type Scope<'a> = #scope_type;
            type State = #state_type;

            #generate_func
            #new_func
            #mount_func
            #proc_func
            #update_func
            #unmount_func
        }
    }
}

fn get_struct_declaration(
    frag_name: &Ident,
    nodes: &Vec<Node>,
) -> proc_macro2::TokenStream {
    let struct_fields = nodes.iter().map(|node| node.get_fields()).flatten();
    let struct_types =
        nodes.iter().map(|node| node.get_field_types()).flatten();
    let struct_field_declarations = struct_fields
        .zip(struct_types)
        .map(|(field, ty)| quote::quote! { #field: #ty })
        .collect::<Vec<_>>();

    quote::quote! {
        pub struct #frag_name {
            #(#struct_field_declarations),*
        }
    }
}

fn get_generate_func_each(
    mask: u64,
    expr: &syn::Expr,
    scope: &ScopeData,
) -> proc_macro2::TokenStream {
    let scope_destructor = scope.get_destructor();

    let flag_check = if mask != 0 {
        quote::quote! { flags & #mask != 0 }
    } else {
        quote::quote! { flags == u64::MAX }
    };

    quote::quote! {
        fn generate(state: &Self::State, scope: Self::Scope<'_>, flags: u64) -> Option<Vec<Self::Item>> {
            let #scope_destructor = scope;
            if #flag_check {
                Some(#expr)
            } else {
                None
            }
        }
    }
}
