use syn::Ident;

use crate::{parse::ScriptData, transform::DerivedVar};

pub fn get_state_code_getter(
    script_data: &ScriptData,
    derived: &Vec<DerivedVar>,
) -> Box<dyn Fn(&Ident) -> proc_macro2::TokenStream> {
    let props = script_data
        .props
        .iter()
        .map(|prop| {
            let name = &prop.name;
            let ty = &prop.ty;
            quote::quote! {
                #name: #ty,
            }
        })
        .collect::<Vec<proc_macro2::TokenStream>>();

    let bindable_props = script_data
        .bindable_props
        .iter()
        .map(|prop| {
            let name = &prop.name;
            let ty = &prop.ty;
            quote::quote! {
                #name: #ty,
            }
        })
        .collect::<Vec<proc_macro2::TokenStream>>();

    let state_vars = script_data
        .state_vars
        .iter()
        .map(|var| {
            let name = &var.name;
            let ty = &var.ty;
            quote::quote! {
                #name: crate::MutateTracker<#ty>,
            }
        })
        .collect::<Vec<proc_macro2::TokenStream>>();

    let derived_vars = script_data
        .derived_vars
        .iter()
        .map(|var| {
            let name = &var.name;
            let ty = &var.ty;
            quote::quote! {
                #name: #ty,
            }
        })
        .collect::<Vec<proc_macro2::TokenStream>>();

    let user_funcs = script_data.state_functions.clone();
    let init_body = if let Some(init_func) = &script_data.init_func {
        let block = &init_func.block;
        quote::quote! {
           #block
        }
    } else {
        quote::quote! {}
    };

    let constructor = get_state_constructor(script_data);

    let derived_update_code = derived
        .iter()
        .map(|var| var.to_code())
        .collect::<Vec<proc_macro2::TokenStream>>();

    let state_struct_body = quote::quote! {
        #(#props)*
        #(#bindable_props)*

        #(#state_vars)*

        #(#derived_vars)*
    };

    let closure = move |state_type: &Ident| {
        quote::quote! {
            pub struct #state_type {
                #state_struct_body
            }

            impl crate::ComponentState for #state_type {
                fn init(&mut self) {
                    #init_body
                }

                fn new() -> Self {
                    Self {
                        #constructor
                    }
                }

                fn update_derived(&mut self) {
                    #(#derived_update_code)*
                }
            }

            impl #state_type {
                #(#user_funcs)*
            }
        }
    };

    Box::new(closure)
}

fn get_state_constructor(script_data: &ScriptData) -> proc_macro2::TokenStream {
    let prop_constructors = script_data.props.iter().map(|prop| {
        let name = &prop.name;
        if let Some(default) = &prop.default {
            let default_expr = &default;
            quote::quote! {
                #name: #default_expr,
            }
        } else {
            quote::quote! {
                #name: Default::default(),
            }
        }
    });
    let bindable_prop_constructors =
        script_data.bindable_props.iter().map(|prop| {
            let name = &prop.name;
            if let Some(default) = &prop.default {
                let default_expr = &default;
                quote::quote! {
                    #name: #default_expr,
                }
            } else {
                quote::quote! {
                    #name: Default::default(),
                }
            }
        });

    let state_var_constructors = script_data.state_vars.iter().map(|var| {
        let name = &var.name;
        let flag_pos: u32 = var.flag_pos as u32;
        let default_expr = &var.default;
        quote::quote! {
            #name: crate::MutateTracker::new(#default_expr, #flag_pos),
        }
    });

    let derived_var_constructors = script_data.derived_vars.iter().map(|var| {
        let name = &var.name;
        let default_expr = &var.default;
        quote::quote! {
            #name: #default_expr,
        }
    });

    quote::quote! {
        #(#prop_constructors)*
        #(#bindable_prop_constructors)*
        #(#state_var_constructors)*
        #(#derived_var_constructors)*
    }
}
