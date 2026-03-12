use syn::ItemFn;

use crate::{
    EVENTS,
    parse::html_parse::AttrType,
    transform::{Node, node::NodeType},
};

pub struct EventHandler {
    event_str_type: String,
    target_id: u32,
    run_code: proc_macro2::TokenStream,
}

pub struct ComponentPropagator {
    target_id: u32,
    accessor: proc_macro2::TokenStream,
    bind_update_code: proc_macro2::TokenStream,
}

pub enum ProcBranch {
    EventHandler(EventHandler),
    EachBlock(/*TODO,*/ Vec<ProcBranch>),
    Component(ComponentPropagator),
}

pub fn gen_event_listeners(
    cur_node: &Node,
    state_funcs: &Vec<ItemFn>,
    scoped_vars: &Vec<String>,
) -> Vec<ProcBranch> {
    let target_id = cur_node.id;
    match &cur_node.content {
        NodeType::Tag(_, attributes, children) => {
            let mut handlers = vec![];
            for attr in attributes {
                if let Some(run_code) =
                    get_run_code_from_attribute(&attr.value, state_funcs)
                {
                    let event_str_type =
                        get_event_str_type_from_attr_name(&attr.name);
                    handlers.push(ProcBranch::EventHandler(EventHandler {
                        event_str_type,
                        target_id,
                        run_code,
                    }));
                }
            }
            for child in children {
                handlers.extend(gen_event_listeners(child, state_funcs, scoped_vars));
            }
            handlers
        }
        NodeType::If(branches, else_branch, _, _) => {
            let mut handlers = vec![];
            for branch in branches {
                for child in &branch.contents {
                    handlers.extend(gen_event_listeners(child, state_funcs, scoped_vars));
                }
            }
            if let Some(else_branch) = else_branch {
                for child in else_branch {
                    handlers.extend(gen_event_listeners(child, state_funcs, scoped_vars));
                }
            }
            handlers
        }
        NodeType::Each(_, _, children, _) => {
            // Event handlers in #each blocks are allowed to access scoped variables,
            // so any handlers in an #each fragment has a nested target path which
            // assigns the scoped variables
            let mut handlers = vec![];
            for child in children {
                handlers.extend(gen_event_listeners(child, state_funcs, scoped_vars));
            }
            /*
            _ if target == id {
                let target = target_path.pop().unwrap();
                let item = self.contents.id.content[target as usize].2;
                // item specific run code here, for example:
                (|state: &mut State, )
            }
             */
            vec![ProcBranch::EachBlock(handlers)]
        }
        NodeType::Comp(comp_name, _) => {
            let accessor = get_content_accessor();
            vec![ProcBranch::Component(ComponentPropagator {
                target_id,
                accessor,
                bind_update_code: quote::quote! {
                }
            })]
        }
        _ => {
            vec![]
        }
    }
}

fn get_event_str_type_from_attr_name(attr_name: &str) -> String {
    EVENTS
        .iter()
        .find(|(event, _, _)| *event == attr_name)
        .map(|(_, _, event_str_type)| event_str_type.to_string())
        .unwrap_or_else(|| panic!("Unsupported event type {}", attr_name))
}