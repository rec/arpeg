//! Bounded simultaneous grammar preparation, lowered to ordinary index selection.

use std::collections::BTreeMap;

use crate::Selection;

pub struct Grammar {
    pub axiom: Vec<String>,
    pub rules: BTreeMap<String, Vec<String>>,
    pub terminals: BTreeMap<String, u64>,
    pub rounds: usize,
    pub rest_outside: bool,
}

impl Grammar {
    pub fn expand(&self) -> Result<Selection, &'static str> {
        if self.axiom.is_empty() || self.axiom.len() > 4096 || self.rounds > 32 {
            return Err("grammar requires 1..4096 axiom symbols and 0..32 rounds");
        }
        if self.axiom.iter().any(String::is_empty)
            || self
                .rules
                .iter()
                .any(|(k, v)| k.is_empty() || v.iter().any(String::is_empty))
            || self
                .terminals
                .iter()
                .any(|(k, v)| k.is_empty() || *v > i64::MAX as u64)
        {
            return Err("grammar requires nonempty symbols and nonnegative i64 terminal ranks");
        }
        let mut symbols: Vec<_> = self.axiom.iter().map(String::as_str).collect();
        for _ in 0..self.rounds {
            let mut expanded = Vec::new();
            for symbol in symbols {
                if let Some(replacement) = self.rules.get(symbol) {
                    if expanded.len() + replacement.len() > 4096 {
                        return Err("grammar expansion exceeds 4096 symbols");
                    }
                    expanded.extend(replacement.iter().map(String::as_str));
                } else {
                    if expanded.len() == 4096 {
                        return Err("grammar expansion exceeds 4096 symbols");
                    }
                    expanded.push(symbol);
                }
            }
            symbols = expanded;
        }
        if symbols.is_empty() {
            return Err("grammar expansion must not be empty");
        }
        let indices = symbols
            .iter()
            .map(|s| {
                self.terminals
                    .get(*s)
                    .copied()
                    .ok_or("every final grammar symbol requires a terminal rank")
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Selection::IndexPattern {
            indices,
            rest_outside: self.rest_outside,
        })
    }
}
