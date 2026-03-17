use std::fmt::Debug;
use syn::{
    Ident, ItemFn, Token, parse::{Parse, ParseStream, Peek}
};

mod gen_vars;
pub use gen_vars::{StateVar, Prop};
use gen_vars::gen_vars;
mod gen_funcs;
mod gen_expr;
pub use gen_expr::*;

pub fn parse_script(js: &str) -> Result<ScriptData, crate::CompileError> {
    // Convert string to ParseStream
    Ok(syn::parse_str::<ScriptData>(js)?)
}

pub struct ComponentImport {
    pub name: String,
    pub path: String,
}

#[derive(Default)]
pub struct ScriptData {
    // $state variables
    pub props: Vec<Prop>,
    pub bindable_props: Vec<Prop>,
    pub state_vars: Vec<StateVar>,
    pub derived_vars: Vec<StateVar>,

    pub init_func: Option<ItemFn>,
    pub state_functions: Vec<ItemFn>,

    pub imports: Vec<ComponentImport>,
    pub agnostic_code: Vec<proc_macro2::TokenStream>,
}

impl Debug for ScriptData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ScriptData {{\n  props: {:?},\n  bindable_props: {:?},\n  state_vars: {:?},\n  derived_vars: {:?},\n  agnostic_code: {:?}\n}}",
            self.props,
            self.bindable_props,
            self.state_vars,
            self.derived_vars,
            self.agnostic_code
        )
    }
}

impl Parse for ScriptData {
    fn parse(mut input: ParseStream) -> syn::Result<Self> {
        let mut script_data = ScriptData::default();

        // Read contents of script block
        loop {
            if input.is_empty() {
                break;
            }

            if input.peek(Token![struct]) {
                // Check if it's the state struct
                if input.peek2(Token![$]) {
                    gen_vars(&mut input, &mut script_data)?;
                } else {
                    // Non-state struct
                    let new_struct = input.parse::<syn::ItemStruct>()?;
                    script_data.agnostic_code.push(quote::quote! { #new_struct });
                }
            } else if input.peek(Token![impl]) {
                // Check if it's an impl block for the state struct
                if input.peek2(Token![$]) {
                    let impl_block = input.parse::<syn::ItemImpl>()?;
                    script_data.agnostic_code.push(quote::quote! { #impl_block });
                } else {
                    // Non-state impl block
                    let new_impl = input.parse::<syn::ItemImpl>()?;
                    script_data.agnostic_code.push(quote::quote! { #new_impl });
                }
            } else if input.peek(Ident) {
                let fork = input.fork();
                let ident: Ident = fork.parse()?;

                if ident != "import" {
                    let item: syn::Item = input.parse()?;
                    script_data.agnostic_code.push(quote::quote! { #item });
                    continue;
                }

                // Handle component import statements
                let _: Ident = input.parse()?; // Consume 'import' keyword
                let component_name: Ident = input.parse()?;
                let _: Ident = input.parse()?; // Consume 'from' keyword
                let import_path: syn::LitStr = input.parse()?;
                let _: Token![;] = input.parse()?; // Consume ';' at end of import statement


                script_data.imports.push(ComponentImport {
                    name: component_name.to_string(),
                    path: import_path.value(),
                });
            } else {
                // Unrecognized item, consume it as tokens (includes comments)
                let item: syn::Item = input.parse()?;
                script_data.agnostic_code.push(quote::quote! { #item });
            }
        }

        let _: proc_macro2::TokenStream = input.parse()?; // Ensure code is consumed

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
