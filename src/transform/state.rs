use syn::Ident;

use crate::parse::ScriptData;

fn get_state_code(state_type: &Ident, scrip_data: &ScriptData) -> proc_macro2::TokenStream {
    let props = scrip_data.props.iter().map(|prop| {
        let name = &prop.name;
        let ty = &prop.ty;
        quote::quote! {
            #name: #ty,
        }
    });

    let bindable_props = scrip_data.bindable_props.iter().map(|prop| {
        let name = &prop.name;
        let ty = &prop.ty;
        quote::quote! {
            #name: #ty,
        }
    });

    let state_vars = scrip_data.state_vars.iter().map(|var| {
        let name = &var.name;
        let ty = &var.ty;
        quote::quote! {
            #name: crate::MutateTracker<#ty>,
        }
    });

    let derived_vars = scrip_data.derived_vars.iter().map(|var| {
        let name = &var.name;
        let ty = &var.ty;
        quote::quote! {
            #name: #ty,
        }
    });

    let user_funcs = &scrip_data.state_functions;
    let init_body = if let Some(init_func) = &scrip_data.init_func {
        let block = &init_func.block;
        quote::quote! {
           #block
        }
    } else {
        quote::quote! {}
    };

    quote::quote! {
        pub struct #state_type {
            #(#props)*
            #(#bindable_props)*

            #(#state_vars)*

            #(#derived_vars)*
        }

        impl ComponentState for #state_type {
            fn init(&mut self) {
                #init_body
            }

            // TODO: new func

            // TODO: update-derived
        }

        impl #state_type {
            #(#user_funcs)*
        }
    }
}
