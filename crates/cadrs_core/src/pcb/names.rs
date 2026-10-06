//! Which MCAD parts are the board, keep-ins and keep-outs by name (PCB5.1, PCB5.4, PCB9.4); the
//! rules are documented in `cadrs_pcb::names`. Here in `cadrs_core` so a keep-named part can
//! default to a grey appearance (P3H.6 judge, `ex3-step9-triad-move.png`).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PartRole {
    Board,
    KeepIn,
    KeepOut,
}

fn has_keyword(name: &str, stems: &[&str]) -> bool {
    let n = name.to_lowercase();
    let b = n.as_bytes();
    for stem in stems {
        let mut from = 0;
        while let Some(i) = n[from..].find(stem) {
            let at = from + i;
            let end = at + stem.len();
            let word_start = at == 0 || !b[at - 1].is_ascii_alphabetic();
            let next = b.get(end).copied();
            let after = next.map(|c| {
                if c == b's' {
                    b.get(end + 1).is_none_or(|c| !c.is_ascii_alphabetic())
                } else {
                    !c.is_ascii_alphabetic()
                }
            });
            if word_start && after.unwrap_or(true) {
                return true;
            }
            from = at + 1;
        }
    }
    false
}

/// The role a part's name gives it (see the module docs), `None` if it isn't translated.
pub fn role_of(name: &str) -> Option<PartRole> {
    if has_keyword(name, &["keep-out", "keepout", "keep out", "keep_out"]) {
        Some(PartRole::KeepOut)
    } else if has_keyword(name, &["keep-in", "keepin", "keep in", "keep_in"]) {
        Some(PartRole::KeepIn)
    } else {
        let n = name.to_lowercase();
        (n.contains("board") || n.contains("pcb")).then_some(PartRole::Board)
    }
}

