use syn::ItemFn;

use crate::{
    EVENTS,
    code_gen::scope::ScopeData,
    parse::html_parse::AttrType,
    transform::{Node, NodeType, TagAttribute},
    web_sys_qualify,
};

/// Generates the `proc` function for root fragments
pub fn get_proc_func_root(
    nodes: &Vec<Node>,
    state_funcs: &Vec<ItemFn>,
    scope: &ScopeData,
) -> proc_macro2::TokenStream {
    get_proc_func_ex(nodes, quote::quote! { () }, state_funcs, scope)
}

/// Generates the `proc` function for if branch fragments
pub fn get_proc_func_if_branch(
    nodes: &Vec<Node>,
    state_funcs: &Vec<ItemFn>,
    scope: &ScopeData,
) -> proc_macro2::TokenStream {
    get_proc_func_ex(
        nodes,
        quote::quote! { Self::Scope<'_> },
        state_funcs,
        scope,
    )
}

/// Generates the `proc` function for each block fragments
pub fn get_proc_func_each(
    nodes: &Vec<Node>,
    state_funcs: &Vec<ItemFn>,
    scope: &ScopeData,
) -> proc_macro2::TokenStream {
    get_proc_func_ex(
        nodes,
        quote::quote! { (Self::Scope<'_>, &Self::Item) },
        state_funcs,
        scope,
    )
}

fn get_proc_func_ex(
    nodes: &Vec<Node>,
    scope_type: proc_macro2::TokenStream,
    state_funcs: &Vec<ItemFn>,
    scope: &ScopeData,
) -> proc_macro2::TokenStream {
    let match_arms = nodes
        .iter()
        .flat_map(|node| node.get_proc_arms(state_funcs))
        .collect::<Vec<_>>();
    let scope_destructor = scope.get_destructor();

    quote::quote! {
        fn proc(
            &mut self,
            state: &mut Self::State,
            scope: #scope_type,
            e: web_sys::Event,
            mut target_path: Vec<u32>
        ) -> Result<(), crate::JsValue> {
            let target = target_path.pop().unwrap();
            let #scope_destructor = scope;

            // event handler code
            match e.type_().as_str() {
                #(#match_arms)*
                _ => {}
            }

            Ok(())
        }
    }
}

impl Node {
    fn get_proc_arms(
        &self,
        state_funcs: &Vec<ItemFn>,
    ) -> Vec<proc_macro2::TokenStream> {
        let mut code = Vec::new();
        let frag_field_idx = self.frag_field_idx as u32;
        match &self.content {
            NodeType::Tag(_, attributes, child_contents) => {
                for attr in attributes {
                    log::info!("Checking {} for event", attr.name);
                    if let Some(run_code) =
                        get_run_code_from_attribute(&attr.value, state_funcs)
                        && let Some(event_str_type) = get_js_event_str(attr)
                    {
                        log::info!("Found event");
                        code.push(quote::quote! {
                            #event_str_type if target == #frag_field_idx => {
                                #run_code
                            }
                        });
                    }
                }
                for child in child_contents {
                    let child_code = child.get_proc_arms(state_funcs);
                    code.extend(child_code);
                }
            }
            NodeType::If(_, _, _, _) | NodeType::Each(_, _, _, _, _) => {
                // Processing of #if and #each fragments is done inside functions, so we just call those functions here
                let struct_field = &self.struct_field;
                code.push(quote::quote! {
                    _ if target == #frag_field_idx => {
                        self.#struct_field.proc(state, scope, e, target_path)?;
                    }
                });
            }
            NodeType::Comp(_, props) => {
                // Proc child components, then update bindable props
                let struct_field = &self.struct_field;
                let mut bindable_updates = Vec::new();
                for (prop, child_mask) in props {
                    let prop_name = &prop.name;
                    bindable_updates.push(quote::quote! {
                        if child_bindable_flags & #child_mask != 0 {
                            *state.#prop_name = self.#struct_field.state.#prop_name;
                            // The mutate tracker will mark this dirty automatically
                        }
                    });
                }

                code.push(quote::quote! {
                    _ if target == #frag_field_idx => {
                        self.#struct_field.proc(state, scope, e, target_path)?;
                        let child_bindable_flags = crate::DIRTY_FLAGS.load(std::sync::atomic::Ordering::SeqCst);
                        crate::DIRTY_FLAGS.store(0, std::sync::atomic::Ordering::SeqCst);

                        #(#bindable_updates)*

                        // TODO: function props
                    }
                });
            }
            _ => {}
        }

        code
    }
}

fn get_run_code_from_attribute(
    attr_val: &AttrType,
    state_funcs: &Vec<ItemFn>,
) -> Option<proc_macro2::TokenStream> {
    match attr_val {
        AttrType::Closure(closure) => {
            let mut call_args = Vec::new();
            if let Some(event_type) = &closure.event_arg {
                call_args.push(
                    quote::quote! { e.dyn_into::<#event_type>().unwrap() },
                );
            }
            let closure_body = &closure.body;
            // Both state and scope are captured by the closure,
            // so we can just call it directly here without passing them as arguments
            Some(quote::quote! {
                (#closure_body)( #( #call_args ),* );
            })
        }
        AttrType::Call(func_name) => {
            let func = state_funcs
                .iter()
                .find(|f| f.sig.ident == *func_name)
                .expect("Function not found for event handler");
            let mut state_arg = false;
            let mut event_arg = false;
            let mut call_args = Vec::new();
            for input in &func.sig.inputs {
                if let syn::FnArg::Receiver(_) = input {
                    if state_arg {
                        panic!(
                            "Multiple self arguments in event handler '{}'. Event handlers can only have one `&mut self` argument.",
                            func.sig.ident
                        );
                    }
                    state_arg = true;
                    call_args.push(quote::quote! { &mut self.state });
                } else if let syn::FnArg::Typed(pat_type) = input {
                    let (type_path, real_type) = if let syn::Type::Reference(
                        type_ref,
                    ) = &*pat_type.ty
                    {
                        if let syn::Type::Path(type_path) = &*type_ref.elem {
                            (type_path, quote::quote! { type_ref })
                        } else {
                            panic!(
                                "Unsupported argument type in event handler '{}'. Event handlers must have a `&mut self` argument and optionally a single event argument of type `&Event` or `&MouseEvent` etc.",
                                func.sig.ident
                            );
                        }
                    } else if let syn::Type::Path(type_path) = &*pat_type.ty {
                        (type_path, quote::quote! { type_path })
                    } else {
                        panic!(
                            "Unsupported argument type in event handler '{}'. Event handlers must have a `&mut self` argument and optionally a single event argument of type `&Event` or `&MouseEvent` etc.",
                            func.sig.ident
                        );
                    };

                    // Check if event type
                    if is_websys_event_type(type_path) {
                        if event_arg {
                            panic!(
                                "Multiple event arguments in event handler '{}'. Event handlers can only have one event argument of type `&Event` or `&MouseEvent` etc.",
                                func.sig.ident
                            );
                        }
                        event_arg = true;
                        call_args.push(
                            quote::quote! { e.dyn_into::<#real_type>().unwrap() },
                        );
                    } else {
                        panic!(
                            "Unsupported argument type in event handler '{}'. Event handlers must have a `&mut self` argument and optionally a single event argument of type `&Event` or `&MouseEvent` etc.",
                            func.sig.ident
                        );
                    }
                } else {
                    panic!(
                        "Unsupported argument type in event handler '{}'. Event handlers must have a `&mut self` argument and optionally a single event argument of type `&Event` or `&MouseEvent` etc.",
                        func.sig.ident
                    );
                }
            }
            Some(quote::quote! {
                #func_name( #( #call_args ),* );
            })
        }
        _ => None,
    }
}

fn is_websys_event_type(ty: &syn::TypePath) -> bool {
    let type_ident = &ty.path.segments[0].ident;
    EVENTS.iter().any(|(_, event_type, _)| {
        event_type == &type_ident.to_string()
            || web_sys_qualify(event_type) == type_ident.to_string()
    })
}

fn get_js_event_str(attr: &TagAttribute) -> Option<String> {
    EVENTS
        .iter()
        .find(|(sv_event, _, _)| *sv_event == attr.name)
        .map(|(_, _, event_str_type)| event_str_type.to_string())
}
