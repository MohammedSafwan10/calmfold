//! Password strength and health, computed locally (zxcvbn, ADR 0005).
//! Nothing leaves the device: there is no breach lookup.

use std::collections::HashMap;

use jiff::{Span, Timestamp};

use crate::id::VaultItemId;
use crate::vault::{ItemKind, VaultItem};

/// zxcvbn's 0–4 score with words for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Strength {
    VeryWeak,
    Weak,
    Fair,
    Strong,
    VeryStrong,
}

impl Strength {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::VeryWeak => "Very weak",
            Self::Weak => "Weak",
            Self::Fair => "Fair",
            Self::Strong => "Strong",
            Self::VeryStrong => "Very strong",
        }
    }

    /// 0..=4, for meters.
    #[must_use]
    pub fn level(self) -> u8 {
        self as u8
    }
}

/// How strong `password` is, and one short hint to improve it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Estimate {
    pub strength: Strength,
    /// "Centuries", "3 hours" (offline attack, slow hash).
    pub crack_time: String,
    pub hint: Option<String>,
}

/// Estimates strength. `context` are words an attacker would try first
/// (the item title, username, site).
#[must_use]
pub fn estimate(password: &str, context: &[&str]) -> Estimate {
    let e = zxcvbn::zxcvbn(password, context);
    let strength = match u8::from(e.score()) {
        0 => Strength::VeryWeak,
        1 => Strength::Weak,
        2 => Strength::Fair,
        3 => Strength::Strong,
        _ => Strength::VeryStrong,
    };
    let hint = e.feedback().and_then(|f| {
        f.warning()
            .map(|w| w.to_string())
            .or_else(|| f.suggestions().first().map(ToString::to_string))
    });
    Estimate {
        strength,
        crack_time: e
            .crack_times()
            .offline_slow_hashing_1e4_per_second()
            .to_string(),
        hint: hint.filter(|h| !h.is_empty()),
    }
}

/// Logins with a password older than this count as "old".
pub const OLD_AFTER_DAYS: i64 = 365;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub weak: Vec<VaultItemId>,
    /// Groups of items sharing one password (each group ≥ 2).
    pub reused: Vec<Vec<VaultItemId>>,
    pub old: Vec<VaultItemId>,
    /// Logins with a password (what the score is out of).
    pub checked: usize,
}

impl Report {
    /// Share of checked logins with no issue, 0..=100.
    #[must_use]
    pub fn score(&self) -> u8 {
        if self.checked == 0 {
            return 100;
        }
        let mut flagged: Vec<VaultItemId> = self.weak.clone();
        flagged.extend(self.reused.iter().flatten().copied());
        flagged.extend(self.old.iter().copied());
        flagged.sort_unstable();
        flagged.dedup();
        let ok = self.checked.saturating_sub(flagged.len());
        u8::try_from(ok * 100 / self.checked).unwrap_or(100)
    }

    #[must_use]
    pub fn issues_for(&self, id: VaultItemId) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.weak.contains(&id) {
            out.push("Weak");
        }
        if self.reused.iter().any(|g| g.contains(&id)) {
            out.push("Reused");
        }
        if self.old.contains(&id) {
            out.push("Old");
        }
        out
    }
}

/// Checks every live login with a password. `strength_of` lets the caller
/// cache estimates (zxcvbn takes ~1 ms per password).
pub fn report(
    items: &[&VaultItem],
    now: Timestamp,
    strength_of: &mut dyn FnMut(&VaultItem) -> Strength,
) -> Report {
    let mut r = Report::default();
    let mut by_password: HashMap<&str, Vec<VaultItemId>> = HashMap::new();
    let cutoff = now
        .checked_sub(Span::new().hours(24 * OLD_AFTER_DAYS))
        .unwrap_or(now);
    for item in items {
        let f = item.fields();
        if item.is_trashed() || f.kind != ItemKind::Login || f.password.is_empty() {
            continue;
        }
        r.checked += 1;
        if strength_of(item) <= Strength::Weak {
            r.weak.push(item.id());
        }
        if item.password_changed_at() < cutoff {
            r.old.push(item.id());
        }
        by_password
            .entry(f.password.as_str())
            .or_default()
            .push(item.id());
    }
    let mut reused: Vec<Vec<VaultItemId>> =
        by_password.into_values().filter(|g| g.len() > 1).collect();
    reused.sort();
    r.reused = reused;
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::ItemFields;

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_second(secs).expect("ts")
    }

    fn login(title: &str, password: &str, when: Timestamp) -> VaultItem {
        let f = ItemFields {
            title: title.into(),
            password: password.into(),
            ..ItemFields::new(ItemKind::Login)
        };
        VaultItem::new(&f, when).expect("item")
    }

    #[test]
    fn estimates_rank_obvious_cases() {
        assert!(estimate("password", &[]).strength <= Strength::Weak);
        assert!(estimate("github2024", &["github"]).strength <= Strength::Fair);
        let strong = estimate("Correct-Horse-Battery-Staple-9", &[]);
        assert!(strong.strength >= Strength::Strong);
        assert!(!strong.crack_time.is_empty());
    }

    #[test]
    fn report_flags_weak_reused_and_old() {
        let now = at(1_800_000_000);
        let long_ago = at(1_700_000_000);
        let a = login("A", "same-Strong-pass-77!", now);
        let b = login("B", "same-Strong-pass-77!", now);
        let weak = login("C", "123456", now);
        let old = login("D", "Unique-Old-pass-91!", long_ago);
        let note = VaultItem::new(
            &ItemFields {
                title: "N".into(),
                ..ItemFields::new(ItemKind::Note)
            },
            now,
        )
        .expect("note");
        let items = [&a, &b, &weak, &old, &note];
        let r = report(&items, now, &mut |i| {
            estimate(&i.fields().password, &[]).strength
        });
        assert_eq!(r.checked, 4);
        assert_eq!(r.weak, [weak.id()]);
        assert_eq!(r.reused.len(), 1);
        assert_eq!(r.reused[0].len(), 2);
        assert_eq!(r.old, [old.id()]);
        assert_eq!(r.score(), 0, "every login has an issue");
        assert_eq!(r.issues_for(weak.id()), ["Weak"]);
        assert_eq!(Report::default().score(), 100);
    }
}
