use syn::ItemFn;

use crate::{
    CompileOutput,
    parse::{
        StateVar,
        html_parse::{self, AttrType},
    },
    transform::{CompContext, DerivedVar, Node},
    utils::CompileError,
};

mod fragment;
mod if_funcs;
mod mount_func;
mod new_func;
mod proc_func;
mod scope;
mod unmount_func;
mod update_func;

pub enum ElementArrayItem {
    Element,
    If(Vec<ElementArrayItem>),
    Each(Vec<ElementArrayItem>),
}

pub struct CodeGenContext {
    pub comps: Vec<CompContext>,
}

pub fn code_gen(
    context: CodeGenContext,
) -> Result<CompileOutput, CompileError> {
    // Parse context for state.rs generation

    // Generate state.rs as tokens (to ensure valid syntax)
    let state_rs_tokens = quote::quote! {
        //
    };

    let state_rs_str = state_rs_tokens.to_string();

    Ok(CompileOutput {
        state_rs: state_rs_str,
    })
}