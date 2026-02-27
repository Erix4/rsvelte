use quote::format_ident;
use syn::Ident;

use crate::transform::{
    node::{Node, NodeType},
    utils::{get_tuple_type_for_nodes, scoped_vars_as_args, scoped_vars_as_params},
};

/// A variable declared in an #each block, which represents the current item in the iteration and can be used in expressions inside the block
#[derive(Clone)]
pub struct EachVar {
    pub name: Ident,
    pub ty: syn::Type,
}

impl EachVar {
    /// The argument for an each var can be owned, because these items are reconstructed
    /// every time the each block is updated, so we don't have to store them
    pub fn to_arg(&self) -> proc_macro2::TokenStream {
        let name = &self.name;
        let ty = &self.ty;
        quote::quote! {
            #name: #ty
        }
    }
}

pub fn get_each_element_functions(
    state_type: &syn::Ident,
    scoped_vars: Vec<EachVar>,
    node: &Node,
) -> proc_macro2::TokenStream {
    let mut functions = quote::quote! {};
    let name_root = &node.mount_name;
    let scoped_args = scoped_vars_as_args(&scoped_vars);
    let scoped_params = scoped_vars_as_params(&scoped_vars);
    let (iter_expr, each_var, contents, flags) = match &node.content {
        NodeType::Each(iter_expr, each_var, contents, flags) => {
            (iter_expr, each_var, contents, flags)
        }
        _ => panic!(
            "get_each_element_functions should only be called on each nodes"
        ),
    };
    let tuple_type = get_tuple_type_for_nodes(contents);

    // Create
    let name_root_create = format_ident!("{}_create", name_root);
    let create_func_call = format_ident!("{}_content_create", name_root);
    functions.extend(quote::quote! {
        fn #name_root_create(state: &#state_type #scoped_args) -> Result<EachElement, JsValue> {
            let window = web_sys::window().expect("no global window exists");
            let document = window.document().expect("no document on window exists");

            let content = iter_expr.map(|item| {
                let contents = #create_func_call(state)?;
                Ok((hash_item(&item), contents))
            })
            .collect::<Result<Vec<(u64, #tuple_type)>, JsValue>>()?;

            Ok(EachElement {
                comment: document.create_comment(""),
                content,
            })
        }
    });

    // Mount
    let name_root_mount = format_ident!("{}_mount", name_root);
    let mount_func_call = format_ident!("{}_content_mount", name_root);
    functions.extend(quote::quote! {
        fn #name_root_mount(parent: &web_sys::Node, frag: &EachElement<#tuple_type>) -> Result<(), JsValue> {
            parent.append_child(&frag.comment)?;
            for (_, contents) in &frag.content.iter() {
                #mount_func_call(parent, &frag.comment, contents)?;
            }
            Ok(())
        }
    });

    // Update
    let name_root_update = format_ident!("{}_update", name_root);
    let mount_func_call = format_ident!("{}_content_mount", name_root);
    let update_func_call = format_ident!("{}_content_update", name_root);
    let unmount_func_call = format_ident!("{}_content_unmount", name_root);
    let item_type = &each_var.ty;
    functions.extend(quote::quote! {
        fn #name_root_update(
            parent: &web_sys::Node,
            state: &#state_type,
            frag: &mut EachElement<#tuple_type>,
            flags: u64
            #scoped_args
        ) -> Result<(), JsValue> {
            if flags & #flags != 0 {
                frag.content = diff_each_content(
                    &frag.content,
                    #iter_expr.collect::<Vec<#item_type>>(),
                    parent,
                    frag.comment.clone(),
                    #unmount_func_call, // unmount_fn
                    |item| { // create_fn
                        #create_func_call(state #scoped_params, item)
                    },
                    #mount_func_call, // mount_fn
                )?;
            }

            for (_, contents) in &mut frag.content {
                #update_func_call(parent, state, contents, flags)?;
                // TODO: add scoped item here too (will always be same as when created)
            }

            Ok(())
        }
    });

    // Unmount
    let name_root_unmount = format_ident!("{}_unmount", name_root);
    let unmount_func_call = format_ident!("{}_content_unmount", name_root);
    functions.extend(quote::quote! {
        fn #name_root_unmount(frag: EachElement<#tuple_type>) {
            for (_, contents) in frag.content {
                #unmount_func_call(contents)?;
            }
            frag.comment.remove();
        }
    });

    functions
}
