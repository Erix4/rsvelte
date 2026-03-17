use quote::format_ident;
use syn::Ident;

use crate::{code_gen::scope::ScopeData, transform::NodeIfBranch};

pub fn get_new_func_if(
    enum_name: &Ident,
    branches: &Vec<NodeIfBranch>,
    else_branch: Option<&Ident>,
    scope: &ScopeData,
) -> proc_macro2::TokenStream {
    let mut if_branches = quote::quote! {};
    for (i, branch) in branches.iter().enumerate() {
        let branch_name = format_ident!("Branch{}", i);
        let fragment_name = &branch.name;
        let expr = &branch.condition;
        if_branches.extend(quote::quote! {
            if #expr { #enum_name::#branch_name(#fragment_name::new(state, scope, current_path)?) } else
        });
    }
    let else_constructor = if let Some(else_branch) = else_branch {
        quote::quote! { #else_branch::new(state, scope, current_path)? }
    } else {
        quote::quote! {}
    };
    if_branches.extend(quote::quote! {
        { #enum_name::Else( #else_constructor ) }
    });

    let scope_destructor = scope.get_destructor();

    quote::quote! {
        fn new(state: &Self::State, scope: Self::Scope<'_>, current_path: &Vec<u32>,) -> Result<Self, crate::JsValue> {
            let #scope_destructor = scope;
            Ok(
                #if_branches
            )
        }
    }
}

pub fn get_mount_func_if(
    num_if_branches: usize,
    else_branch: bool,
) -> proc_macro2::TokenStream {
    let match_st = get_pass_on_match(
        num_if_branches,
        else_branch,
        quote::quote! { mount(parent, add_method) },
    );
    quote::quote! {
        fn mount(&self, parent: &web_sys::Element, add_method: impl crate::AddMethod) -> Result<(), crate::JsValue> {
            #match_st
        }
    }
}

pub fn get_proc_func_if(
    num_if_branches: usize,
    else_branch: bool,
) -> proc_macro2::TokenStream {
    let match_st = get_pass_on_match(
        num_if_branches,
        else_branch,
        quote::quote! { proc(state, scope, e, target_path) },
    );
    quote::quote! {
        fn proc(
            &mut self,
            state: &mut Self::State,
            scope: Self::Scope<'_>,
            e: web_sys::Event,
            target_path: Vec<u32>
        ) -> Result<(), crate::JsValue> {
            #match_st
        }
    }
}

pub fn get_update_func_if(
    num_if_branches: usize,
    else_branch: bool,
) -> proc_macro2::TokenStream {
    let match_st = get_pass_on_match(
        num_if_branches,
        else_branch,
        quote::quote! { update(parent, state, scope, flags) },
    );
    quote::quote! {
        fn update(&mut self, parent: &web_sys::Element, state: &Self::State, scope: Self::Scope<'_>, flags: u64) -> Result<(), crate::JsValue> {
            #match_st
        }
    }
}

pub fn get_unmount_func_if(
    num_if_branches: usize,
    else_branch: bool,
) -> proc_macro2::TokenStream {
    let match_st = get_pass_on_match(
        num_if_branches,
        else_branch,
        quote::quote! { unmount() },
    );
    quote::quote! {
        fn unmount(&self) {
            #match_st
        }
    }
}

pub fn get_branch_changed_func_if(
    if_mask: u64,
    branches: &Vec<NodeIfBranch>,
    scope: &ScopeData,
) -> proc_macro2::TokenStream {
    let mut match_arms = Vec::new();
    let mut else_condition = Vec::new();

    for (i, branch) in branches.iter().enumerate() {
        let branch_name = format_ident!("Branch{}", i);
        let condition = &branch.condition;
        match_arms.push(quote::quote! {
            Self::#branch_name(_) if #condition => false,
        });
        else_condition.push(quote::quote! { !(#condition) });
    }
    match_arms.push(quote::quote! {
        Self::Else(_) if #(#else_condition)&&* => false,
    });

    let scope_destructor = scope.get_destructor();

    quote::quote! {
        fn branch_changed(&self, state: &Self::State, scope: Self::Scope<'_>, flags: u64) -> bool {
            let #scope_destructor = scope;
            
            if flags & #if_mask != 0 {
                match self {
                    #(#match_arms)*
                    _ => true,
                }
            } else {
                false
            }
        }
    }
}

/// Get branches of a match statement to propagate a function call
/// to the active branch of an if block fragment
fn get_pass_on_match(
    num_if_branches: usize,
    else_branch: bool,
    func: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let mut match_arms = quote::quote! {};
    for i in 0..num_if_branches {
        let branch_name = format_ident!("Branch{}", i);
        match_arms.extend(quote::quote! {
            Self::#branch_name(fragment) => fragment.#func,
        });
    }
    if else_branch {
        match_arms.extend(quote::quote! {
            Self::Else(fragment) => fragment.#func,
        });
    } else {
        match_arms.extend(quote::quote! {
            Self::Else(_) => Ok(()),
        });
    }
    quote::quote! {
        match self {
            #match_arms
        }
    }
}
