use crate::{EVENTS, parse::html_parse::AttrType, transform::{Node, node::{NodeType, scope::ScopeData}}};

/// Generates the `new` function for fragments which are if branch or root
pub fn get_new_func_generic(
    nodes: &Vec<Node>,
    scope: &ScopeData,
    target_path: Vec<u32>,
) -> proc_macro2::TokenStream {
    get_new_func_ex(
        nodes,
        scope,
        target_path,
        quote::quote! { Self::Scope<'_> },
    )
}

/// Generates the `new` function for each block fragments
///
/// NOTE: scope passed in must already be wrapped in each block's item
pub fn get_new_func_each(
    nodes: &Vec<Node>,
    scope: &ScopeData,
    target_path: Vec<u32>,
) -> proc_macro2::TokenStream {
    get_new_func_ex(
        nodes,
        scope,
        target_path,
        quote::quote! { (Self::Scope<'_>, &Self::Item) },
    )
}

fn get_new_func_ex(
    nodes: &Vec<Node>,
    scope: &ScopeData,
    target_path: Vec<u32>,
    scope_type: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let mut struct_fields = Vec::new();
    let mut creators = Vec::new();
    let mut listeners = Vec::new();
    for node in nodes {
        struct_fields.extend(node.get_fields());
        creators.push(node.get_create_code(scope));
        listeners.extend(node.get_listeners(&target_path));
    }

    let scope_destructor = scope.get_destructor();

    quote::quote! {
        fn new(&self, state: &Self::State, scope: #scope_type) -> Result<Self, JsValue> {
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
    fn get_create_code(
        &self,
        scope: &ScopeData,
    ) -> proc_macro2::TokenStream {
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
                    children.iter().map(|child| child.get_create_code(scope));
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
                    let #struct_field = IfElement::new(state, scope)?;
                }
            }
            NodeType::Each(_, _, _, _, _) => {
                quote::quote! {
                    let #struct_field = EachElement::new(state, scope)?;
                }
            }
            NodeType::Comp(comp_name, _) => {
                quote::quote! {
                    let #struct_field = Component::<#comp_name>::new()?;
                }
                // Downward state propagation is done after creation in the apply() function
            }
        }
    }

    fn get_listeners(
        &self,
        target_path: &Vec<u32>,
    ) -> Vec<proc_macro2::TokenStream> {
        match &self.content {
            NodeType::Tag(_, attributes, children) => {
                let struct_field = &self.struct_field;
                let tuple_idx = self.tuple_idx;
                let mut listeners = Vec::new();
                for attr in attributes {
                    if let Some((_, _, js_event_str)) = EVENTS
                        .iter()
                        .find(|(svelte_event, _, _)| svelte_event == &attr.name)
                    {
                        listeners.push(quote::quote! {
                            add_listener(&self.#struct_field, #js_event_str, vec![#(#target_path),* , #tuple_idx])?;
                        });
                    }
                }
                for child in children.iter() {
                    listeners.extend(child.get_listeners(target_path));
                }
                listeners
            }
            _ => vec![],
        }
    }
}
