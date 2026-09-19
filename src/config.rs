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
    Reveal,
    Refresh,
    Preferences,
    SignIn,
    Retry,
}

impl Action {
    pub const ALL: [Action; 10] = [
        Action::CopyPrimary,
        Action::CopyUsername,
        Action::CopyTotp,
        Action::CopyUrl,
        Action::OpenActions,
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

/// The chord bound to each action.
///
/// Loaded by hand rather than by `#[derive(Deserialize)]` so that an action name this version
/// does not know costs only its own entry. cosmic-config reads a stored config by building
/// `Preferences::default()` and overwriting one field per stored key, so a `shortcuts` value
/// that fails to parse leaves the whole field at its defaults: one stale name — `open_detail`,
/// written by every version before this one — would revert every chord the user ever set
/// (FR-110). Serialization is unchanged, so a config this version writes stays readable.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Shortcuts(BTreeMap<Action, KeyChord>);

impl std::ops::Deref for Shortcuts {
    type Target = BTreeMap<Action, KeyChord>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Shortcuts {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl FromIterator<(Action, KeyChord)> for Shortcuts {
    fn from_iter<I: IntoIterator<Item = (Action, KeyChord)>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl<'de> Deserialize<'de> for Shortcuts {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(ShortcutsVisitor)
    }
}

struct ShortcutsVisitor;

impl<'de> serde::de::Visitor<'de> for ShortcutsVisitor {
    type Value = Shortcuts;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a map of action names to key chords")
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Shortcuts, A::Error> {
        let mut shortcuts = BTreeMap::new();
        while let Some(MaybeAction(action)) = map.next_key()? {
            // The value is read either way: skipping it would leave the parser mid-entry.
            let chord = map.next_value()?;
            if let Some(action) = action {
                shortcuts.insert(action, chord);
            }
        }
        Ok(Shortcuts(shortcuts))
    }
}

/// One key of a stored shortcut map: the action it names, or `None` for a name this version
/// has no action for.
struct MaybeAction(Option<Action>);

impl<'de> Deserialize<'de> for MaybeAction {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // An identifier, because that is how RON writes an enum used as a map key and how
        // JSON writes any key at all.
        deserializer.deserialize_identifier(ActionNameVisitor)
    }
}

struct ActionNameVisitor;

impl serde::de::Visitor<'_> for ActionNameVisitor {
    type Value = MaybeAction;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("an action name")
    }

    fn visit_str<E: serde::de::Error>(self, name: &str) -> Result<MaybeAction, E> {
        // Resolved through `Action`'s own derived `Deserialize` so the names it accepts here
        // cannot drift from the names it writes.
        use serde::de::IntoDeserializer;
        let name: serde::de::value::StrDeserializer<'_, E> = name.into_deserializer();
        Ok(MaybeAction(Action::deserialize(name).ok()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, CosmicConfigEntry)]
#[version = 1]
pub struct Preferences {
    pub clipboard_clear_secs: u32,
    pub refresh_stale_secs: u32,
    pub max_results: u16,
    pub shortcuts: Shortcuts,
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
        self.shortcuts = Shortcuts(shortcuts);
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
        let shortcuts: Shortcuts = serde_json::from_str(stored).unwrap();
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
    fn chords_round_trip_through_serde_json() {
        let p = Preferences::default();
        let json = serde_json::to_string(&p.shortcuts).unwrap();
        assert!(json.contains("\"copy_primary\""));
        assert!(json.contains("\"sign_in\""));
        assert!(json.contains("\"retry\""));
        let back: Shortcuts = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p.shortcuts);
    }
    /// The format the chords are actually stored in. cosmic-config reads its files with
    /// `ron::from_str` and writes an enum map key as a bare identifier, which the derived
    /// `Deserialize` would reject outright on a name this version no longer has: it answers
    /// `NoSuchEnumVariant { found: "open_detail" }`, the whole `shortcuts` value falls back
    /// to `Preferences::default()`, and every chord the user set is gone (FR-110, SC-005).
    /// The serde_json tests above cannot catch that — RON is a different parser, and its map
    /// keys arrive as identifiers rather than strings.
    #[test]
    fn a_stored_ron_map_keeps_every_chord_but_the_removed_action() {
        let stored = r#"{
            open_detail: (modifiers: [Ctrl], key: "i"),
            copy_username: (modifiers: [Ctrl], key: "y"),
            refresh: (modifiers: [], key: "F7"),
        }"#;
        let shortcuts: Shortcuts =
            ron::from_str(stored).expect("a name this version dropped must not fail the map");
        assert_eq!(shortcuts.len(), 2, "only the unknown name is dropped");
        let p = Preferences {
            shortcuts,
            ..Preferences::default()
        }
        .validated();
        assert_eq!(
            p.chord(Action::CopyUsername),
            KeyChord::new(&[Modifier::Ctrl], "y"),
            "a rebound chord survives the upgrade"
        );
        assert_eq!(p.chord(Action::Refresh), KeyChord::new(&[], "F7"));
        assert_eq!(
            p.action_for(&[Modifier::Ctrl], "i"),
            None,
            "and the chord the removed action held is free"
        );
    }

    #[test]
    fn chords_round_trip_through_ron() {
        let p = Preferences::default();
        let stored = ron::ser::to_string_pretty(&p.shortcuts, ron::ser::PrettyConfig::new())
            .expect("the map serializes to RON");
        assert!(
            stored.contains("copy_primary: ("),
            "RON writes an action key as a bare identifier: {stored}"
        );
        let back: Shortcuts = ron::from_str(&stored).expect("and reads its own output back");
        assert_eq!(back, p.shortcuts);
    }

    /// FR-109: the detail pane is gone, and so is the action that opened it.
    #[test]
    fn nothing_opens_a_detail_pane_any_more() {
        assert_eq!(Action::ALL.len(), 10, "one action fewer than before");
        let names: Vec<String> = Action::ALL
            .iter()
            .map(|a| serde_json::to_string(a).expect("an action serializes to its name"))
            .collect();
        assert!(
            !names.iter().any(|n| n.contains("detail")),
            "no action is named after the removed pane: {names:?}"
        );
        assert!(
            !Action::ALL.iter().any(|a| a.label().contains("detail")),
            "and none of them still offers to show details"
        );
        assert_eq!(
            Preferences::default().action_for(&[Modifier::Ctrl], "i"),
            None,
            "Ctrl+I is left bound to nothing"
        );
    }

    /// A chord map stored by an older version still names `open_detail`. Dropping that one
    /// entry rather than failing the whole map is what keeps every other chord the user set
    /// (FR-110): cosmic-config overwrites one field per config key, so a `shortcuts` value
    /// that fails to parse takes all eleven bindings down with it.
    #[test]
    fn a_binding_for_a_removed_action_is_dropped_and_the_rest_survive() {
        let stored = r#"{"open_detail":{"modifiers":["Ctrl"],"key":"i"},"copy_username":{"modifiers":["Ctrl"],"key":"y"}}"#;
        let shortcuts: Shortcuts =
            serde_json::from_str(stored).expect("an unknown action name must not fail the map");
        assert_eq!(
            shortcuts.get(&Action::CopyUsername),
            Some(&KeyChord::new(&[Modifier::Ctrl], "y")),
            "the recognized chord is kept"
        );
        assert_eq!(shortcuts.len(), 1, "and the unknown one is simply gone");
    }

    /// SC-005: no user-set chord may silently revert because a stale action name sat beside it.
    #[test]
    fn validating_a_map_with_a_removed_action_keeps_the_user_set_chords() {
        let stored = r#"{"open_detail":{"modifiers":["Ctrl"],"key":"i"},"copy_username":{"modifiers":["Ctrl"],"key":"y"},"refresh":{"modifiers":[],"key":"F7"}}"#;
        let shortcuts: Shortcuts = serde_json::from_str(stored).expect("the map still loads");
        let p = Preferences {
            shortcuts,
            ..Preferences::default()
        }
        .validated();
        assert_eq!(
            p.chord(Action::CopyUsername),
            KeyChord::new(&[Modifier::Ctrl], "y")
        );
        assert_eq!(p.chord(Action::Refresh), KeyChord::new(&[], "F7"));
        assert_eq!(
            p.shortcuts.len(),
            Action::ALL.len(),
            "the actions the map never mentioned are filled with their defaults"
        );
    }

    /// The newtype only changes how unknown keys are treated: a config it writes is the same
    /// map a config it reads is, so an older version can still read this one back.
    #[test]
    fn the_shortcut_map_is_written_in_the_shape_it_is_read() {
        let p = Preferences::default();
        let json = serde_json::to_string(&p.shortcuts).expect("the map serializes");
        let plain: BTreeMap<Action, KeyChord> =
            serde_json::from_str(&json).expect("a plain map reads it back");
        assert_eq!(plain.len(), Action::ALL.len());
        let back: Shortcuts = serde_json::from_str(&json).expect("and so does the newtype");
        assert_eq!(back, p.shortcuts);
    }
}
