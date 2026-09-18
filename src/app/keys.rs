//! Maps key presses to `core` messages using the configured shortcuts.

use crate::config::{Action, Modifier, Preferences};
use crate::core::state::{Mode, Msg};

/// A key press in toolkit-neutral form. `key` is a named key (`ArrowDown`, `Enter`, `F5`)
/// or the typed character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPress {
    pub key: String,
    pub modifiers: Vec<Modifier>,
}

impl KeyPress {
    pub fn new(modifiers: &[Modifier], key: &str) -> Self {
        Self {
            key: key.to_owned(),
            modifiers: modifiers.to_vec(),
        }
    }

    fn is(&self, modifiers: &[Modifier], key: &str) -> bool {
        let mut own = self.modifiers.clone();
        own.sort();
        let mut want = modifiers.to_vec();
        want.sort();
        own == want && self.key.eq_ignore_ascii_case(key)
    }
}

/// Context the key map needs besides the mode.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyContext {
    /// The search field caret is at the end of the query.
    pub caret_at_end: bool,
}

/// Keys that only modify other keys; never captured as a chord on their own.
const MODIFIER_KEYS: [&str; 8] = [
    "Control", "Shift", "Alt", "Super", "Meta", "Hyper", "AltGraph", "CapsLock",
];

pub fn map_key(press: &KeyPress, mode: &Mode, prefs: &Preferences, ctx: KeyContext) -> Option<Msg> {
    use Modifier::{Ctrl, Shift};
    if let Mode::Preferences { rebinding: Some(_) } = mode {
        if press.is(&[], "Escape") {
            return Some(Msg::Escape);
        }
        if MODIFIER_KEYS
            .iter()
            .any(|m| press.key.eq_ignore_ascii_case(m))
        {
            return None;
        }
        return Some(Msg::ChordCaptured(crate::config::KeyChord::new(
            &press.modifiers,
            &press.key,
        )));
    }
    let in_actions = matches!(mode, Mode::Actions { .. });
    if press.is(&[], "ArrowDown") || press.is(&[Ctrl], "n") || press.is(&[Ctrl], "j") {
        return Some(Msg::SelectNext);
    }
    if press.is(&[], "ArrowUp") || press.is(&[Ctrl], "p") || press.is(&[Ctrl], "k") {
        return Some(Msg::SelectPrev);
    }
    if press.is(&[], "PageDown") {
        return Some(Msg::PageDown);
    }
    if press.is(&[], "PageUp") {
        return Some(Msg::PageUp);
    }
    if press.is(&[], "Escape") {
        return Some(Msg::Escape);
    }
    if in_actions && (press.is(&[], "ArrowLeft") || press.is(&[Shift], "Tab")) {
        return Some(Msg::Back);
    }
    if matches!(mode, Mode::List) && ctx.caret_at_end && press.is(&[], "ArrowRight") {
        return Some(Msg::OpenActions);
    }
    if matches!(mode, Mode::Preferences { .. }) {
        return None;
    }
    if matches!(mode, Mode::List)
        && prefs.action_for(&press.modifiers, &press.key) == Some(Action::Preferences)
    {
        return Some(Msg::OpenPreferences);
    }
    match prefs.action_for(&press.modifiers, &press.key)? {
        Action::Refresh => Some(Msg::RefreshRequested),
        Action::CopyPrimary if in_actions => Some(Msg::ActivateAction(None)),
        Action::CopyPrimary => Some(Msg::CopyPrimary),
        Action::CopyUsername => Some(Msg::CopyUsername),
        Action::CopyTotp => Some(Msg::CopyTotp),
        Action::CopyUrl => Some(Msg::CopyUrl),
        Action::OpenActions if matches!(mode, Mode::List) => Some(Msg::OpenActions),
        Action::OpenDetail if matches!(mode, Mode::List) => Some(Msg::OpenDetail),
        Action::Reveal if matches!(mode, Mode::Detail { .. }) => Some(Msg::ToggleReveal),
        Action::OpenActions | Action::OpenDetail | Action::Reveal | Action::Preferences => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Modifier::{Ctrl, Shift};

    fn map(mods: &[Modifier], key: &str) -> Option<Msg> {
        map_key(
            &KeyPress::new(mods, key),
            &Mode::List,
            &Preferences::default(),
            KeyContext::default(),
        )
    }

    fn is(msg: Option<Msg>, pat: fn(&Msg) -> bool) -> bool {
        msg.as_ref().is_some_and(pat)
    }

    #[test]
    fn navigation_keys() {
        for (mods, key) in [
            (&[][..], "ArrowDown"),
            (&[Ctrl][..], "n"),
            (&[Ctrl][..], "j"),
        ] {
            assert!(
                is(map(mods, key), |m| matches!(m, Msg::SelectNext)),
                "{key}"
            );
        }
        for (mods, key) in [(&[][..], "ArrowUp"), (&[Ctrl][..], "p"), (&[Ctrl][..], "K")] {
            assert!(
                is(map(mods, key), |m| matches!(m, Msg::SelectPrev)),
                "{key}"
            );
        }
        assert!(is(map(&[], "PageDown"), |m| matches!(m, Msg::PageDown)));
        assert!(is(map(&[], "PageUp"), |m| matches!(m, Msg::PageUp)));
    }

    #[test]
    fn escape_goes_back() {
        assert!(is(map(&[], "Escape"), |m| matches!(m, Msg::Escape)));
    }

    #[test]
    fn enter_copies_primary() {
        assert!(is(map(&[], "Enter"), |m| matches!(m, Msg::CopyPrimary)));
        assert!(map(&[Ctrl], "Enter").is_none());
    }

    #[test]
    fn refresh_uses_configured_chord() {
        assert!(is(map(&[], "F5"), |m| matches!(m, Msg::RefreshRequested)));
        let mut prefs = Preferences::default();
        prefs.shortcuts.insert(
            Action::Refresh,
            crate::config::KeyChord::new(&[Ctrl, Shift], "r"),
        );
        let msg = map_key(
            &KeyPress::new(&[Shift, Ctrl], "R"),
            &Mode::List,
            &prefs,
            KeyContext::default(),
        );
        assert!(is(msg, |m| matches!(m, Msg::RefreshRequested)));
    }

    fn actions_mode() -> Mode {
        Mode::Actions {
            key: crate::model::ItemKey::new("s", "i"),
            selected: 0,
        }
    }

    fn map_in(mode: &Mode, mods: &[Modifier], key: &str, caret_at_end: bool) -> Option<Msg> {
        map_key(
            &KeyPress::new(mods, key),
            mode,
            &Preferences::default(),
            KeyContext { caret_at_end },
        )
    }

    #[test]
    fn copy_chords() {
        assert!(is(map(&[Ctrl], "u"), |m| matches!(m, Msg::CopyUsername)));
        assert!(is(map(&[Ctrl], "o"), |m| matches!(m, Msg::CopyTotp)));
        assert!(is(map(&[Ctrl], "l"), |m| matches!(m, Msg::CopyUrl)));
        let actions = actions_mode();
        assert!(is(map_in(&actions, &[Ctrl], "u", false), |m| matches!(
            m,
            Msg::CopyUsername
        )));
    }

    #[test]
    fn open_actions() {
        assert!(is(map(&[], "Tab"), |m| matches!(m, Msg::OpenActions)));
        assert!(is(
            map_in(&Mode::List, &[], "ArrowRight", true),
            |m| matches!(m, Msg::OpenActions)
        ));
        assert!(map_in(&Mode::List, &[], "ArrowRight", false).is_none());
        assert!(map_in(&actions_mode(), &[], "Tab", false).is_none());
    }

    #[test]
    fn action_list_keys() {
        let mode = actions_mode();
        assert!(is(map_in(&mode, &[], "ArrowDown", false), |m| matches!(
            m,
            Msg::SelectNext
        )));
        assert!(is(map_in(&mode, &[], "Enter", false), |m| matches!(
            m,
            Msg::ActivateAction(None)
        )));
        assert!(is(map_in(&mode, &[], "ArrowLeft", false), |m| matches!(
            m,
            Msg::Back
        )));
        assert!(is(map_in(&mode, &[Shift], "Tab", false), |m| matches!(
            m,
            Msg::Back
        )));
        assert!(is(map_in(&mode, &[], "Escape", false), |m| matches!(
            m,
            Msg::Escape
        )));
        assert!(map_in(&Mode::List, &[], "ArrowLeft", true).is_none());
    }

    #[test]
    fn rebound_copy_chord() {
        let mut prefs = Preferences::default();
        prefs
            .shortcuts
            .insert(Action::CopyTotp, crate::config::KeyChord::new(&[Ctrl], "t"));
        let t = map_key(
            &KeyPress::new(&[Ctrl], "t"),
            &Mode::List,
            &prefs,
            KeyContext::default(),
        );
        assert!(is(t, |m| matches!(m, Msg::CopyTotp)));
        let o = map_key(
            &KeyPress::new(&[Ctrl], "o"),
            &Mode::List,
            &prefs,
            KeyContext::default(),
        );
        assert!(o.is_none());
    }

    #[test]
    fn detail_keys() {
        assert!(is(map(&[Ctrl], "i"), |m| matches!(m, Msg::OpenDetail)));
        let detail = Mode::Detail {
            key: crate::model::ItemKey::new("s", "i"),
        };
        assert!(is(map_in(&detail, &[Ctrl], "r", false), |m| matches!(
            m,
            Msg::ToggleReveal
        )));
        assert!(map(&[Ctrl], "r").is_none());
        assert!(is(map_in(&detail, &[], "Enter", false), |m| matches!(
            m,
            Msg::CopyPrimary
        )));
        assert!(map_in(&detail, &[Ctrl], "i", false).is_none());
    }

    #[test]
    fn preferences_mode_ignores_chords() {
        assert!(map_in(&Mode::Preferences { rebinding: None }, &[Ctrl], "u", false).is_none());
        assert!(is(
            map_in(&Mode::Preferences { rebinding: None }, &[], "Escape", false),
            |m| matches!(m, Msg::Escape)
        ));
    }

    #[test]
    fn preferences_keys() {
        assert!(is(map(&[Ctrl], ","), |m| matches!(m, Msg::OpenPreferences)));
        let rebinding = Mode::Preferences {
            rebinding: Some(Action::CopyUrl),
        };
        let captured = map_in(&rebinding, &[Ctrl, Shift], "k", false);
        assert!(is(captured, |m| matches!(
            m,
            Msg::ChordCaptured(c) if *c == crate::config::KeyChord::new(&[Ctrl, Shift], "k")
        )));
        assert!(map_in(&rebinding, &[Ctrl], "Control", false).is_none());
        assert!(is(map_in(&rebinding, &[], "Escape", false), |m| matches!(
            m,
            Msg::Escape
        )));
    }

    #[test]
    fn plain_typing_is_not_mapped() {
        assert!(map(&[], "n").is_none());
        assert!(map(&[Shift], "N").is_none());
        assert!(map(&[Ctrl], "q").is_none());
    }
}
