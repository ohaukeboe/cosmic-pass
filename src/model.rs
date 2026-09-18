//! Domain types: vaults, items, and field references.
//!
//! Everything here is non-secret metadata. Secret values never enter these types.

use serde::{Deserialize, Serialize};

/// Vault share ID from `pass-cli`. May change across sessions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ShareId(pub String);

/// Item ID. Unique only within a share.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ItemId(pub String);

/// Proton account ID from `pass-cli info`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AccountId(pub String);

/// Global item identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ItemKey {
    pub share: ShareId,
    pub item: ItemId,
}

impl ItemKey {
    pub fn new(share: impl Into<String>, item: impl Into<String>) -> Self {
        Self {
            share: ShareId(share.into()),
            item: ItemId(item.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vault {
    pub share_id: ShareId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemKind {
    Login,
    Note,
    CreditCard,
    Identity,
    Alias,
    SshKey,
    Wifi,
    Custom,
    /// A kind this version does not know. Shown with a generic icon.
    Unknown(String),
}

impl ItemKind {
    /// Parses both the `item_type` form (`credit_card`) and the content key form
    /// (`CreditCard`). Case, `-`, and `_` are ignored.
    pub fn from_cli(name: &str) -> Self {
        let normalized: String = name
            .chars()
            .filter(|c| *c != '_' && *c != '-')
            .map(|c| c.to_ascii_lowercase())
            .collect();
        match normalized.as_str() {
            "login" => Self::Login,
            "note" => Self::Note,
            "creditcard" => Self::CreditCard,
            "identity" => Self::Identity,
            "alias" => Self::Alias,
            "sshkey" => Self::SshKey,
            "wifi" => Self::Wifi,
            "custom" => Self::Custom,
            _ => Self::Unknown(name.to_owned()),
        }
    }
}

/// A copyable field of an item. Holds a value only for non-secret fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldRef {
    /// `pass-cli` field name (`password`, `number`, a custom field name, ...).
    pub name: String,
    /// Human-readable label for the action list.
    pub label: String,
    /// Secret fields are masked in the UI and cleared from the clipboard after a timeout.
    pub secret: bool,
    /// The value, if it is non-secret and known without calling `pass-cli`.
    pub value: Option<String>,
}

impl FieldRef {
    /// A secret field whose value must be fetched on demand.
    pub fn secret(name: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            secret: true,
            value: None,
        }
    }

    /// A non-secret field whose value is stored in the summary.
    pub fn plain(name: impl Into<String>, label: impl Into<String>, value: String) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            secret: false,
            value: Some(value),
        }
    }

    /// A non-secret field whose value is not stored (fetched on demand, no clipboard timeout).
    pub fn unstored(name: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            secret: false,
            value: None,
        }
    }
}

/// Non-secret, searchable, cacheable summary of an item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemSummary {
    pub key: ItemKey,
    pub vault_name: String,
    pub kind: ItemKind,
    pub title: String,
    pub username: Option<String>,
    pub email: Option<String>,
    /// Secondary line: username/email, card holder, identity name, or SSID. `None` for
    /// notes, aliases, SSH keys, and custom items, whose only distinguishing content is
    /// secret.
    pub subtitle: Option<String>,
    pub urls: Vec<String>,
    /// Names of fields that produce one-time codes (`totp_uri`, custom TOTP names).
    pub totp_fields: Vec<String>,
    /// Standard and custom copyable fields, in display order. Excludes TOTP fields.
    pub fields: Vec<FieldRef>,
    /// Unix seconds.
    pub modified_at: i64,
}

impl ItemSummary {
    pub fn display_title(&self) -> &str {
        if self.title.trim().is_empty() {
            "(untitled)"
        } else {
            &self.title
        }
    }

    pub fn has_totp(&self) -> bool {
        !self.totp_fields.is_empty()
    }

    pub fn field(&self, name: &str) -> Option<&FieldRef> {
        self.fields.iter().find(|f| f.name == name)
    }
}

/// Version of the [`CacheFile`] payload.
pub const CACHE_FORMAT_VERSION: u16 = 1;

/// Plaintext of the encrypted metadata cache. Holds no secret values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheFile {
    pub format_version: u16,
    pub account: AccountId,
    pub fetched_at: i64,
    pub vaults: Vec<Vault>,
    pub items: Vec<ItemSummary>,
    pub usage: Vec<crate::core::usage::UsageRecord>,
}

/// Parses `YYYY-MM-DDTHH:MM:SS` (UTC) into Unix seconds.
pub fn parse_timestamp(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' {
        return None;
    }
    let num = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    Some(days_from_civil(y, m, d) * 86_400 + hh * 3_600 + mm * 60 + ss)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn summary(title: &str) -> ItemSummary {
        ItemSummary {
            key: ItemKey::new("s", "i"),
            vault_name: "Personal".into(),
            kind: ItemKind::Login,
            title: title.into(),
            username: None,
            email: None,
            subtitle: None,
            urls: vec![],
            totp_fields: vec![],
            fields: vec![],
            modified_at: 0,
        }
    }

    #[test]
    fn kind_parses_item_type_and_content_key_forms() {
        assert_eq!(ItemKind::from_cli("credit-card"), ItemKind::CreditCard);
        assert_eq!(ItemKind::from_cli("credit_card"), ItemKind::CreditCard);
        assert_eq!(ItemKind::from_cli("CreditCard"), ItemKind::CreditCard);
        assert_eq!(ItemKind::from_cli("ssh_key"), ItemKind::SshKey);
        assert_eq!(ItemKind::from_cli("SshKey"), ItemKind::SshKey);
        assert_eq!(ItemKind::from_cli("login"), ItemKind::Login);
        assert_eq!(ItemKind::from_cli("Wifi"), ItemKind::Wifi);
    }

    #[test]
    fn unknown_kind_is_kept() {
        assert_eq!(
            ItemKind::from_cli("passkey-thing"),
            ItemKind::Unknown("passkey-thing".into())
        );
    }

    #[test]
    fn item_key_uses_share_and_item() {
        let a = ItemKey::new("s1", "i1");
        let b = ItemKey::new("s2", "i1");
        assert_ne!(a, b);
        let set: HashSet<_> = [a.clone(), b, a].into_iter().collect();
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn empty_title_displays_untitled() {
        assert_eq!(summary("").display_title(), "(untitled)");
        assert_eq!(summary("  ").display_title(), "(untitled)");
        assert_eq!(summary("GitHub").display_title(), "GitHub");
    }

    #[test]
    fn secret_field_ref_holds_no_value() {
        let f = FieldRef::secret("password", "Password");
        assert!(f.secret);
        assert_eq!(f.value, None);
        let p = FieldRef::plain("username", "Username", "octo".into());
        assert!(!p.secret);
        assert_eq!(p.value.as_deref(), Some("octo"));
    }

    #[test]
    fn has_totp_and_field_lookup() {
        let mut s = summary("x");
        assert!(!s.has_totp());
        s.totp_fields.push("totp_uri".into());
        s.fields.push(FieldRef::secret("password", "Password"));
        assert!(s.has_totp());
        assert!(s.field("password").is_some());
        assert!(s.field("pin").is_none());
    }

    #[test]
    fn timestamps_parse_to_unix_seconds() {
        assert_eq!(parse_timestamp("1970-01-01T00:00:00"), Some(0));
        assert_eq!(parse_timestamp("2000-03-01T00:00:00"), Some(951_868_800));
        assert_eq!(parse_timestamp("2026-02-03T04:05:06"), Some(1_770_091_506));
        assert_eq!(parse_timestamp("2026-13-03T04:05:06"), None);
        assert_eq!(parse_timestamp("garbage"), None);
    }
}
