//! Password and passphrase generator.
//!
//! Randomness comes from the caller (`below(n)` must return a uniform value
//! in `0..n`; the app passes `keepstone_crypto::random_below`), so the rules
//! here are deterministic and testable. Every pick is independent and
//! uniform; "must include a digit" is met by retrying, which keeps the
//! result uniform over all passwords that satisfy the options.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use zeroize::Zeroizing;

const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";
const SYMBOLS: &str = "!@#$%^&*-_=+?.";
/// Characters easy to confuse when read or typed.
const AMBIGUOUS: &str = "Il1O0o";

pub const MIN_LENGTH: u8 = 8;
pub const MAX_LENGTH: u8 = 64;
pub const MIN_WORDS: u8 = 3;
pub const MAX_WORDS: u8 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    /// Random characters.
    Random,
    /// Random words from the EFF list.
    Words,
}

/// Generator settings (remembered between uses).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub style: Style,
    pub length: u8,
    pub digits: bool,
    pub symbols: bool,
    pub avoid_ambiguous: bool,
    pub words: u8,
    pub separator: char,
    pub capitalize: bool,
    pub add_number: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            style: Style::Random,
            length: 20,
            digits: true,
            symbols: true,
            avoid_ambiguous: true,
            words: 5,
            separator: '-',
            capitalize: true,
            add_number: true,
        }
    }
}

impl Options {
    #[must_use]
    pub fn clamped(mut self) -> Self {
        self.length = self.length.clamp(MIN_LENGTH, MAX_LENGTH);
        self.words = self.words.clamp(MIN_WORDS, MAX_WORDS);
        if !matches!(self.separator, '-' | '.' | '_' | ' ' | ',') {
            self.separator = '-';
        }
        self
    }

    fn alphabet(&self) -> Vec<char> {
        let mut set: String = format!("{LOWER}{UPPER}");
        if self.digits {
            set.push_str(DIGITS);
        }
        if self.symbols {
            set.push_str(SYMBOLS);
        }
        set.chars()
            .filter(|c| !(self.avoid_ambiguous && AMBIGUOUS.contains(*c)))
            .collect()
    }

    /// Bits of entropy of a generated value (how hard it is to guess when
    /// the attacker knows these options).
    #[must_use]
    pub fn entropy_bits(&self) -> f64 {
        let o = self.clamped();
        match o.style {
            Style::Random => f64::from(o.length) * (o.alphabet().len() as f64).log2(),
            Style::Words => {
                let words = f64::from(o.words) * (words().len() as f64).log2();
                // The digit (10 choices) and which word gets it.
                let number = if o.add_number {
                    10f64.log2() + f64::from(o.words).log2()
                } else {
                    0.
                };
                words + number
            }
        }
    }
}

/// The EFF large word list (7,776 words, CC BY 3.0 US).
fn words() -> &'static [&'static str] {
    static WORDS: OnceLock<Vec<&'static str>> = OnceLock::new();
    WORDS.get_or_init(|| {
        include_str!("../../data/eff_large_wordlist.txt")
            .lines()
            .filter_map(|line| line.split('\t').nth(1))
            .collect()
    })
}

/// Tries before giving up on "must contain every class" (a healthy source
/// needs one or two at length 8; hitting this means it isn't random).
const MAX_TRIES: usize = 1_000;

/// A new password or passphrase, or `None` if the random source failed or
/// isn't behaving randomly. Never falls back to a predictable value.
pub fn generate(
    options: &Options,
    below: &mut dyn FnMut(u32) -> Option<u32>,
) -> Option<Zeroizing<String>> {
    let o = options.clamped();
    match o.style {
        Style::Random => random(&o, below),
        Style::Words => passphrase(&o, below),
    }
}

fn pick<T: Copy>(items: &[T], below: &mut dyn FnMut(u32) -> Option<u32>) -> Option<T> {
    let n = u32::try_from(items.len()).ok()?;
    let i = below(n)? as usize;
    // An out-of-range answer is a broken source, not something to wrap.
    items.get(i).copied()
}

fn random(o: &Options, below: &mut dyn FnMut(u32) -> Option<u32>) -> Option<Zeroizing<String>> {
    let alphabet = o.alphabet();
    let has = |s: &str, class: &str| s.chars().any(|c| class.contains(c));
    for _ in 0..MAX_TRIES {
        let mut candidate = Zeroizing::new(String::with_capacity(usize::from(o.length)));
        for _ in 0..o.length {
            candidate.push(pick(&alphabet, below)?);
        }
        let ok = has(&candidate, LOWER)
            && has(&candidate, UPPER)
            && (!o.digits || has(&candidate, DIGITS))
            && (!o.symbols || has(&candidate, SYMBOLS));
        if ok {
            return Some(candidate);
        }
    }
    None
}

fn passphrase(o: &Options, below: &mut dyn FnMut(u32) -> Option<u32>) -> Option<Zeroizing<String>> {
    let list = words();
    let mut parts: Vec<Zeroizing<String>> = Vec::with_capacity(usize::from(o.words));
    for _ in 0..o.words {
        let w = pick(list, below)?;
        parts.push(Zeroizing::new(if o.capitalize {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_default()
        } else {
            w.to_owned()
        }));
    }
    if o.add_number {
        let at = below(u32::from(o.words))? as usize;
        let digit = char::from_digit(below(10)?, 10)?;
        parts.get_mut(at)?.push(digit);
    }
    let sep = o.separator.to_string();
    Some(Zeroizing::new(
        parts
            .iter()
            .map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join(&sep),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic "random" source for tests.
    fn counter() -> impl FnMut(u32) -> Option<u32> {
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        move |n| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            Some((state % u64::from(n.max(1))) as u32)
        }
    }

    #[test]
    fn a_failing_stuck_or_broken_source_gives_nothing() {
        for style in [Style::Random, Style::Words] {
            let o = Options {
                style,
                ..Options::default()
            };
            assert!(generate(&o, &mut |_| None).is_none(), "{style:?}: failure");
            assert!(generate(&o, &mut Some).is_none(), "{style:?}: out of range");
        }
        // A stuck source (always 0) can't meet "has a digit and a symbol":
        // it must stop, not hang.
        assert!(generate(&Options::default(), &mut |_| Some(0)).is_none());
    }

    #[test]
    fn random_passwords_follow_the_options() {
        let mut rng = counter();
        let o = Options::default();
        for _ in 0..50 {
            let p = generate(&o, &mut rng).expect("generated");
            assert_eq!(p.chars().count(), 20);
            assert!(p.chars().any(|c| c.is_ascii_digit()));
            assert!(p.chars().any(|c| SYMBOLS.contains(c)));
            assert!(!p.chars().any(|c| AMBIGUOUS.contains(c)));
        }
        let letters = Options {
            digits: false,
            symbols: false,
            length: 3,
            ..o
        };
        let p = generate(&letters, &mut rng).expect("generated");
        assert_eq!(p.len(), usize::from(MIN_LENGTH), "length is clamped");
        assert!(p.chars().all(|c| c.is_ascii_alphabetic()));
    }

    #[test]
    fn passphrases_use_the_eff_list() {
        assert_eq!(words().len(), 7776);
        assert_eq!(words()[0], "abacus");
        let mut rng = counter();
        let o = Options {
            style: Style::Words,
            ..Options::default()
        };
        let p = generate(&o, &mut rng).expect("generated");
        let parts: Vec<&str> = p.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert!(
            parts
                .iter()
                .all(|w| w.chars().next().is_some_and(char::is_uppercase))
        );
        assert_eq!(p.chars().filter(char::is_ascii_digit).count(), 1);
    }

    #[test]
    fn entropy_matches_the_math() {
        let words = Options {
            style: Style::Words,
            add_number: false,
            words: 6,
            ..Options::default()
        };
        assert!((words.entropy_bits() - 6. * 7776f64.log2()).abs() < 1e-9);
        let random = Options::default();
        // 52 letters + 10 digits + 14 symbols − 6 ambiguous = 70.
        assert!((random.entropy_bits() - 20. * 70f64.log2()).abs() < 1e-9);
        assert!(random.entropy_bits() > 120.);
    }
}
