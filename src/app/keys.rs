//! Maps key presses to `core` messages using the configured shortcuts.

use crate::config::{Action, CLIPBOARD_CLEAR_STEP, Modifier, Preferences};
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

/// A bare key press, ignoring Shift: on most layouts `+` is only reachable with it.
fn unshifted(press: &KeyPress, keys: &[&str]) -> bool {
    keys.iter()
        .any(|k| press.is(&[], k) || press.is(&[Modifier::Shift], k))
}

/// The preferences editor has no focus ring to walk — `set_keyboard_nav(false)` turns Tab
/// traversal off — so every control it offers needs its own chord here.
fn preferences_key(
    press: &KeyPress,
    rebinding: Option<Action>,
    prefs: &Preferences,
) -> Option<Msg> {
    use Modifier::{Ctrl, Shift};
    if press.is(&[], "Escape") {
        return Some(Msg::Escape);
    }
    if rebinding.is_some() {
        // Capture mode: anything but a lone modifier becomes the new binding.
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
    if press.is(&[Ctrl, Shift], "Delete") {
        return Some(Msg::ResetShortcuts);
    }
    if unshifted(press, &["ArrowUp", "ArrowRight", "+", "="]) {
        return Some(Msg::AdjustClipboardClear(CLIPBOARD_CLEAR_STEP));
    }
    if unshifted(press, &["ArrowDown", "ArrowLeft", "-", "_"]) {
        return Some(Msg::AdjustClipboardClear(-CLIPBOARD_CLEAR_STEP));
    }
    // Each row shows the chord it holds, so pressing that chord is how the row is reached.
    prefs
        .action_for(&press.modifiers, &press.key)
        .map(Msg::StartRebind)
}

pub fn map_key(press: &KeyPress, mode: &Mode, prefs: &Preferences, ctx: KeyContext) -> Option<Msg> {
    use Modifier::{Ctrl, Shift};
    if let Mode::Preferences { rebinding } = mode {
        return preferences_key(press, *rebinding, prefs);
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
        Action::SignIn => Some(Msg::StartLogin),
        Action::Retry => Some(Msg::Startup),
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

    fn prefs_mode() -> Mode {
        Mode::Preferences { rebinding: None }
    }

    #[test]
    fn preferences_mode_leaves_the_result_list_alone() {
        for (mods, key) in [
            (&[][..], "PageDown"),
            (&[][..], "PageUp"),
            (&[Ctrl][..], "n"),
            (&[Ctrl][..], "p"),
        ] {
            assert!(
                map_in(&prefs_mode(), mods, key, false).is_none(),
                "{key} must not reach the hidden list"
            );
        }
        for (key, pat) in [
            ("ArrowDown", -10_i64),
            ("ArrowUp", 10),
            ("ArrowLeft", -10),
            ("ArrowRight", 10),
        ] {
            let msg = map_in(&prefs_mode(), &[], key, false);
            assert!(
                matches!(msg, Some(Msg::AdjustClipboardClear(step)) if step == pat),
                "{key}"
            );
        }
        assert!(is(
            map_in(&prefs_mode(), &[], "Escape", false),
            |m| matches!(m, Msg::Escape)
        ));
    }

    #[test]
    fn preferences_editor_is_reachable_by_keyboard() {
        assert!(matches!(
            map_in(&prefs_mode(), &[Shift], "+", false),
            Some(Msg::AdjustClipboardClear(10))
        ));
        assert!(matches!(
            map_in(&prefs_mode(), &[], "-", false),
            Some(Msg::AdjustClipboardClear(-10))
        ));
        assert!(is(
            map_in(&prefs_mode(), &[Ctrl, Shift], "Delete", false),
            |m| matches!(m, Msg::ResetShortcuts)
        ));
        // A row is rebound by pressing the chord it currently shows.
        assert!(is(
            map_in(&prefs_mode(), &[Ctrl], "u", false),
            |m| matches!(m, Msg::StartRebind(Action::CopyUsername))
        ));
        assert!(is(
            map_in(&prefs_mode(), &[], "Enter", false),
            |m| matches!(m, Msg::StartRebind(Action::CopyPrimary))
        ));
        assert!(map_in(&prefs_mode(), &[Ctrl], "q", false).is_none());
    }

    #[test]
    fn rebind_capture_still_wins_over_editor_keys() {
        let rebinding = Mode::Preferences {
            rebinding: Some(Action::Refresh),
        };
        for key in ["ArrowDown", "-", "Delete"] {
            assert!(
                is(map_in(&rebinding, &[], key, false), |m| matches!(
                    m,
                    Msg::ChordCaptured(_)
                )),
                "{key}"
            );
        }
    }

    #[test]
    fn status_panel_actions_have_chords() {
        assert!(is(map(&[Ctrl, Shift], "s"), |m| matches!(
            m,
            Msg::StartLogin
        )));
        assert!(is(map(&[Ctrl, Shift], "r"), |m| matches!(m, Msg::Startup)));
        assert!(!is(map(&[Ctrl], "r"), |m| matches!(m, Msg::Startup)));
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
