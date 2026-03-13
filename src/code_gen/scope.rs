/// Scope is a recursive data type passed through fragments.
/// At root, it is simply `()`, but for every each block a new
/// item is added, for instance: `((), item)`. This data type
/// has most recent added items on the top level.
///
/// Scope var name should already be sanitized when passed in,
/// for example var `a` would be `a_scope`.
pub struct ScopeData<'a> {
    name: String,
    ty: syn::Type,
    child: Option<&'a ScopeData<'a>>,
}

impl ScopeData<'_> {
    pub fn new() -> Self {
        ScopeData {
            name: String::new(),
            ty: syn::parse_quote! { () },
            child: None,
        }
    }

    pub fn wrap(&self, name: String, ty: syn::Type) -> ScopeData {
        ScopeData {
            name,
            ty,
            child: Some(self),
        }
    }

    fn has_name(&self, name: &str) -> bool {
        if self.name == name {
            true
        } else if let Some(child) = &self.child {
            child.has_name(name)
        } else {
            false
        }
    }

    pub fn get_type(&self) -> syn::Type {
        if let Some(child) = &self.child {
            let child_type = child.get_type();
            let self_type = &self.ty;
            syn::parse_quote! {
                (#child_type, #self_type)
            }
        } else {
            // Base case: no more child scopes, so type is just unit
            syn::parse_quote! { () }
        }
    }

    pub fn get_destructor(&self) -> proc_macro2::TokenStream {
        if let Some(child) = &self.child {
            let child_destructor = child.get_destructor();
            let self_name =
                syn::Ident::new(&self.name, proc_macro2::Span::call_site());
            quote::quote! {
                ( #child_destructor, #self_name )
            }
        } else {
            // Base case: no more child scopes, just discard unit
            quote::quote! { _ }
        }
    }
}

fn sanitize_scope_var_name(var_name: &str) -> String {
    format!("{}_scope", var_name)
}
