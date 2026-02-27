use quote::format_ident;

use crate::transform::{each_block::EachVar, node::{Node, NodeType}, utils::{scoped_vars_as_args, scoped_vars_as_params}};

pub fn get_if_element_functions(state_type: &syn::Ident, scoped_vars: Vec<EachVar>, node: &Node) -> proc_macro2::TokenStream {
    let mut functions = quote::quote! {};
    let name_root = &node.mount_name;
    let scoped_args = scoped_vars_as_args(&scoped_vars);
    let scoped_params = scoped_vars_as_params(&scoped_vars);
    let (enum_name, branches, else_branch, flags) = match &node.content {
        NodeType::If(if_branches, else_branch, enum_name, flags) => (enum_name, if_branches, else_branch, flags),
        _ => panic!("get_if_element_functions should only be called on if nodes"),
    };

    // Create
    let mut create_arms = quote::quote! {};
    for (i, branch) in branches.iter().enumerate() {
        let condition = &branch.condition;
        let branch_name = &branch.name;
        let function_name = format_ident!("{}_branch_{}_create", name_root, i);
        create_arms.extend(quote::quote! {
            if #condition {
                (#enum_name::#branch_name( #function_name(state #scoped_params)? ), #i)
            }
        });

        if i < branches.len() - 1 {
            create_arms.extend(quote::quote! { else });
        }
    }
    let active_branch_idx = branches.len();
    if let Some(_) = else_branch {
        let function_name = format_ident!("{}_branch_{}_create", name_root, branches.len());
        create_arms.extend(quote::quote! {
            else {
                (#enum_name::Else( #function_name(state #scoped_params)? ), #active_branch_idx)
            }
        });
    } else {
        create_arms.extend(quote::quote! {
            else {
                (#enum_name::Else(), #active_branch_idx)
            }
        });
    }
    let name_root_create = format_ident!("{}_create", name_root);
    functions.extend(quote::quote! {
        fn #name_root_create(state: &#state_type #scoped_args) -> Result<IfElement<#enum_name>, JsValue> {
            let window = web_sys::window().expect("no global window exists");
            let document = window.document().expect("no document on window exists");

            let (content_enum, active_branch) = #create_arms;

            Ok(IfElement {
                comment: document.create_comment(""),
                active_branch,
                content_enum,
            })
        }
    });

    // Mount
    let mut mount_arms = Vec::new();
    for (i, branch) in branches.iter().enumerate() {
        let branch_name = &branch.name;
        let function_name = format_ident!("{}_branch_{}_mount", name_root, i);
        mount_arms.push(quote::quote! {
            #enum_name::#branch_name(ref contents) => {
                #function_name(parent, &frag.comment, contents)?;
            }
        });
    }
    if let Some(_) = else_branch {
        let function_name = format_ident!("{}_branch_{}_mount", name_root, branches.len());
        mount_arms.push(quote::quote! {
            #enum_name::Else(ref contents) => {
                #function_name(parent, &frag.comment, contents)?;
            }
        });
    } else {
        mount_arms.push(quote::quote! {
            _ => {}
        });
    }

    let name_root_mount = format_ident!("{}_mount", name_root);
    functions.extend(quote::quote! {
        fn #name_root_mount(parent: &web_sys::Node, frag: &IfElement<#enum_name>) -> Result<(), JsValue> {
            parent.append_child(&frag.comment)?;
            match frag.content_enum {
                #(#mount_arms)*
            }

            Ok(())
        }
    });

    // Arms for calling unmount functions
    let mut unmount_arms = Vec::new();
    for (i, branch) in branches.iter().enumerate() {
        let branch_name = &branch.name;
        let function_name = format_ident!("{}_branch_{}_unmount", name_root, i);
        unmount_arms.push(quote::quote! {
            #enum_name::#branch_name(ref contents) => {
                #function_name(contents)?;
            }
        });
    }
    if let Some(_) = else_branch {
        let function_name = format_ident!("{}_branch_{}_unmount", name_root, branches.len());
        unmount_arms.push(quote::quote! {
            #enum_name::Else(ref contents) => {
                #function_name(contents)?;
            }
        });
    } else {
        unmount_arms.push(quote::quote! {
            _ => {}
        });
    }

    // Update
    let mut remount_arms = quote::quote! {};
    for (i, branch) in branches.iter().enumerate() {
        let condition = &branch.condition;
        let branch_name = &branch.name;
        let create_func_name = format_ident!("{}_branch_{}_create", name_root, i);
        let mount_func_name = format_ident!("{}_branch_{}_mount", name_root, i);
        remount_arms.extend(quote::quote! {
            if #condition {
                let new_content = #create_func_name(state #scoped_params)?;
                #mount_func_name(parent, &frag.comment, &new_content)?;
                ( #enum_name::#branch_name(new_content), #i )
            }
        });

        if i < branches.len() - 1 {
            remount_arms.extend(quote::quote! { else });
        }
    }
    let active_branch_idx = branches.len();
    if let Some(_) = else_branch {
        let create_func_name = format_ident!("{}_branch_{}_create", name_root, branches.len());
        let mount_func_name = format_ident!("{}_branch_{}_mount", name_root, branches.len());
        remount_arms.extend(quote::quote! {
            else {
                let new_content = #create_func_name(state)?;
                #mount_func_name(parent, &frag.comment, &new_content)?;
                ( #enum_name::Else(new_content), #active_branch_idx )
            }
        });
    } else {
        remount_arms.extend(quote::quote! {
            else {
                ( #enum_name::Else(), #active_branch_idx )
            }
        });
    }

    let mut update_arms = Vec::new();
    for (i, branch) in branches.iter().enumerate() {
        let branch_name = &branch.name;
        let function_name = format_ident!("{}_branch_{}_update", name_root, i);
        update_arms.push(quote::quote! {
            #enum_name::#branch_name(ref contents) => {
                #function_name(parent, state, contents, flags)?;
            }
        });
    }
    if let Some(_) = else_branch {
        let function_name = format_ident!("{}_branch_{}_update", name_root, branches.len());
        update_arms.push(quote::quote! {
            #enum_name::Else(ref contents) => {
                #function_name(parent, state, contents, flags)?;
            }
        });
    } else {
        update_arms.push(quote::quote! {
            _ => {}
        });
    }

    let name_root_update = format_ident!("{}_update", name_root);
    functions.extend(quote::quote! {
        fn #name_root_update(parent: &web_sys::Node, state: &#state_type, frag: &mut IfElement<#enum_name>, flags: u64) -> Result<(), JsValue> {
            // Check for branch changes
            if flags & #flags != 0 {
                let active_branch = 
                if active_branch != frag.active_branch {
                    // Unmount old content
                    match frag.content_enum {
                        #(#unmount_arms)*
                    }

                    // Mount new content
                    (frag.content_enum, active_branch) = #remount_arms;
                }
            }

            // Check for changes in branch content
            match &frag.content_enum {
                #(#update_arms)*
            }
        }

        Ok(())
    });
    
    // Unmount
    let name_root_unmount = format_ident!("{}_unmount", name_root);
    functions.extend(quote::quote! {
        fn #name_root_unmount(frag: &IfElement<#enum_name>) {
            match frag.content_enum {
                #(#unmount_arms)*
            }
            frag.comment.remove();
        }
    });

    functions
}