// SPDX-License-Identifier: MIT
//! Pins the oxidd API this crate depends on, so an upgrade that moves it fails here
//! rather than somewhere in the middle of the symbolic encoding.

use oxidd::bdd::{new_manager, BDDFunction};
use oxidd::{BooleanFunction, Manager, ManagerRef};

/// Sized for the test, not for a conversation: a handful of nodes and a small cache.
const NODE_CAPACITY: usize = 1024;
const CACHE_CAPACITY: usize = 1024;
const THREADS: u32 = 1;

#[test]
fn a_conjunction_is_true_only_when_both_are() {
    let manager_ref = new_manager(NODE_CAPACITY, CACHE_CAPACITY, THREADS);
    // Variables are declared on the manager first and then referred to by number.
    // That numbering IS the BDD variable order, which is why the symbol table has to be
    // frozen before any of this runs.
    let (x, y) = manager_ref.with_manager_exclusive(|manager| {
        let vars = manager.add_vars(2);
        let x = BDDFunction::var(manager, vars.start).unwrap();
        let y = BDDFunction::var(manager, vars.start + 1).unwrap();
        (x, y)
    });

    let both: BDDFunction = x.and(&y).unwrap();

    assert!(both.satisfiable());
    assert!(!both.valid());

    // eval is how an encoding gets checked against the explicit engine: build the
    // formula, then ask it about a concrete assignment the crawl also knows the answer
    // for.
    assert!(both.eval([(0, true), (1, true)]));
    assert!(!both.eval([(0, true), (1, false)]));
    assert!(!both.eval([(0, false), (1, true)]));
}

/// The operation the transition relation is built out of: a multiplexer on one bit.
#[test]
fn ite_selects_between_two_functions() {
    let manager_ref = new_manager(NODE_CAPACITY, CACHE_CAPACITY, THREADS);
    let (c, x, y) = manager_ref.with_manager_exclusive(|manager| {
        let vars = manager.add_vars(3);
        (
            BDDFunction::var(manager, vars.start).unwrap(),
            BDDFunction::var(manager, vars.start + 1).unwrap(),
            BDDFunction::var(manager, vars.start + 2).unwrap(),
        )
    });

    let chosen = c.ite(&x, &y).unwrap();

    assert!(chosen.eval([(0, true), (1, true), (2, false)]));
    assert!(!chosen.eval([(0, true), (1, false), (2, true)]));
    assert!(chosen.eval([(0, false), (1, false), (2, true)]));
}
