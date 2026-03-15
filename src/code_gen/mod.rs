use quote::format_ident;

use crate::{
    CompileOutput, code_gen::fragment::get_all_fragment_code,
    transform::CompContext, utils::CompileError,
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
    pub root_comp: CompContext,
    pub comps: Vec<CompContext>,
}

pub fn code_gen(
    context: CodeGenContext,
) -> Result<CompileOutput, CompileError> {
    // Parse context for state.rs generation
    let mut fragment_code =
        (context.root_comp.state_code_getter)(&format_ident!("PageState"));
    fragment_code.extend(get_all_fragment_code(
        "Page".to_string(),
        &format_ident!("PageState"),
        context.root_comp.root_node,
        &context.root_comp.state_funcs,
    ));
    fragment_code.extend(context.comps.into_iter().map(|comp| {
        let mut code = (comp.state_code_getter)(&comp.state_type);
        code.extend(get_all_fragment_code(
            comp.comp_id,
            &comp.state_type,
            comp.root_node,
            &comp.state_funcs,
        ));
        code
    }));

    // Generate state.rs as tokens (to ensure valid syntax)
    let state_rs_tokens = quote::quote! {
        use crate::GenericFragment;
        use wasm_bindgen::JsCast;

        #fragment_code
    };

    let state_rs_str = state_rs_tokens.to_string();

    Ok(CompileOutput {
        state_rs: state_rs_str,
    })
}
