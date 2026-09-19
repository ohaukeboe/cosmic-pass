//! Copy actions available for each item kind.

use crate::config::Action;
use crate::model::{FieldRef, ItemKind, ItemSummary};

/// What to copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopySource {
    /// A field; copied directly when its value is stored, otherwise fetched.
    Field(FieldRef),
    /// The current one-time code of a TOTP field.
    Totp { field: String },
}

impl CopySource {
    pub fn is_secret(&self) -> bool {
        match self {
            CopySource::Field(f) => f.secret,
            CopySource::Totp { .. } => true,
        }
    }
}

/// One entry of the action list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionEntry {
    pub source: CopySource,
    pub label: String,
    pub shortcut: Option<Action>,
    /// Where this row's field sits in the item's fields, or `None` for a one-time code, which
    /// is not one of them. A reveal pins the position rather than the name, because names
    /// repeat and positions do not.
    pub field_index: Option<usize>,
}

/// Every copyable field of an item, primary first (FR-013).
pub fn all_actions(item: &ItemSummary) -> Vec<ActionEntry> {
    let primary = primary_action(item);
    // Bind by position, so a shortcut lands on exactly the field its own accessor copies
    // even when several fields share a name (an item with two websites, say).
    let username = username_index(item);
    let url = url_index(item);
    let mut entries = Vec::new();
    if let Some(source) = &primary {
        // The primary row shows a field the item already carries, so it answers for that
        // field's position rather than for one of its own.
        let at = match source {
            CopySource::Field(field) => item.fields.iter().position(|f| f == field),
            CopySource::Totp { .. } => None,
        };
        entries.push(entry(source.clone(), Some(Action::CopyPrimary), at));
    }
    for (i, field) in item.fields.iter().enumerate() {
        let source = CopySource::Field(field.clone());
        if primary.as_ref() == Some(&source) {
            continue;
        }
        let shortcut = if Some(i) == username {
            Some(Action::CopyUsername)
        } else if Some(i) == url {
            Some(Action::CopyUrl)
        } else {
            None
        };
        entries.push(entry(source, shortcut, Some(i)));
    }
    for (i, field) in item.totp_fields.iter().enumerate() {
        let source = CopySource::Totp {
            field: field.clone(),
        };
        if primary.as_ref() == Some(&source) {
            continue;
        }
        let shortcut = (i == 0).then_some(Action::CopyTotp);
        entries.push(entry(source, shortcut, None));
    }
    entries
}

fn entry(source: CopySource, shortcut: Option<Action>, field_index: Option<usize>) -> ActionEntry {
    let label = match &source {
        CopySource::Field(f) => f.label.clone(),
        CopySource::Totp { field } if field == "totp_uri" => "One-time code".to_owned(),
        CopySource::Totp { field } => field.clone(),
    };
    ActionEntry {
        source,
        label,
        shortcut,
        field_index,
    }
}

fn index_of(item: &ItemSummary, name: &str) -> Option<usize> {
    item.fields.iter().position(|f| f.name == name)
}

fn username_index(item: &ItemSummary) -> Option<usize> {
    index_of(item, "username").or_else(|| index_of(item, "email"))
}

/// The first website; the parser names it `url` and numbers the rest (`url2`, ...).
fn url_index(item: &ItemSummary) -> Option<usize> {
    index_of(item, "url")
}

fn username_field(item: &ItemSummary) -> Option<&FieldRef> {
    username_index(item).map(|i| &item.fields[i])
}

/// Username, else email (FR-012).
pub fn username_action(item: &ItemSummary) -> Option<CopySource> {
    username_field(item).cloned().map(CopySource::Field)
}

/// The first one-time code field.
pub fn totp_action(item: &ItemSummary) -> Option<CopySource> {
    item.totp_fields.first().map(|field| CopySource::Totp {
        field: field.clone(),
    })
}

/// The first website.
pub fn url_action(item: &ItemSummary) -> Option<CopySource> {
    url_index(item).map(|i| CopySource::Field(item.fields[i].clone()))
}

/// The field copied by Enter (FR-011).
pub fn primary_action(item: &ItemSummary) -> Option<CopySource> {
    let preferred = match item.kind {
        ItemKind::Login => Some("password"),
        ItemKind::CreditCard => Some("number"),
        ItemKind::Note => Some("note"),
        _ => None,
    };
    let field = preferred
        .and_then(|name| item.field(name))
        .or_else(|| item.fields.iter().find(|f| f.secret))
        .or_else(|| item.fields.first());
    match field {
        Some(f) => Some(CopySource::Field(f.clone())),
        None => item.totp_fields.first().map(|field| CopySource::Totp {
            field: field.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ItemKey;

    fn item(kind: ItemKind, fields: Vec<FieldRef>) -> ItemSummary {
        ItemSummary {
            key: ItemKey::new("s", "i"),
            vault_name: "V".into(),
            kind,
            title: "t".into(),
            username: None,
            email: None,
            subtitle: None,
            urls: vec![],
            totp_fields: vec![],
            fields,
            modified_at: 0,
        }
    }

    fn primary_name(item: &ItemSummary) -> Option<String> {
        match primary_action(item)? {
            CopySource::Field(f) => Some(f.name),
            CopySource::Totp { field } => Some(format!("totp:{field}")),
        }
    }

    fn user() -> FieldRef {
        FieldRef::plain("username", "Username", "u".into())
    }

    #[test]
    fn login_copies_password() {
        let i = item(
            ItemKind::Login,
            vec![
                user(),
                FieldRef::secret("password", "Password"),
                FieldRef::secret("note", "Note"),
            ],
        );
        assert_eq!(primary_name(&i).as_deref(), Some("password"));
        assert!(primary_action(&i).unwrap().is_secret());
    }

    #[test]
    fn login_without_password_falls_back_to_first_field() {
        let i = item(ItemKind::Login, vec![user()]);
        assert_eq!(primary_name(&i).as_deref(), Some("username"));
    }

    #[test]
    fn card_copies_number() {
        let i = item(
            ItemKind::CreditCard,
            vec![
                FieldRef::plain("cardholder_name", "Cardholder", "x".into()),
                FieldRef::secret("verification_number", "Security code"),
                FieldRef::secret("number", "Card number"),
            ],
        );
        assert_eq!(primary_name(&i).as_deref(), Some("number"));
    }

    #[test]
    fn note_copies_note() {
        let i = item(
            ItemKind::Note,
            vec![
                FieldRef::unstored("Extra", "Extra"),
                FieldRef::secret("note", "Note"),
            ],
        );
        assert_eq!(primary_name(&i).as_deref(), Some("note"));
    }

    #[test]
    fn other_kinds_prefer_first_secret_field() {
        for kind in [
            ItemKind::Identity,
            ItemKind::SshKey,
            ItemKind::Wifi,
            ItemKind::Custom,
        ] {
            let i = item(
                kind.clone(),
                vec![
                    FieldRef::plain("ssid", "Network", "n".into()),
                    FieldRef::secret("password", "Password"),
                ],
            );
            assert_eq!(primary_name(&i).as_deref(), Some("password"), "{kind:?}");
            let i = item(
                kind.clone(),
                vec![FieldRef::unstored("Endpoint", "Endpoint")],
            );
            assert_eq!(primary_name(&i).as_deref(), Some("Endpoint"), "{kind:?}");
        }
    }

    #[test]
    fn alias_and_unknown_use_first_field_or_nothing() {
        let i = item(ItemKind::Alias, vec![FieldRef::secret("note", "Note")]);
        assert_eq!(primary_name(&i).as_deref(), Some("note"));
        let i = item(ItemKind::Unknown("x".into()), vec![]);
        assert_eq!(primary_name(&i), None);
    }

    fn login_full() -> ItemSummary {
        let mut i = item(
            ItemKind::Login,
            vec![
                user(),
                FieldRef::plain("email", "Email", "e@x".into()),
                FieldRef::secret("password", "Password"),
                FieldRef::plain("url", "Website", "https://x".into()),
                FieldRef::secret("Recovery", "Recovery"),
                FieldRef::secret("note", "Note"),
            ],
        );
        i.totp_fields = vec!["totp_uri".into(), "Backup".into()];
        i
    }

    fn summarize(entries: &[ActionEntry]) -> Vec<(String, Option<Action>, bool)> {
        entries
            .iter()
            .map(|e| (e.label.clone(), e.shortcut, e.source.is_secret()))
            .collect()
    }

    fn source_name(s: &CopySource) -> String {
        match s {
            CopySource::Field(f) => f.name.clone(),
            CopySource::Totp { field } => format!("totp:{field}"),
        }
    }

    /// The dedicated accessors bind by name and kind, never by position, so dropping the
    /// user-defined text fields the parser used to emit lands every shortcut on the field it
    /// landed on before (FR-116).
    #[test]
    fn dropping_user_defined_text_fields_moves_no_shortcut() {
        fn picks(i: &ItemSummary) -> Vec<Option<String>> {
            [
                primary_action(i),
                username_action(i),
                url_action(i),
                totp_action(i),
            ]
            .iter()
            .map(|s| s.as_ref().map(source_name))
            .collect()
        }
        let kept = login_full();
        let mut with_text = login_full();
        // What the parser emitted before: an unstored row per custom text field, on either
        // side of the fields the shortcuts aim at.
        with_text
            .fields
            .insert(0, FieldRef::unstored("Nickname", "Nickname"));
        with_text.fields.push(FieldRef::unstored("Hint", "Hint"));
        assert_eq!(picks(&with_text), picks(&kept));
        assert_eq!(
            picks(&kept),
            [
                Some("password".to_owned()),
                Some("username".to_owned()),
                Some("url".to_owned()),
                Some("totp:totp_uri".to_owned()),
            ]
        );
    }

    #[test]
    fn login_action_list() {
        let entries = all_actions(&login_full());
        assert_eq!(
            summarize(&entries),
            vec![
                ("Password".into(), Some(Action::CopyPrimary), true),
                ("Username".into(), Some(Action::CopyUsername), false),
                ("Email".into(), None, false),
                ("Website".into(), Some(Action::CopyUrl), false),
                ("Recovery".into(), None, true),
                ("Note".into(), None, true),
                ("One-time code".into(), Some(Action::CopyTotp), true),
                ("Backup".into(), None, true),
            ]
        );
    }

    #[test]
    fn every_website_is_listed_but_only_the_first_has_the_shortcut() {
        let mut i = login_full();
        i.fields.push(FieldRef::plain(
            "url2",
            "Website 2",
            "https://second.x".into(),
        ));
        i.urls = vec!["https://x".into(), "https://second.x".into()];
        let entries = all_actions(&i);
        let websites: Vec<_> = entries
            .iter()
            .filter(|e| matches!(&e.source, CopySource::Field(f) if f.name.starts_with("url")))
            .map(|e| (e.label.as_str(), e.shortcut, e.source.is_secret()))
            .collect();
        assert_eq!(
            websites,
            vec![
                ("Website", Some(Action::CopyUrl), false),
                ("Website 2", None, false),
            ]
        );
        assert!(
            matches!(url_action(&i), Some(CopySource::Field(f)) if f.value.as_deref() == Some("https://x"))
        );
    }

    #[test]
    fn every_row_carries_the_position_of_the_field_behind_it() {
        let mut i = login_full();
        // A custom field may repeat a built-in name, so the position — not the name — is what
        // a reveal has to pin to reach the right field.
        i.fields.push(FieldRef::unstored("username", "username"));
        let entries = all_actions(&i);
        assert_eq!(
            entries
                .iter()
                .map(|e| (e.label.as_str(), e.field_index))
                .collect::<Vec<_>>(),
            vec![
                ("Password", Some(2)),
                ("Username", Some(0)),
                ("Email", Some(1)),
                ("Website", Some(3)),
                ("Recovery", Some(4)),
                ("Note", Some(5)),
                ("username", Some(6)),
                ("One-time code", None),
                ("Backup", None),
            ]
        );
        // The primary row repeats a field of the item rather than introducing one, so it names
        // that field's own position.
        assert_eq!(
            entries[0].field_index.map(|at| i.fields[at].name.as_str()),
            Some("password")
        );
    }

    #[test]
    fn a_shortcut_is_bound_to_at_most_one_entry() {
        let mut i = login_full();
        // A custom field may carry the same name as a built-in one.
        i.fields.push(FieldRef::unstored("url", "url"));
        i.fields.push(FieldRef::unstored("username", "username"));
        let entries = all_actions(&i);
        for action in [Action::CopyUrl, Action::CopyUsername] {
            let bound = entries
                .iter()
                .filter(|e| e.shortcut == Some(action))
                .count();
            assert_eq!(bound, 1, "{action:?}");
        }
    }

    #[test]
    fn card_action_list_starts_with_number() {
        let i = item(
            ItemKind::CreditCard,
            vec![
                FieldRef::secret("number", "Card number"),
                FieldRef::plain("cardholder_name", "Cardholder", "x".into()),
                FieldRef::plain("expiration_date", "Expiration date", "2030-01".into()),
                FieldRef::secret("verification_number", "Security code"),
            ],
        );
        let labels: Vec<_> = all_actions(&i).into_iter().map(|e| e.label).collect();
        assert_eq!(
            labels,
            [
                "Card number",
                "Cardholder",
                "Expiration date",
                "Security code"
            ]
        );
    }

    #[test]
    fn note_action_list() {
        let i = item(
            ItemKind::Note,
            vec![
                FieldRef::secret("note", "Note"),
                FieldRef::unstored("Tag", "Tag"),
            ],
        );
        let entries = all_actions(&i);
        assert_eq!(entries[0].shortcut, Some(Action::CopyPrimary));
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn username_falls_back_to_email() {
        let mut i = login_full();
        assert!(matches!(username_action(&i), Some(CopySource::Field(f)) if f.name == "username"));
        i.fields.retain(|f| f.name != "username");
        assert!(matches!(username_action(&i), Some(CopySource::Field(f)) if f.name == "email"));
        i.fields.retain(|f| f.name != "email");
        assert!(username_action(&i).is_none());
        let entries = all_actions(&i);
        assert!(
            entries
                .iter()
                .all(|e| e.shortcut != Some(Action::CopyUsername))
        );
    }

    #[test]
    fn email_gets_username_shortcut_when_no_username() {
        let mut i = login_full();
        i.fields.retain(|f| f.name != "username");
        let email = all_actions(&i)
            .into_iter()
            .find(|e| e.label == "Email")
            .unwrap();
        assert_eq!(email.shortcut, Some(Action::CopyUsername));
    }

    #[test]
    fn totp_and_url_actions() {
        let i = login_full();
        assert_eq!(
            totp_action(&i),
            Some(CopySource::Totp {
                field: "totp_uri".into()
            })
        );
        assert!(
            matches!(url_action(&i), Some(CopySource::Field(f)) if f.value.as_deref() == Some("https://x"))
        );
        let bare = item(ItemKind::Login, vec![]);
        assert!(totp_action(&bare).is_none());
        assert!(url_action(&bare).is_none());
    }

    #[test]
    fn item_with_only_totp_copies_code() {
        let mut i = item(ItemKind::Custom, vec![]);
        i.totp_fields.push("Code".into());
        assert_eq!(primary_name(&i).as_deref(), Some("totp:Code"));
    }
}
