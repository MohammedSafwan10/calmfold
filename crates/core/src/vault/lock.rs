//! The vault's lock state machine and auto-lock rule.
//!
//! ```text
//! Uninitialized ──SetUp──▶ Unlocked
//! Locked ──StartUnlock──▶ Unlocking ──Unlocked──▶ Unlocked
//!                           └──Failed──▶ Locked
//! Unlocked ──Lock──▶ Locked            (manual, idle, sleep, quit)
//! Locked ──Reset──▶ Uninitialized      (forgot password → start a new vault;
//!                                       the old one is archived first)
//! ```
//!
//! Every other (state, event) pair is refused, so for example a stale unlock
//! result arriving after the user locked again is ignored.
//!
//! ## What opens the vault ([`Opener`], ADR 0014)
//!
//! A second, independent machine says which secret opens the vault on this
//! device. The recovery key (ADR 0006) always works as well.
//!
//! ```text
//! (none) ──SetUp(MasterPassword)──▶ MasterPassword   (not signed in to Ottero Cloud)
//! (none) ──SetUp(Account)─────────▶ Account          (signed in: no master password)
//! MasterPassword ──Link──▶ Account       (Locked or Unlocked; needs the master
//!                                         password once and an unlocked account)
//! any ──Recover(o)──▶ o                  (Locked; recovery key, then a new master
//!                                         password, or the account that is open now)
//! any ──Reset──▶ (none)                  (Locked; "Start a new vault", archived first)
//! ```
//!
//! There is no way back from `Account` to `MasterPassword` except recovery,
//! which needs the recovery key: signing out or signing in to another
//! account never downgrades the vault to a password the user retired.

use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockState {
    /// No vault yet: show setup.
    Uninitialized,
    Locked,
    /// Deriving the key in the background.
    Unlocking,
    Unlocked,
}

impl LockState {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Uninitialized => "not set up",
            Self::Locked => "locked",
            Self::Unlocking => "unlocking",
            Self::Unlocked => "unlocked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockEvent {
    SetUp,
    StartUnlock,
    Unlocked,
    Failed,
    Lock,
    Reset,
}

/// The next state, or `None` if `event` isn't allowed in `state`.
#[must_use]
pub fn next(state: LockState, event: LockEvent) -> Option<LockState> {
    use LockEvent as E;
    use LockState as S;
    match (state, event) {
        (S::Uninitialized, E::SetUp) => Some(S::Unlocked),
        (S::Locked, E::StartUnlock) => Some(S::Unlocking),
        (S::Unlocking, E::Unlocked) => Some(S::Unlocked),
        (S::Unlocking, E::Failed) => Some(S::Locked),
        (S::Unlocked | S::Unlocking, E::Lock) => Some(S::Locked),
        (S::Locked, E::Reset) => Some(S::Uninitialized),
        _ => None,
    }
}

/// The secret that opens the vault on this device (besides the recovery key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opener {
    /// The vault's own master password (Argon2id, ADR 0003).
    MasterPassword,
    /// The Ottero Cloud account key: one password for the app (ADR 0014).
    Account,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenerEvent {
    SetUp(Opener),
    /// Re-wrap the vault key under the account key and retire the master password.
    Link,
    /// The recovery key opened the vault; it now opens with this.
    Recover(Opener),
    Reset,
}

/// The opener after `event`, or `None` if `event` isn't allowed. The outer
/// `Option` is "allowed?", the inner one is "is there a vault?".
#[must_use]
pub fn next_opener(
    state: LockState,
    opener: Option<Opener>,
    event: OpenerEvent,
) -> Option<Option<Opener>> {
    use LockState as S;
    use OpenerEvent as E;
    match (state, opener, event) {
        (S::Uninitialized, None, E::SetUp(o)) => Some(Some(o)),
        (S::Locked | S::Unlocked, Some(Opener::MasterPassword), E::Link) => {
            Some(Some(Opener::Account))
        }
        (S::Locked, Some(_), E::Recover(o)) => Some(Some(o)),
        (S::Locked, Some(_), E::Reset) => Some(None),
        _ => None,
    }
}

/// How long the vault may sit unused before it locks itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoLock {
    OneMinute,
    #[default]
    FiveMinutes,
    FifteenMinutes,
    OneHour,
    /// Only when Ottero quits (or the user locks it).
    Never,
}

impl AutoLock {
    pub const ALL: [Self; 5] = [
        Self::OneMinute,
        Self::FiveMinutes,
        Self::FifteenMinutes,
        Self::OneHour,
        Self::Never,
    ];

    #[must_use]
    pub fn after(self) -> Option<Duration> {
        let mins = match self {
            Self::OneMinute => 1,
            Self::FiveMinutes => 5,
            Self::FifteenMinutes => 15,
            Self::OneHour => 60,
            Self::Never => return None,
        };
        Some(Duration::from_secs(mins * 60))
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::OneMinute => "1 min",
            Self::FiveMinutes => "5 min",
            Self::FifteenMinutes => "15 min",
            Self::OneHour => "1 hour",
            Self::Never => "Never",
        }
    }

    /// Whether `idle` (time since the last activity) has run out.
    #[must_use]
    pub fn expired(self, idle: Duration) -> bool {
        self.after().is_some_and(|limit| idle >= limit)
    }
}

/// How long a copied secret stays on the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardClear {
    FifteenSeconds,
    #[default]
    ThirtySeconds,
    NinetySeconds,
}

impl ClipboardClear {
    pub const ALL: [Self; 3] = [
        Self::FifteenSeconds,
        Self::ThirtySeconds,
        Self::NinetySeconds,
    ];

    #[must_use]
    pub fn after(self) -> Duration {
        Duration::from_secs(match self {
            Self::FifteenSeconds => 15,
            Self::ThirtySeconds => 30,
            Self::NinetySeconds => 90,
        })
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::FifteenSeconds => "15 s",
            Self::ThirtySeconds => "30 s",
            Self::NinetySeconds => "90 s",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use LockEvent as E;
    use LockState as S;

    #[test]
    fn transition_table() {
        let all_states = [S::Uninitialized, S::Locked, S::Unlocking, S::Unlocked];
        let all_events = [
            E::SetUp,
            E::StartUnlock,
            E::Unlocked,
            E::Failed,
            E::Lock,
            E::Reset,
        ];
        let allowed = [
            (S::Uninitialized, E::SetUp, S::Unlocked),
            (S::Locked, E::StartUnlock, S::Unlocking),
            (S::Unlocking, E::Unlocked, S::Unlocked),
            (S::Unlocking, E::Failed, S::Locked),
            (S::Unlocking, E::Lock, S::Locked),
            (S::Unlocked, E::Lock, S::Locked),
            (S::Locked, E::Reset, S::Uninitialized),
        ];
        for s in all_states {
            for e in all_events {
                let expected = allowed
                    .iter()
                    .find(|(a, b, _)| *a == s && *b == e)
                    .map(|(_, _, to)| *to);
                assert_eq!(next(s, e), expected, "{s:?} + {e:?}");
            }
        }
    }

    #[test]
    fn opener_transition_table() {
        use Opener::{Account, MasterPassword};
        use OpenerEvent as O;
        let states = [S::Uninitialized, S::Locked, S::Unlocking, S::Unlocked];
        let openers = [None, Some(MasterPassword), Some(Account)];
        let events = [
            O::SetUp(MasterPassword),
            O::SetUp(Account),
            O::Link,
            O::Recover(MasterPassword),
            O::Recover(Account),
            O::Reset,
        ];
        let allowed: &[(LockState, Option<Opener>, OpenerEvent, Option<Opener>)] = &[
            (
                S::Uninitialized,
                None,
                O::SetUp(MasterPassword),
                Some(MasterPassword),
            ),
            (S::Uninitialized, None, O::SetUp(Account), Some(Account)),
            (S::Locked, Some(MasterPassword), O::Link, Some(Account)),
            (S::Unlocked, Some(MasterPassword), O::Link, Some(Account)),
            (
                S::Locked,
                Some(MasterPassword),
                O::Recover(MasterPassword),
                Some(MasterPassword),
            ),
            (
                S::Locked,
                Some(MasterPassword),
                O::Recover(Account),
                Some(Account),
            ),
            (
                S::Locked,
                Some(Account),
                O::Recover(MasterPassword),
                Some(MasterPassword),
            ),
            (S::Locked, Some(Account), O::Recover(Account), Some(Account)),
            (S::Locked, Some(MasterPassword), O::Reset, None),
            (S::Locked, Some(Account), O::Reset, None),
        ];
        for s in states {
            for o in openers {
                for e in events {
                    let expected = allowed
                        .iter()
                        .find(|(a, b, c, _)| *a == s && *b == o && *c == e)
                        .map(|(_, _, _, to)| *to);
                    assert_eq!(next_opener(s, o, e), expected, "{s:?} + {o:?} + {e:?}");
                }
            }
        }
    }

    #[test]
    fn a_linked_vault_never_falls_back_to_the_master_password_by_itself() {
        // Only recovery (which needs the recovery key) leaves Account.
        for s in [S::Uninitialized, S::Locked, S::Unlocking, S::Unlocked] {
            for e in [
                OpenerEvent::Link,
                OpenerEvent::SetUp(Opener::MasterPassword),
            ] {
                assert_eq!(next_opener(s, Some(Opener::Account), e), None);
            }
        }
    }

    #[test]
    fn auto_lock_expires_only_with_a_limit() {
        assert!(AutoLock::FiveMinutes.expired(Duration::from_secs(300)));
        assert!(!AutoLock::FiveMinutes.expired(Duration::from_secs(299)));
        assert!(!AutoLock::Never.expired(Duration::from_secs(1_000_000)));
        assert_eq!(ClipboardClear::default().after(), Duration::from_secs(30));
    }
}
