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
}

/// Every copyable field of an item, primary first (FR-013).
pub fn all_actions(item: &ItemSummary) -> Vec<ActionEntry> {
    let primary = primary_action(item);
    let username = username_field(item).map(|f| f.name.as_str());
    let mut entries = Vec::new();
    if let Some(source) = &primary {
        entries.push(entry(source.clone(), Some(Action::CopyPrimary)));
    }
    for field in &item.fields {
        let source = CopySource::Field(field.clone());
        if primary.as_ref() == Some(&source) {
            continue;
        }
        let shortcut = if Some(field.name.as_str()) == username {
            Some(Action::CopyUsername)
        } else if field.name == "url" {
            Some(Action::CopyUrl)
        } else {
            None
        };
        entries.push(entry(source, shortcut));
    }
    for (i, field) in item.totp_fields.iter().enumerate() {
        let source = CopySource::Totp {
            field: field.clone(),
        };
        if primary.as_ref() == Some(&source) {
            continue;
        }
        let shortcut = (i == 0).then_some(Action::CopyTotp);
        entries.push(entry(source, shortcut));
    }
    entries
}

fn entry(source: CopySource, shortcut: Option<Action>) -> ActionEntry {
    let label = match &source {
        CopySource::Field(f) => f.label.clone(),
        CopySource::Totp { field } if field == "totp_uri" => "One-time code".to_owned(),
        CopySource::Totp { field } => field.clone(),
    };
    ActionEntry {
        source,
        label,
        shortcut,
    }
}

fn username_field(item: &ItemSummary) -> Option<&FieldRef> {
    item.field("username").or_else(|| item.field("email"))
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
    item.field("url").cloned().map(CopySource::Field)
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
