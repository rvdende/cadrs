//! Which MCAD parts are the board, keep-ins and keep-outs (PCB5.1, PCB5.4, PCB9.4).
//!
//! The rules (the course says names *contain* "board" or "PCB", e.g. "Mainboard", that keep areas
//! are named "Keep-in/Keepin" or "Keep-out/Keepout" and that extra words are allowed, e.g.
//! "Keep-out Battery"; it doesn't say whether case matters, so matching ignores case):
//!
//! 1. **Keep-out** if the name contains `keep-out`, `keepout`, `keep out` or `keep_out` at the
//!    start of a word and not followed by a letter other than a plural `s` ("Keep-out Battery",
//!    "KEEPOUT_2", "Antenna keepouts"; not "Keepouter").
//! 2. **Keep-in** likewise for `keep-in`, `keepin`, `keep in`, `keep_in` ("Keep-in", "keepin 3";
//!    not "Keeping").
//! 3. **Board** if the name contains `board` or `pcb` anywhere ("Mainboard", "PCB_1",
//!    "Board [Vision PCB]", "Motherboard"; note "Keyboard" also counts, as a substring rule
//!    must).
//! 4. Anything else is **not translated** ("Enclosure", "Battery").
//!
//! Keep names are checked before the board rule, so "Keep-out board edge" is a keep-out and
//! "PCB keep-in" a keep-in.

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

/// Parts sorted by role (indices into the input, in input order).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recognised {
    /// Every part named as a board; Sync uses the first.
    pub boards: Vec<usize>,
    pub keep_ins: Vec<usize>,
    pub keep_outs: Vec<usize>,
    /// Parts not translated (PCB5.4: the message lists them).
    pub unrecognised: Vec<usize>,
}

/// Sorts part names by [`role_of`].
pub fn recognise<S: AsRef<str>>(names: &[S]) -> Recognised {
    let mut r = Recognised::default();
    for (i, n) in names.iter().enumerate() {
        match role_of(n.as_ref()) {
            Some(PartRole::Board) => r.boards.push(i),
            Some(PartRole::KeepIn) => r.keep_ins.push(i),
            Some(PartRole::KeepOut) => r.keep_outs.push(i),
            None => r.unrecognised.push(i),
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_matching_cases() {
        use PartRole::*;
        for (name, role) in [
            ("Board", Some(Board)),
            ("Mainboard", Some(Board)),
            ("Motherboard 2", Some(Board)),
            ("PCB", Some(Board)),
            ("PCB_1", Some(Board)),
            ("pcb", Some(Board)),
            ("Board [Vision PCB]", Some(Board)),
            ("Keepout", Some(KeepOut)),
            ("Keep-out", Some(KeepOut)),
            ("KEEP-OUT", Some(KeepOut)),
            ("Keep-out Battery", Some(KeepOut)),
            ("Antenna keepout", Some(KeepOut)),
            ("keep out 2", Some(KeepOut)),
            ("Keepouts", Some(KeepOut)),
            ("Keep-out board edge", Some(KeepOut)),
            ("Keep-in", Some(KeepIn)),
            ("Keepin", Some(KeepIn)),
            ("keepin 3", Some(KeepIn)),
            ("PCB keep-in", Some(KeepIn)),
            ("Keeping bracket", None),
            ("Keepouter", None),
            ("Enclosure", None),
            ("Battery", None),
            ("Part 1", None),
        ] {
            assert_eq!(role_of(name), role, "{name}");
        }
        let r = recognise(&["Enclosure", "Mainboard", "Keep-out Battery", "Keep-in", "Antenna"]);
        assert_eq!(r.boards, vec![1]);
        assert_eq!(r.keep_outs, vec![2]);
        assert_eq!(r.keep_ins, vec![3]);
        assert_eq!(r.unrecognised, vec![0, 4]);
    }
}
