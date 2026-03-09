use crate::transform::ReactiveVar;
use syn::{parse::Parser, visit_mut::VisitMut};

/// Find all state variables in the expression, dereference them if necessary and add state accessor,
/// and extract a bitmask of which variables are used.
///
/// As an example, the {counter} expression (provided counter is a state variable),
/// would become: `*state.counter`, but {my_struct.a} would be `state.my_struct.a`.
/// There's no need to dereference if the state variable is already being accessed with dot notation.
/// Deferences are only needed on $state variables, as all others have manually set flags.
pub fn transform_content_expr(
    mut expr: syn::Expr,
    state_vars: &Vec<ReactiveVar>,
    reactive_vars: &Vec<ReactiveVar>,
) -> (syn::Expr, u64) {
    // Use a visitor to traverse the expression AST
    struct VarVisitor<'a> {
        state_vars: &'a Vec<ReactiveVar>,
        reactive_vars: &'a Vec<ReactiveVar>,
        flags: u64,
    }

    impl<'a, 'ast> syn::visit_mut::VisitMut for VarVisitor<'a> {
        fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
            if let syn::Expr::Path(path) = expr.clone() {
                if path.path.segments.len() == 1 {
                    let ident = &path.path.segments[0].ident;
                    for var in self.reactive_vars.iter() {
                        if var.name == *ident {
                            // Found a state variable, set the corresponding flag
                            self.flags |= var.flag_mask;
                            // Replace with state accessed version
                            *expr = syn::parse2(quote::quote! {
                                state.#ident
                            })
                            .unwrap();
                            break;
                        }
                    }
                    for var in self.state_vars.iter() {
                        if var.name == *ident {
                            // Found a state variable, set the corresponding flag, & add dereference
                            self.flags |= var.flag_mask;
                            // Replace with dereferenced state accessed version
                            *expr = syn::parse2(quote::quote! {
                                *state.#ident
                            })
                            .unwrap();
                            break;
                        }
                    }
                }
            }
            syn::visit_mut::visit_expr_mut(self, expr);
        }
    }

    let mut visitor = VarVisitor {
        state_vars,
        reactive_vars,
        flags: 0,
    };

    visitor.visit_expr_mut(&mut expr);

    (expr, visitor.flags)
}

/// Similar to transform_content_expr, but also keep list of which reactive variables are used
pub fn transform_derived_expr(
    expr: syn::Expr,
    state_vars: &Vec<ReactiveVar>,
    reactive_vars: &Vec<ReactiveVar>,
) -> (syn::Expr, Vec<syn::Ident>) {
    let mut used_vars = Vec::new();

    struct VarVisitor<'a> {
        state_vars: &'a Vec<ReactiveVar>,
        reactive_vars: &'a Vec<ReactiveVar>,
        used_vars: &'a mut Vec<syn::Ident>,
    }

    impl<'a, 'ast> syn::visit_mut::VisitMut for VarVisitor<'a> {
        fn visit_expr_mut(&mut self, i: &mut syn::Expr) {
            if let syn::Expr::Path(path) = i.clone() {
                if path.path.segments.len() == 1 {
                    let ident = &path.path.segments[0].ident;
                    for var in self.reactive_vars.iter() {
                        if var.name == *ident {
                            // Found a state variable, add to used vars
                            if !self.used_vars.contains(ident) {
                                self.used_vars.push(ident.clone());
                            }
                            // Replace with state accessed version (no dereference for non-state vars)
                            *i = syn::parse2(quote::quote! {
                                state.#ident
                            })
                            .unwrap();
                            break;
                        }
                    }
                    for var in self.state_vars.iter() {
                        if var.name == *ident {
                            // Found a state variable, add to used vars, & add dereference
                            if !self.used_vars.contains(ident) {
                                self.used_vars.push(ident.clone());
                            }
                            // Replace with dereferenced state accessed version
                            *i = syn::parse2(quote::quote! {
                                *state.#ident
                            })
                            .unwrap();
                            break;
                        }
                    }
                } else {
                    // Check if the first segment is a reactive variable, if so add to used vars (handles dot notation case)
                    let first_ident = &path.path.segments[0].ident;
                    for var in self.reactive_vars.iter() {
                        if var.name == *first_ident {
                            if !self.used_vars.contains(first_ident) {
                                self.used_vars.push(first_ident.clone());
                            }
                        }
                    }
                }
            }
            syn::visit_mut::visit_expr_mut(self, i);
        }
    }

    let mut visitor = VarVisitor {
        state_vars,
        reactive_vars,
        used_vars: &mut used_vars,
    };

    let mut expr_mut = expr.clone();
    visitor.visit_expr_mut(&mut expr_mut);

    (expr_mut, used_vars)
}

/// Infer the item type of an iterable expression
pub fn infer_iter_item_type(
    expr: &syn::Expr,
    reactive_vars: &[ReactiveVar],
) -> syn::Type {
    match expr {
        // Range expressions: (0..10), (0..counter), (start..=end)
        syn::Expr::Range(range) => {
            // Try to infer from the start bound first, then the end bound
            let bound_expr = range.start.as_deref().or(range.end.as_deref());
            if let Some(bound) = bound_expr {
                if let Some(ty) = infer_type_from_bound(bound, reactive_vars) {
                    return ty;
                }
            }
            // Default range type is i32
            syn::parse_str("i32").unwrap()
        }

        // Path expression: a variable name like `items`
        syn::Expr::Path(expr_path) => {
            if let Some(ident) = expr_path.path.get_ident() {
                // Check if it's a reactive variable and try to extract inner type
                if let Some(rv) =
                    reactive_vars.iter().find(|rv| rv.name == *ident)
                {
                    return extract_iter_item_type(&rv.ty);
                }
            }
            syn::parse_str("_").unwrap()
        }

        // Method call: items.iter(), items.into_iter(), items.chars(), etc.
        syn::Expr::MethodCall(method_call) => {
            let method_name = method_call.method.to_string();
            match method_name.as_str() {
                "iter" | "into_iter" => {
                    // Try to get the type from the receiver
                    if let syn::Expr::Path(expr_path) = &*method_call.receiver {
                        if let Some(ident) = expr_path.path.get_ident() {
                            if let Some(rv) = reactive_vars
                                .iter()
                                .find(|rv| rv.name == *ident)
                            {
                                return extract_iter_item_type(&rv.ty);
                            }
                        }
                    }
                    syn::parse_str("_").unwrap()
                }
                "chars" => syn::parse_str("char").unwrap(),
                "bytes" => syn::parse_str("u8").unwrap(),
                "lines" => syn::parse_str("&str").unwrap(),
                "keys" => {
                    // HashMap<K, V>.keys() -> K
                    if let syn::Expr::Path(expr_path) = &*method_call.receiver {
                        if let Some(ident) = expr_path.path.get_ident() {
                            if let Some(rv) = reactive_vars
                                .iter()
                                .find(|rv| rv.name == *ident)
                            {
                                return extract_map_key_type(&rv.ty);
                            }
                        }
                    }
                    syn::parse_str("_").unwrap()
                }
                "values" => {
                    // HashMap<K, V>.values() -> V
                    if let syn::Expr::Path(expr_path) = &*method_call.receiver {
                        if let Some(ident) = expr_path.path.get_ident() {
                            if let Some(rv) = reactive_vars
                                .iter()
                                .find(|rv| rv.name == *ident)
                            {
                                return extract_map_value_type(&rv.ty);
                            }
                        }
                    }
                    syn::parse_str("_").unwrap()
                }
                "enumerate" => {
                    // Recursively get inner type, wrap in tuple with usize
                    let inner = infer_iter_item_type(
                        &method_call.receiver,
                        reactive_vars,
                    );
                    let ty_str = format!("(usize, {})", quote::quote!(#inner));
                    syn::parse_str(&ty_str).unwrap()
                }
                _ => syn::parse_str("_").unwrap(),
            }
        }

        // Macro call: vec![1, 2, 3]
        syn::Expr::Macro(expr_macro) => {
            if expr_macro.mac.path.is_ident("vec") {
                // Try to parse first element to infer type
                let tokens = expr_macro.mac.tokens.clone();
                let parser = syn::punctuated::Punctuated::<
                    syn::Expr,
                    syn::Token![,],
                >::parse_terminated;
                if let Ok(exprs) = parser.parse2(tokens) {
                    if let Some(first) = exprs.first() {
                        if let Some(ty) =
                            infer_type_from_bound(first, reactive_vars)
                        {
                            return ty;
                        }
                    }
                }
            }
            syn::parse_str("_").unwrap()
        }

        // Array literal: [1, 2, 3]
        syn::Expr::Array(arr) => {
            if let Some(first) = arr.elems.first() {
                if let Some(ty) = infer_type_from_bound(first, reactive_vars) {
                    return ty;
                }
            }
            syn::parse_str("_").unwrap()
        }

        _ => syn::parse_str("_").unwrap(),
    }
}

/// Infer type from a single expression (used for range bounds and array elements)
fn infer_type_from_bound(
    expr: &syn::Expr,
    reactive_vars: &[ReactiveVar],
) -> Option<syn::Type> {
    match expr {
        syn::Expr::Lit(lit) => match &lit.lit {
            syn::Lit::Int(lit_int) => {
                if !lit_int.suffix().is_empty() {
                    syn::parse_str(lit_int.suffix()).ok()
                } else {
                    Some(syn::parse_str("i32").unwrap())
                }
            }
            syn::Lit::Float(lit_float) => {
                if !lit_float.suffix().is_empty() {
                    syn::parse_str(lit_float.suffix()).ok()
                } else {
                    Some(syn::parse_str("f64").unwrap())
                }
            }
            syn::Lit::Str(_) => Some(syn::parse_str("String").unwrap()),
            syn::Lit::Bool(_) => Some(syn::parse_str("bool").unwrap()),
            syn::Lit::Char(_) => Some(syn::parse_str("char").unwrap()),
            _ => None,
        },
        // Variable reference: check reactive vars for their type
        syn::Expr::Path(expr_path) => {
            if let Some(ident) = expr_path.path.get_ident() {
                if let Some(rv) =
                    reactive_vars.iter().find(|rv| rv.name == *ident)
                {
                    return Some(rv.ty.clone());
                }
            }
            None
        }
        _ => None,
    }
}

/// Extract the item type from a collection type
/// Vec<T> -> T, [T; N] -> T, HashSet<T> -> T
fn extract_iter_item_type(ty: &syn::Type) -> syn::Type {
    if let syn::Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            let type_name = segment.ident.to_string();
            match type_name.as_str() {
                "Vec" | "HashSet" | "BTreeSet" | "VecDeque" | "LinkedList" => {
                    if let syn::PathArguments::AngleBracketed(args) =
                        &segment.arguments
                    {
                        if let Some(syn::GenericArgument::Type(inner_ty)) =
                            args.args.first()
                        {
                            return inner_ty.clone();
                        }
                    }
                }
                _ => {}
            }
        }
    }
    // Array type [T; N] -> T
    if let syn::Type::Array(arr) = ty {
        return *arr.elem.clone();
    }
    // Slice type [T] -> T
    if let syn::Type::Slice(slice) = ty {
        return *slice.elem.clone();
    }
    syn::parse_str("_").unwrap()
}

/// Extract key type from HashMap<K, V> or BTreeMap<K, V>
fn extract_map_key_type(ty: &syn::Type) -> syn::Type {
    if let syn::Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            let type_name = segment.ident.to_string();
            if type_name == "HashMap" || type_name == "BTreeMap" {
                if let syn::PathArguments::AngleBracketed(args) =
                    &segment.arguments
                {
                    if let Some(syn::GenericArgument::Type(key_ty)) =
                        args.args.first()
                    {
                        return key_ty.clone();
                    }
                }
            }
        }
    }
    syn::parse_str("_").unwrap()
}

/// Extract value type from HashMap<K, V> or BTreeMap<K, V>
fn extract_map_value_type(ty: &syn::Type) -> syn::Type {
    if let syn::Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            let type_name = segment.ident.to_string();
            if type_name == "HashMap" || type_name == "BTreeMap" {
                if let syn::PathArguments::AngleBracketed(args) =
                    &segment.arguments
                {
                    let mut iter = args.args.iter();
                    iter.next(); // Skip key type
                    if let Some(syn::GenericArgument::Type(val_ty)) =
                        iter.next()
                    {
                        return val_ty.clone();
                    }
                }
            }
        }
    }
    syn::parse_str("_").unwrap()
}
