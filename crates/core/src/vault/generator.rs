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

/// A new password or passphrase.
pub fn generate(options: &Options, below: &mut dyn FnMut(u32) -> u32) -> Zeroizing<String> {
    let o = options.clamped();
    match o.style {
        Style::Random => random(&o, below),
        Style::Words => passphrase(&o, below),
    }
}

fn pick<T: Copy>(items: &[T], below: &mut dyn FnMut(u32) -> u32) -> T {
    let n = u32::try_from(items.len()).unwrap_or(u32::MAX);
    items[below(n) as usize % items.len()]
}

fn random(o: &Options, below: &mut dyn FnMut(u32) -> u32) -> Zeroizing<String> {
    let alphabet = o.alphabet();
    let has = |s: &str, class: &str| s.chars().any(|c| class.contains(c));
    loop {
        let candidate: Zeroizing<String> =
            Zeroizing::new((0..o.length).map(|_| pick(&alphabet, below)).collect());
        let ok = has(&candidate, LOWER)
            && has(&candidate, UPPER)
            && (!o.digits || has(&candidate, DIGITS))
            && (!o.symbols || has(&candidate, SYMBOLS));
        if ok {
            return candidate;
        }
    }
}

fn passphrase(o: &Options, below: &mut dyn FnMut(u32) -> u32) -> Zeroizing<String> {
    let list = words();
    let mut parts: Vec<Zeroizing<String>> = (0..o.words)
        .map(|_| {
            let w = pick(list, below);
            Zeroizing::new(if o.capitalize {
                let mut c = w.chars();
                c.next()
                    .map(|f| f.to_uppercase().chain(c).collect())
                    .unwrap_or_default()
            } else {
                w.to_owned()
            })
        })
        .collect();
    if o.add_number {
        let at = below(u32::from(o.words)) as usize % parts.len();
        let digit = char::from(b'0' + u8::try_from(below(10) % 10).unwrap_or(0));
        parts[at].push(digit);
    }
    let sep = o.separator.to_string();
    Zeroizing::new(
        parts
            .iter()
            .map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join(&sep),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic "random" source for tests.
    fn counter() -> impl FnMut(u32) -> u32 {
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        move |n| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % u64::from(n.max(1))) as u32
        }
    }

    #[test]
    fn random_passwords_follow_the_options() {
        let mut rng = counter();
        let o = Options::default();
        for _ in 0..50 {
            let p = generate(&o, &mut rng);
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
        let p = generate(&letters, &mut rng);
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
        let p = generate(&o, &mut rng);
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
