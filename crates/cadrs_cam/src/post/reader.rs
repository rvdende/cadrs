//! Reads a posted program back: its moves, with the modal state each was written under. For
//! checking a program against what it was posted from (the posts' own tests, and the tests of
//! the crates above), not for running one: it knows the words [`super::post`] writes.

/// A move of a program read back.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadMove {
    /// `G0` (else a feed move).
    pub rapid: bool,
    /// Where it ends: X Y Z, as written (`NAN` for one not yet given).
    pub xyz: [f64; 3],
    /// The rotary axes there, by letter (those the program has named so far): the turns the
    /// axis has really made (an axis that rolls over followed the shortest way round).
    pub rotary: Vec<(char, f64)>,
    /// Its `F` word, when it has one of its own.
    pub feed: Option<f64>,
    /// Written under `G93` (its `F` is one over its minutes).
    pub inverse_time: bool,
    /// Written under tool centre point control.
    pub tcp: bool,
    /// The line it is on (from 1).
    pub line: usize,
}

impl ReadMove {
    /// The angle of the rotary axis `letter` (0 before the program names it).
    pub fn angle(&self, letter: char) -> f64 {
        self.rotary
            .iter()
            .find(|r| r.0 == letter)
            .map_or(0.0, |r| r.1)
    }
}

/// The straight moves of a program (`G0` and `G1`; arcs and canned cycles are passed over).
pub fn read(nc: &str) -> Vec<ReadMove> {
    let mut out = Vec::new();
    let mut xyz = [f64::NAN; 3];
    let mut rotary: Vec<(char, f64)> = Vec::new();
    let (mut mode, mut inverse, mut tcp) = (None::<u8>, false, false);
    // The rotary axes the program says roll over (a note at its head): their words are
    // angles 0 to 360, each reached the shortest way round.
    let rolling: Vec<char> = nc
        .lines()
        .filter_map(|l| {
            let c = l.trim_start().strip_prefix('(')?;
            let letter = c.chars().next()?;
            c[letter.len_utf8()..]
                .starts_with(" rolls over")
                .then_some(letter)
        })
        .collect();
    for (n, line) in nc.lines().enumerate() {
        let text = strip_comments(line);
        let words = words(&text);
        // A block in machine coordinates or an increment (a home retract) is no move of the
        // program's own.
        // (G codes in tenths, so that G43.4 is a whole number.)
        let code = |v: f64| (v * 10.0).round() as i64;
        let passing = words
            .iter()
            .any(|w| w.0 == 'G' && matches!(code(w.1), 530 | 280 | 910));
        let mut moved = false;
        let mut feed = None;
        for (letter, v) in &words {
            match letter {
                'G' => match code(*v) {
                    0 => mode = Some(0),
                    10 => mode = Some(1),
                    20 | 30 | 730..=890 => mode = Some(9),
                    930 => inverse = true,
                    940 => inverse = false,
                    434 | 2340 => tcp = true,
                    490 => tcp = false,
                    _ => {}
                },
                'M' if *v == 428.0 => tcp = true,
                'M' if *v == 429.0 => tcp = false,
                'X'..='Z' if !passing => {
                    xyz[(*letter as u8 - b'X') as usize] = *v;
                    moved = true;
                }
                'A'..='C' if !passing => {
                    let now = rotary.iter().find(|r| r.0 == *letter).map_or(0.0, |r| r.1);
                    let v = &if rolling.contains(letter) {
                        let mut d = (*v - now).rem_euclid(360.0);
                        if d > 180.0 {
                            d -= 360.0;
                        }
                        now + d
                    } else {
                        *v
                    };
                    match rotary.iter_mut().find(|r| r.0 == *letter) {
                        Some(r) => r.1 = *v,
                        None => rotary.push((*letter, *v)),
                    }
                    moved = true;
                }
                'F' => feed = Some(*v),
                _ => {}
            }
        }
        if passing {
            // (A retract leaves Z unknown to the program.)
            if words.iter().any(|w| w.0 == 'Z') {
                xyz[2] = f64::NAN;
            }
            continue;
        }
        if moved && let Some(m @ (0 | 1)) = mode {
            out.push(ReadMove {
                rapid: m == 0,
                xyz,
                rotary: rotary.clone(),
                feed,
                inverse_time: inverse,
                tcp,
                line: n + 1,
            });
        }
    }
    out
}

fn strip_comments(line: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in line.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = (depth - 1).max(0),
            ';' if depth == 0 => break,
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// A block's words: a letter and its number each.
fn words(text: &str) -> Vec<(char, f64)> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i].to_ascii_uppercase();
        i += 1;
        if !c.is_ascii_alphabetic() {
            continue;
        }
        let start = i;
        while i < chars.len() && (chars[i].is_ascii_digit() || matches!(chars[i], '.' | '-' | '+'))
        {
            i += 1;
        }
        let number: String = chars[start..i].iter().collect();
        if let Ok(v) = number.parse::<f64>() {
            out.push((c, v));
        }
    }
    out
}
