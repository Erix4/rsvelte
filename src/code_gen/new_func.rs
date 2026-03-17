use crate::{
    EVENTS,
    code_gen::scope::ScopeData,
    parse::html_parse::AttrType,
    transform::{Node, NodeType},
};

/// Generates the `new` function for root fragments
pub fn get_new_func_root(
    nodes: &Vec<Node>,
    scope: &ScopeData,
) -> proc_macro2::TokenStream {
    get_new_func_ex(nodes, scope, quote::quote! { () })
}

/// Generates the `new` function for if branch fragments
pub fn get_new_func_if_branch(
    nodes: &Vec<Node>,
    scope: &ScopeData,
) -> proc_macro2::TokenStream {
    get_new_func_ex(nodes, scope, quote::quote! { Self::Scope<'_> })
}

/// Generates the `new` function for each block fragments
///
/// NOTE: scope passed in must already be wrapped in each block's item
pub fn get_new_func_each(
    nodes: &Vec<Node>,
    scope: &ScopeData,
) -> proc_macro2::TokenStream {
    get_new_func_ex(
        nodes,
        scope,
        quote::quote! { (Self::Scope<'_>, &Self::Item) },
    )
}

fn get_new_func_ex(
    nodes: &Vec<Node>,
    scope: &ScopeData,
    scope_type: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let mut struct_fields = Vec::new();
    let mut creators = Vec::new();
    let mut listeners = Vec::new();
    for node in nodes {
        struct_fields.extend(node.get_fields());
        creators.push(node.get_create_code());
        listeners.extend(node.get_listeners());
    }

    let scope_destructor = scope.get_destructor();

    quote::quote! {
        fn new(state: &Self::State, scope: #scope_type, current_path: &Vec<u32>) -> Result<Self, crate::JsValue> {
            let window = web_sys::window().expect("no global window exists");
            let document = window.document().expect("no document on window exists");

            let #scope_destructor = scope;

            // creators
            #(#creators)*

            // listeners
            #(#listeners)*

            // builder
            Ok(Self {
                #(#struct_fields),*
            })
        }
    }
}

impl Node {
    /// Generate the code to create this node and assign it to its mount name, for example:
    /// ```
    /// let a = document.create_element("div")?;
    /// let b = document.create_text_node("Hello")?;
    /// let c = document.create_text_node(format!("{}", some_expr))?;
    /// let d = node_4_create(&state)?;
    /// ```
    /// This is done statefully, using page state and scoped variables (for #each blocks)
    fn get_create_code(&self) -> proc_macro2::TokenStream {
        let struct_field = &self.struct_field;
        match &self.content {
            NodeType::Text(text) => {
                quote::quote! {
                    let #struct_field = document.create_text_node(#text);
                }
            }
            NodeType::Expr(expr, _) => {
                quote::quote! {
                    let #struct_field = document.create_text_node(&format!("{}", #expr));
                }
            }
            NodeType::Tag(tag_name, attributes, children) => {
                let children_code =
                    children.iter().map(|child| child.get_create_code());
                let attribute_setters = attributes.iter().filter(|attr| attr.flag_mask.is_none()).map(|attr| {
                    let attr_name = &attr.name;
                    if let Some(attr_value) = match &attr.value {
                        AttrType::Str(val) => Some(quote::quote! { #val }),
                        AttrType::Expr(expr) => Some(quote::quote! { &format!("{}", #expr) }),
                        _ => None,
                    } {
                        quote::quote! {
                            #struct_field.set_attribute(#attr_name, #attr_value)?;
                        }
                    } else {
                        quote::quote! {}
                    }
                });
                quote::quote! {
                    let #struct_field = document.create_element(#tag_name)?;
                    #(#attribute_setters)*
                    #(#children_code)*
                }
            }
            // Creation of #if and #each fragments is done inside functions
            NodeType::If(_, _, _, _) => {
                quote::quote! {
                    let #struct_field = crate::IfElement::new(state, scope, current_path)?;
                }
            }
            NodeType::Each(_, _, _, _, _) => {
                quote::quote! {
                    let #struct_field = crate::EachElement::new(state, scope, current_path)?;
                }
            }
            NodeType::Comp(comp_name, _) => {
                quote::quote! {
                    let #struct_field = crate::Component::<#comp_name>::new()?;
                }
                // Downward state propagation is done after creation in the apply() function
            }
        }
    }

    fn get_listeners(&self) -> Vec<proc_macro2::TokenStream> {
        match &self.content {
            NodeType::Tag(_, attributes, children) => {
                let struct_field = &self.struct_field;
                let frag_field_idx = self.frag_field_idx as u32;
                let mut listeners = Vec::new();
                for attr in attributes {
                    if let Some((_, _, js_event_str)) = EVENTS
                        .iter()
                        .find(|(svelte_event, _, _)| svelte_event == &attr.name)
                    {
                        listeners.push(quote::quote! {
                            crate::add_listener(&#struct_field, #js_event_str, crate::prepend_path(current_path, #frag_field_idx))?;
                        });
                    }
                }
                for child in children.iter() {
                    listeners.extend(child.get_listeners());
                }
                listeners
            }
            _ => vec![],
        }
    }
}
