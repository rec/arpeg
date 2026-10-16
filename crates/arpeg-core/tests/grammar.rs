use std::collections::BTreeMap;

use arpeg_core::{Selection, grammar::Grammar};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn grammar_expansion_enforces_exact_preparation_limits() {
    let mut grammar = Grammar {
        axiom: vec!["A".into()],
        rules: BTreeMap::from([("A".into(), vec!["A".into(), "A".into()])]),
        terminals: BTreeMap::from([("A".into(), 0)]),
        rounds: 12,
        rest_outside: false,
    };
    let Selection::IndexPattern { indices, .. } = grammar.expand().unwrap() else {
        panic!("prepared index pattern");
    };
    assert_eq!(indices.len(), 4096);
    grammar.rounds = 13;
    assert!(grammar.expand().is_err());
    grammar.rules.clear();
    grammar.rounds = 32;
    assert!(grammar.expand().is_ok());
    grammar.rounds = 33;
    assert!(grammar.expand().is_err());
    grammar.rounds = 0;
    grammar.axiom = vec!["A".into(); 4097];
    assert!(grammar.expand().is_err());
}
