use std::collections::HashMap;

use syn::Ident;

use crate::{
    parse::StateVar,
    transform::{ReactiveVar, expr::transform_derived_expr},
};

#[derive(Clone)]
pub struct DerivedVar {
    data: StateVar,
    dependents: Vec<Ident>,
    update_mask: u64,
    visted_as_parent: bool,
    visited: bool,
}

impl DerivedVar {
    /// Transforms a StateVar into a DerivedVar by analyzing its default expression to find dependencies on reactive variables.
    ///
    /// NOTE: reactive_vars should include derived variables
    fn from_state_var(
        var: StateVar,
        state_vars: &Vec<ReactiveVar>,
        reactive_vars: &Vec<ReactiveVar>,
    ) -> Self {
        let (revised_expr, dependents) =
            transform_derived_expr(var.default, state_vars, reactive_vars);

        let update_mask = reactive_vars
            .iter()
            .filter(|v| dependents.contains(&v.name))
            .fold(0, |accum, v| accum | v.flag_mask);

        DerivedVar {
            data: StateVar {
                default: revised_expr,
                ty: var.ty,
                name: var.name,
                flag_pos: var.flag_pos,
            },
            dependents,
            update_mask,
            visted_as_parent: false,
            visited: false,
        }
    }

    pub fn to_code(&self) -> proc_macro2::TokenStream {
        let name = &self.data.name;
        let default = &self.data.default;
        let update_mask = self.update_mask;
        let var_mask: u64 = 1 << self.data.flag_pos;

        quote::quote! {
            if crate::DIRTY_FLAGS.load(std::sync::atomic::Ordering::SeqCst) & #update_mask != 0 {
                self.#name = #default;
                crate::DIRTY_FLAGS.fetch_or(#var_mask, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }
}

/// Builds an ordered list of derived variables such that if var A depends on var B, then B will be updated before A.
/// This is done by performing a depth first topological sort on the derived variable dependency graph.
pub fn build_derived_order(
    derived_vars: Vec<StateVar>,
    state_vars: &Vec<ReactiveVar>,
    reactive_vars: &Vec<ReactiveVar>,
) -> Vec<DerivedVar> {
    let mut derived_vars: Vec<DerivedVar> = derived_vars
        .into_iter()
        .map(|var| DerivedVar::from_state_var(var, state_vars, reactive_vars))
        .collect();

    /// Depth first topological sort
    ///
    /// Resolves derived var dependencies by ordering vars such that
    /// if var A depends on var B, then B will be updated before A
    fn visit(
        var_idx: usize,
        ordered_vars: &mut Vec<usize>,
        derived_vars: &mut Vec<DerivedVar>,
    ) {
        if derived_vars[var_idx].visited {
            return;
        }
        if derived_vars[var_idx].visted_as_parent {
            panic!("Cyclic dependency detected in derived variables");
        }

        // Find all vars that this var is dependent on and visit them first
        let mut i = 0;
        while i < derived_vars.len() {
            if derived_vars[var_idx]
                .dependents
                .contains(&derived_vars[i].data.name)
            {
                derived_vars[i].visted_as_parent = true;
                visit(i, ordered_vars, derived_vars);
            } else {
                i += 1;
            }
        }

        ordered_vars.push(var_idx);
    }

    let mut ordered_vars = Vec::new();
    for i in 0..derived_vars.len() {
        visit(i, &mut ordered_vars, &mut derived_vars);
    }

    // ordered_vars is sorted source indexes
    // We want to return in order of destination indexes, so we need to reverse the mapping

    let dest_sorted = ordered_vars
        .into_iter()
        .enumerate()
        .map(|(dest_idx, source_idx)| (source_idx, dest_idx))
        .collect::<HashMap<usize, usize>>();

    let mut owned_ordered = vec![None; derived_vars.len()];
    for (source_idx, var_to_move) in derived_vars.drain(..).enumerate() {
        let dest_idx = dest_sorted.get(&source_idx).unwrap();
        owned_ordered[*dest_idx] = Some(var_to_move);
    }

    owned_ordered
        .into_iter()
        .map(|var_opt| var_opt.unwrap())
        .collect()
}
