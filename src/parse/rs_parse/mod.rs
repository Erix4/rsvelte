use std::fmt::Debug;
use syn::{
    Token,
    parse::{Parse, ParseStream, Peek},
};

mod gen_vars;
use gen_vars::gen_vars;
pub use gen_vars::{ReactiveVar, StateVar};
mod gen_funcs;
pub use gen_funcs::FuncData;
mod gen_expr;
pub use gen_expr::*;

pub fn parse_script(js: &str) -> Result<ScriptData, crate::CompileError> {
    // Convert string to ParseStream
    Ok(syn::parse_str::<ScriptData>(js)?)
}

pub struct ScriptData {
    pub reactive_vars: Vec<ReactiveVar>,
    pub non_reactive_vars: Vec<StateVar>,
    pub init_code: Option<proc_macro2::TokenStream>,
    pub functions: Vec<FuncData>,
    pub imports: Vec<proc_macro2::TokenStream>,
}

impl Debug for ScriptData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ScriptData {{ reactive_vars: {:?}, non_reactive_vars: {:?}, functions: {:?}, imports: {:?} }}",
            self.reactive_vars,
            self.non_reactive_vars,
            self.functions
                .iter()
                .map(|func| func.code.sig.ident.to_string())
                .collect::<Vec<_>>(),
            self.imports
                .iter()
                .map(|imp| imp.to_string())
                .collect::<Vec<_>>(),
        )
    }
}

impl Parse for ScriptData {
    fn parse(mut input: ParseStream) -> syn::Result<Self> {
        let mut script_data = ScriptData {
            reactive_vars: Vec::new(),
            non_reactive_vars: Vec::new(),
            init_code: None,
            functions: Vec::new(),
            imports: Vec::new(),
        };

        let mut init_stmts = Vec::new();
        loop {
            if input.is_empty() {
                break;
            }

            if input.parse::<Token![let]>().is_ok() {
                gen_vars(
                    &mut input,
                    &mut script_data.reactive_vars,
                    &mut script_data.non_reactive_vars,
                )?;
            } else if input.peek(Token![fn]) {
                let func: FuncData = input.parse()?;
                script_data.functions.push(func);
            } else if input.peek(Token![use]) || input.peek(Token![mod]) {
                // Parse import statement
                let import_tokens = parse_to(input, Token![;], true)?;
                script_data.imports.push(import_tokens);
            } else {
                // init statement
                let init_tokens = parse_to(input, Token![;], true)?;
                init_stmts.push(init_tokens);
            }
        }

        let _: proc_macro2::TokenStream = input.parse()?; // Ensure code is consumed

        // Check that reactive arguments in functions are valid
        for func in &script_data.functions {
            for reactive_arg in &func.reactive_args {
                if !script_data
                    .reactive_vars
                    .iter()
                    .any(|rv| rv.var.name == *reactive_arg)
                {
                    return Err(syn::Error::new(
                        reactive_arg.span(),
                        format!(
                            "Reactive argument '{}' not declared as reactive state variable",
                            reactive_arg
                        ),
                    ));
                }
            }
        }

        Ok(script_data)
    }
}

fn parse_to<T: Peek>(
    input: ParseStream,
    end_token: T,
    include: bool,
) -> syn::Result<proc_macro2::TokenStream> {
    let mut tokens = proc_macro2::TokenStream::new();

    while !input.peek(end_token) {
        let tt: proc_macro2::TokenTree = input.parse()?;
        tokens.extend(std::iter::once(tt));
    }

    if include {
        let end_tt: proc_macro2::TokenTree = input.parse()?;
        tokens.extend(std::iter::once(end_tt));
    }

    Ok(tokens)
}
