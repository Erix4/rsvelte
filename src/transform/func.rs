use syn::visit_mut::VisitMut;
use syn::{Block, ItemFn};

use crate::EVENTS;
use crate::transform::ReactiveVar;

/// Find all references to state variables and add the appropriate dereferences.
///
/// Since state functions are written and impl functions for the state struct, non-state
/// reactive variables do not need to be modified to add state accessors. However, state
/// variables still need to be dereferenced to trigger mutation tracking.
///
/// Like in expressions, dereferencing only needs to be done when the variables is not
/// destructured further (with dot notation), since that will automatically be coerced by Rust.
///
/// For example, `self.counter` would need a dereference to `(*self.counter)`,
/// but `self.counter.value` would not since the dot notation will automatically
/// dereference `counter` to access `value`.
fn transform_func_body(
    mut func: Block,
    state_vars: &Vec<ReactiveVar>,
) -> Block {
    // Use visitor pattern to traverse the function body and modify variable references
    struct FuncTransformer<'a> {
        state_vars: &'a Vec<ReactiveVar>,
    }

    impl<'a> syn::visit_mut::VisitMut for FuncTransformer<'a> {
        fn visit_expr_path_mut(&mut self, expr_path: &mut syn::ExprPath) {
            // Check if the path is just self.var
            if expr_path.path.segments.len() == 2
                && expr_path.path.segments[0].ident == "self"
            {
                let var_name = &expr_path.path.segments[1].ident;
                if self.state_vars.iter().any(|v| v.name == *var_name) {
                    // Add dereference to self.var
                    *expr_path = syn::parse_quote! { (*#expr_path) };
                }
            }

            // Continue traversing the expression path
            syn::visit_mut::visit_expr_path_mut(self, expr_path);
        }
    }

    let mut transformer = FuncTransformer { state_vars };
    transformer.visit_block_mut(&mut func);

    func
}

pub fn transform_func(func: ItemFn, state_vars: &Vec<ReactiveVar>) -> ItemFn {
    let new_block = Box::new(transform_func_body(*func.block, state_vars));

    ItemFn {
        block: new_block,
        ..func
    }
}

/// Check that an event handler function has a &mut self argument and,
/// optionally, a single event argument with one of the listed types
///
/// TODO: check that the event argument matches all calls to the event handler
/// TODO: add optional result return value
pub fn validate_event_handler_args(func: &ItemFn) -> bool {
    let mut has_self_arg = false;
    let mut has_valid_event_arg = false;

    for arg in &func.sig.inputs {
        if let syn::FnArg::Receiver(receiver) = arg {
            if receiver.reference.is_some() && receiver.mutability.is_some() {
                has_self_arg = true;
            }
        } else if let syn::FnArg::Typed(pat_type) = arg {
            let mut check_type_path = |type_path: &syn::TypePath| {
                let type_ident = &type_path.path.segments[0].ident;
                has_valid_event_arg = has_valid_event_arg
                    || EVENTS.iter().any(|(_, event_type, _)| {
                        event_type == &type_ident.to_string()
                    });
            };
            if let syn::Type::Reference(type_ref) = &*pat_type.ty {
                if let syn::Type::Path(type_path) = &*type_ref.elem {
                    check_type_path(type_path);
                }
            } else if let syn::Type::Path(type_path) = &*pat_type.ty {
                check_type_path(type_path);
            }
        }
    }

    has_self_arg && (has_valid_event_arg || func.sig.inputs.len() == 1)
}
