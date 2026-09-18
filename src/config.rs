//! User preferences stored with cosmic-config.

use std::collections::BTreeMap;

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

pub const CONFIG_ID: &str = "io.github.ohaukeboe.CosmicPass";

/// Actions that can be bound to a key chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    CopyPrimary,
    CopyUsername,
    CopyTotp,
    CopyUrl,
    OpenActions,
    OpenDetail,
    Reveal,
    Refresh,
    Preferences,
    SignIn,
    Retry,
}

impl Action {
    pub const ALL: [Action; 11] = [
        Action::CopyPrimary,
        Action::CopyUsername,
        Action::CopyTotp,
        Action::CopyUrl,
        Action::OpenActions,
        Action::OpenDetail,
        Action::Reveal,
        Action::Refresh,
        Action::Preferences,
        Action::SignIn,
        Action::Retry,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Action::CopyPrimary => "Copy primary field",
            Action::CopyUsername => "Copy username",
            Action::CopyTotp => "Copy one-time code",
            Action::CopyUrl => "Copy website",
            Action::OpenActions => "Show all fields",
            Action::OpenDetail => "Show details",
            Action::Reveal => "Reveal secrets",
            Action::Refresh => "Refresh",
            Action::Preferences => "Preferences",
            Action::SignIn => "Sign in",
            Action::Retry => "Try again",
        }
    }

    fn default_chord(self) -> KeyChord {
        match self {
            Action::CopyPrimary => KeyChord::new(&[], "Enter"),
            Action::CopyUsername => KeyChord::new(&[Modifier::Ctrl], "u"),
            Action::CopyTotp => KeyChord::new(&[Modifier::Ctrl], "o"),
            Action::CopyUrl => KeyChord::new(&[Modifier::Ctrl], "l"),
            Action::OpenActions => KeyChord::new(&[], "Tab"),
            Action::OpenDetail => KeyChord::new(&[Modifier::Ctrl], "i"),
            Action::Reveal => KeyChord::new(&[Modifier::Ctrl], "r"),
            Action::Refresh => KeyChord::new(&[], "F5"),
            Action::Preferences => KeyChord::new(&[Modifier::Ctrl], ","),
            Action::SignIn => KeyChord::new(&[Modifier::Ctrl, Modifier::Shift], "s"),
            Action::Retry => KeyChord::new(&[Modifier::Ctrl, Modifier::Shift], "r"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Modifier {
    Ctrl,
    Shift,
    Alt,
    Super,
}

/// A key plus modifiers. `key` is a named key (`Enter`, `Tab`, `F5`) or a single character.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyChord {
    pub modifiers: Vec<Modifier>,
    pub key: String,
}

impl KeyChord {
    pub fn new(modifiers: &[Modifier], key: &str) -> Self {
        let mut modifiers = modifiers.to_vec();
        modifiers.sort();
        modifiers.dedup();
        Self {
            modifiers,
            key: key.to_owned(),
        }
    }

    /// Case-insensitive key comparison; modifiers must match exactly.
    pub fn matches(&self, modifiers: &[Modifier], key: &str) -> bool {
        let mut pressed = modifiers.to_vec();
        pressed.sort();
        pressed.dedup();
        let mut own = self.modifiers.clone();
        own.sort();
        own.dedup();
        own == pressed && self.key.eq_ignore_ascii_case(key)
    }

    fn normalized(&self) -> Self {
        let mut chord = Self::new(&self.modifiers, &self.key);
        chord.key = chord.key.to_lowercase();
        chord
    }
}

impl std::fmt::Display for KeyChord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for m in &self.modifiers {
            write!(f, "{m:?}+")?;
        }
        if self.key.chars().count() == 1 {
            write!(f, "{}", self.key.to_uppercase())
        } else {
            write!(f, "{}", self.key)
        }
    }
}

pub const CLIPBOARD_CLEAR_RANGE: std::ops::RangeInclusive<u32> = 10..=600;
/// How much one press of the timeout stepper (button or key) changes the value.
pub const CLIPBOARD_CLEAR_STEP: i64 = 10;
pub const REFRESH_STALE_RANGE: std::ops::RangeInclusive<u32> = 30..=86_400;
/// Each rendered row costs layout and drawing time, so the default is small; raise it in
/// the config file if you prefer a longer list.
pub const MAX_RESULTS_RANGE: std::ops::RangeInclusive<u16> = 5..=200;

#[derive(Debug, Clone, PartialEq, Eq, CosmicConfigEntry)]
#[version = 1]
pub struct Preferences {
    pub clipboard_clear_secs: u32,
    pub refresh_stale_secs: u32,
    pub max_results: u16,
    pub shortcuts: BTreeMap<Action, KeyChord>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            clipboard_clear_secs: 90,
            refresh_stale_secs: 300,
            max_results: 12,
            shortcuts: Action::ALL
                .iter()
                .map(|a| (*a, a.default_chord()))
                .collect(),
        }
    }
}

impl Preferences {
    /// Clamps numbers into range, fills missing shortcuts, and resets any shortcut whose chord
    /// is already used by an earlier action to its default.
    pub fn validated(mut self) -> Self {
        self.clipboard_clear_secs = clamp(self.clipboard_clear_secs, &CLIPBOARD_CLEAR_RANGE);
        self.refresh_stale_secs = clamp(self.refresh_stale_secs, &REFRESH_STALE_RANGE);
        self.max_results = clamp(self.max_results, &MAX_RESULTS_RANGE);

        let mut used: Vec<KeyChord> = Vec::new();
        let mut shortcuts = BTreeMap::new();
        for action in Action::ALL {
            let chord = self
                .shortcuts
                .remove(&action)
                .filter(|c| !c.key.is_empty() && !used.contains(&c.normalized()))
                .unwrap_or_else(|| action.default_chord());
            used.push(chord.normalized());
            shortcuts.insert(action, chord);
        }
        self.shortcuts = shortcuts;
        self
    }

    pub fn chord(&self, action: Action) -> KeyChord {
        self.shortcuts
            .get(&action)
            .cloned()
            .unwrap_or_else(|| action.default_chord())
    }

    /// The action bound to a key press, if any.
    pub fn action_for(&self, modifiers: &[Modifier], key: &str) -> Option<Action> {
        Action::ALL
            .into_iter()
            .find(|a| self.chord(*a).matches(modifiers, key))
    }

    /// Returns the action already using `chord`, other than `action`.
    pub fn conflict(&self, action: Action, chord: &KeyChord) -> Option<Action> {
        let wanted = chord.normalized();
        Action::ALL
            .into_iter()
            .find(|a| *a != action && self.chord(*a).normalized() == wanted)
    }
}

fn clamp<T: PartialOrd + Copy>(value: T, range: &std::ops::RangeInclusive<T>) -> T {
    if value < *range.start() {
        *range.start()
    } else if value > *range.end() {
        *range.end()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let p = Preferences::default();
        assert_eq!(p.clipboard_clear_secs, 90);
        assert_eq!(p.refresh_stale_secs, 300);
        assert_eq!(p.max_results, 12);
        assert_eq!(p.chord(Action::CopyPrimary), KeyChord::new(&[], "Enter"));
        assert_eq!(
            p.chord(Action::CopyTotp),
            KeyChord::new(&[Modifier::Ctrl], "o")
        );
        assert_eq!(p.shortcuts.len(), Action::ALL.len());
        assert_eq!(p.clone().validated(), p);
    }

    #[test]
    fn numbers_are_clamped() {
        let p = Preferences {
            clipboard_clear_secs: 1,
            refresh_stale_secs: 1_000_000,
            max_results: 2,
            ..Preferences::default()
        }
        .validated();
        assert_eq!(p.clipboard_clear_secs, 10);
        assert_eq!(p.refresh_stale_secs, 86_400);
        assert_eq!(p.max_results, 5);

        let p = Preferences {
            clipboard_clear_secs: 601,
            refresh_stale_secs: 29,
            max_results: 500,
            ..Preferences::default()
        }
        .validated();
        assert_eq!(p.clipboard_clear_secs, 600);
        assert_eq!(p.refresh_stale_secs, 30);
        assert_eq!(p.max_results, 200);
    }

    #[test]
    fn duplicate_chord_falls_back_to_default_for_later_action() {
        let mut p = Preferences::default();
        let taken = KeyChord::new(&[Modifier::Ctrl], "u");
        p.shortcuts.insert(Action::CopyUrl, taken.clone());
        let p = p.validated();
        assert_eq!(p.chord(Action::CopyUsername), taken);
        assert_eq!(
            p.chord(Action::CopyUrl),
            KeyChord::new(&[Modifier::Ctrl], "l")
        );
    }

    #[test]
    fn duplicates_ignore_case_and_modifier_order() {
        let mut p = Preferences::default();
        p.shortcuts.insert(
            Action::Refresh,
            KeyChord {
                modifiers: vec![Modifier::Ctrl],
                key: "U".into(),
            },
        );
        assert_eq!(
            p.validated().chord(Action::Refresh),
            KeyChord::new(&[], "F5")
        );
    }

    #[test]
    fn missing_shortcuts_are_filled() {
        let mut p = Preferences::default();
        p.shortcuts.clear();
        assert_eq!(p.validated(), Preferences::default());
    }

    #[test]
    fn action_lookup() {
        let p = Preferences::default();
        assert_eq!(p.action_for(&[], "Enter"), Some(Action::CopyPrimary));
        assert_eq!(p.action_for(&[Modifier::Ctrl], "O"), Some(Action::CopyTotp));
        assert_eq!(p.action_for(&[Modifier::Ctrl], "Enter"), None);
        assert_eq!(p.action_for(&[], "u"), None);
    }

    #[test]
    fn conflicts() {
        let p = Preferences::default();
        let chord = KeyChord::new(&[Modifier::Ctrl], "u");
        assert_eq!(
            p.conflict(Action::CopyUrl, &chord),
            Some(Action::CopyUsername)
        );
        assert_eq!(p.conflict(Action::CopyUsername, &chord), None);
    }

    #[test]
    fn chord_display() {
        assert_eq!(KeyChord::new(&[Modifier::Ctrl], "u").to_string(), "Ctrl+U");
        assert_eq!(KeyChord::new(&[], "F5").to_string(), "F5");
    }

    #[test]
    fn status_actions_have_distinct_defaults() {
        let p = Preferences::default();
        assert_eq!(
            p.chord(Action::SignIn),
            KeyChord::new(&[Modifier::Ctrl, Modifier::Shift], "s")
        );
        assert_eq!(
            p.chord(Action::Retry),
            KeyChord::new(&[Modifier::Ctrl, Modifier::Shift], "r")
        );
        assert_eq!(p.action_for(&[Modifier::Ctrl], "r"), Some(Action::Reveal));
        assert_eq!(
            p.action_for(&[Modifier::Ctrl, Modifier::Shift], "R"),
            Some(Action::Retry)
        );
    }

    /// Preferences stored before the status actions existed must still load.
    #[test]
    fn older_stored_shortcuts_gain_the_new_actions() {
        let stored = r#"{"copy_primary":{"modifiers":[],"key":"Enter"},"refresh":{"modifiers":[],"key":"F7"}}"#;
        let shortcuts: BTreeMap<Action, KeyChord> = serde_json::from_str(stored).unwrap();
        let p = Preferences {
            shortcuts,
            ..Preferences::default()
        }
        .validated();
        assert_eq!(p.chord(Action::Refresh), KeyChord::new(&[], "F7"));
        assert_eq!(p.shortcuts.len(), Action::ALL.len());
        assert_eq!(
            p.chord(Action::SignIn),
            Action::SignIn.default_chord(),
            "missing action falls back to its default"
        );
    }

    #[test]
    fn chords_round_trip_through_ron_like_serde() {
        let p = Preferences::default();
        let json = serde_json::to_string(&p.shortcuts).unwrap();
        assert!(json.contains("\"copy_primary\""));
        assert!(json.contains("\"sign_in\""));
        assert!(json.contains("\"retry\""));
        let back: BTreeMap<Action, KeyChord> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p.shortcuts);
    }
}
