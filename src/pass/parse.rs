//! Lenient parsing of `pass-cli` JSON output; strips secret fields.
//!
//! Items are deserialized into typed structs that declare only non-secret fields.
//! Secret values are skipped by serde without being copied into owned strings, except
//! where a field's emptiness matters ([`NonEmpty`]), which only borrows or zeroizes.

use std::collections::BTreeMap;
use std::fmt;

use secrecy::SecretString;
use serde::Deserialize;
use serde::de::{self, IgnoredAny, MapAccess, Visitor};
use zeroize::Zeroize;

use super::error::PassError;
use crate::model::{
    AccountId, FieldRef, ItemId, ItemKey, ItemKind, ItemSummary, ShareId, Vault, parse_timestamp,
};

type Result<T> = std::result::Result<T, PassError>;

fn json<'a, T: Deserialize<'a>>(bytes: &'a [u8], command: &'static str) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|_| PassError::Protocol { command })
}

pub fn parse_account(bytes: &[u8]) -> Result<AccountId> {
    #[derive(Deserialize)]
    struct Info {
        id: String,
    }
    let info: Info = json(bytes, "info")?;
    Ok(AccountId(info.id))
}

#[derive(Deserialize)]
struct RawVault {
    #[serde(alias = "shareId")]
    share_id: String,
    name: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum VaultList {
    Wrapped { vaults: Vec<RawVault> },
    Bare(Vec<RawVault>),
}

pub fn parse_vaults(bytes: &[u8]) -> Result<Vec<Vault>> {
    let list: VaultList = json(bytes, "vault list")?;
    let (VaultList::Wrapped { vaults } | VaultList::Bare(vaults)) = list;
    Ok(vaults
        .into_iter()
        .map(|v| Vault {
            share_id: ShareId(v.share_id),
            name: v.name,
        })
        .collect())
}

pub fn parse_items(bytes: &[u8], share: &ShareId, vault_name: &str) -> Result<Vec<ItemSummary>> {
    #[derive(Deserialize)]
    struct ItemList {
        items: Vec<RawItem>,
    }
    let list: ItemList = json(bytes, "item list")?;
    Ok(list
        .items
        .into_iter()
        .filter(|i| !i.state.eq_ignore_ascii_case("trashed"))
        .map(|i| i.into_summary(share, vault_name))
        .collect())
}

/// Raw `item view --field` output: the value plus one trailing newline.
pub fn parse_field(bytes: &[u8]) -> Result<SecretString> {
    let mut text = String::from_utf8(bytes.to_vec()).map_err(|e| {
        e.into_bytes().zeroize();
        PassError::Protocol {
            command: "item view",
        }
    })?;
    if text.ends_with("\r\n") {
        text.truncate(text.len() - 2);
    } else if text.ends_with('\n') {
        text.truncate(text.len() - 1);
    }
    Ok(SecretString::from(text))
}

pub fn parse_totp(bytes: &[u8]) -> Result<BTreeMap<String, SecretString>> {
    let codes: BTreeMap<String, String> = json(bytes, "item totp")?;
    Ok(codes
        .into_iter()
        .map(|(name, code)| (name, SecretString::from(code)))
        .collect())
}

// ---------------------------------------------------------------------------------------------
// Item structure

#[derive(Deserialize)]
struct RawItem {
    id: String,
    #[serde(default)]
    share_id: Option<String>,
    #[serde(default)]
    state: String,
    #[serde(default)]
    modify_time: String,
    /// Plain listing only.
    #[serde(default)]
    title: Option<String>,
    /// Plain listing only.
    #[serde(default)]
    item_type: Option<String>,
    /// `--show-secrets` listing only.
    #[serde(default)]
    content: Option<RawContent>,
}

#[derive(Deserialize)]
struct RawContent {
    #[serde(default)]
    title: String,
    #[serde(default)]
    note: NonEmpty,
    #[serde(default)]
    content: KindContent,
    #[serde(default)]
    extra_fields: Vec<RawCustomField>,
}

#[derive(Deserialize)]
struct RawCustomField {
    name: String,
    content: CustomFieldKind,
}

#[derive(Deserialize)]
struct RawSection {
    #[serde(default)]
    section_fields: Vec<RawCustomField>,
}

/// The variant of a custom field (`Text`, `Hidden`, `Totp`); the value is never kept.
enum CustomFieldKind {
    Text,
    Hidden,
    Totp,
    Other,
}

impl<'de> Deserialize<'de> for CustomFieldKind {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = CustomFieldKind;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a custom field content map")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut kind = CustomFieldKind::Other;
                while let Some(key) = map.next_key::<KeyName<'de>>()? {
                    map.next_value::<IgnoredAny>()?;
                    kind = match key.as_str() {
                        "Text" => CustomFieldKind::Text,
                        "Hidden" => CustomFieldKind::Hidden,
                        "Totp" => CustomFieldKind::Totp,
                        _ => CustomFieldKind::Other,
                    };
                }
                Ok(kind)
            }
        }
        d.deserialize_map(V)
    }
}

/// Map key that borrows when possible.
#[derive(Deserialize)]
struct KeyName<'a>(#[serde(borrow)] std::borrow::Cow<'a, str>);

impl KeyName<'_> {
    fn as_str(&self) -> &str {
        &self.0
    }
}

/// Whether a string value is non-empty. The value itself is never kept: borrowed input is
/// only inspected, and owned copies (strings with escapes) are zeroized.
#[derive(Default, Clone, Copy)]
struct NonEmpty(bool);

impl<'de> Deserialize<'de> for NonEmpty {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = NonEmpty;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string or null")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<NonEmpty, E> {
                Ok(NonEmpty(!v.is_empty()))
            }
            fn visit_string<E: de::Error>(self, mut v: String) -> std::result::Result<NonEmpty, E> {
                let present = !v.is_empty();
                v.zeroize();
                Ok(NonEmpty(present))
            }
            fn visit_none<E: de::Error>(self) -> std::result::Result<NonEmpty, E> {
                Ok(NonEmpty(false))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<NonEmpty, E> {
                Ok(NonEmpty(false))
            }
        }
        d.deserialize_any(V)
    }
}

/// Non-secret string: empty strings become `None`.
fn text<'de, D: de::Deserializer<'de>>(d: D) -> std::result::Result<Option<String>, D::Error> {
    Ok(Option::<String>::deserialize(d)?.filter(|s| !s.trim().is_empty()))
}

#[derive(Deserialize, Default)]
struct LoginBody {
    #[serde(default, deserialize_with = "text")]
    username: Option<String>,
    #[serde(default, deserialize_with = "text")]
    email: Option<String>,
    #[serde(default)]
    urls: Vec<String>,
    #[serde(default)]
    password: NonEmpty,
    #[serde(default)]
    totp_uri: NonEmpty,
}

#[derive(Deserialize, Default)]
struct CardBody {
    #[serde(default, deserialize_with = "text")]
    cardholder_name: Option<String>,
    #[serde(default, deserialize_with = "text")]
    expiration_date: Option<String>,
    #[serde(default)]
    number: NonEmpty,
    #[serde(default)]
    verification_number: NonEmpty,
    #[serde(default)]
    pin: NonEmpty,
}

#[derive(Deserialize, Default)]
struct IdentityBody {
    #[serde(default, deserialize_with = "text")]
    full_name: Option<String>,
    #[serde(default, deserialize_with = "text")]
    email: Option<String>,
    #[serde(default)]
    social_security_number: NonEmpty,
    #[serde(default)]
    passport_number: NonEmpty,
    #[serde(default)]
    license_number: NonEmpty,
}

#[derive(Deserialize, Default)]
struct WifiBody {
    #[serde(default, deserialize_with = "text")]
    ssid: Option<String>,
    #[serde(default)]
    password: NonEmpty,
    #[serde(default)]
    sections: Vec<RawSection>,
}

#[derive(Deserialize, Default)]
struct SshBody {
    #[serde(default, deserialize_with = "text")]
    public_key: Option<String>,
    #[serde(default)]
    private_key: NonEmpty,
    #[serde(default)]
    sections: Vec<RawSection>,
}

#[derive(Deserialize, Default)]
struct CustomBody {
    #[serde(default)]
    sections: Vec<RawSection>,
}

/// `content.content`: a map with a single key naming the item kind.
#[derive(Default)]
enum KindContent {
    Login(LoginBody),
    Card(CardBody),
    Identity(IdentityBody),
    Wifi(WifiBody),
    Ssh(SshBody),
    Custom(CustomBody),
    Note,
    Alias,
    Unknown(String),
    #[default]
    Missing,
}

impl<'de> Deserialize<'de> for KindContent {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = KindContent;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an item content map")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut kind = KindContent::Missing;
                while let Some(key) = map.next_key::<KeyName<'de>>()? {
                    let name = key.as_str();
                    kind = match ItemKind::from_cli(name) {
                        ItemKind::Login => KindContent::Login(opt(map.next_value()?)),
                        ItemKind::CreditCard => KindContent::Card(opt(map.next_value()?)),
                        ItemKind::Identity => KindContent::Identity(opt(map.next_value()?)),
                        ItemKind::Wifi => KindContent::Wifi(opt(map.next_value()?)),
                        ItemKind::SshKey => KindContent::Ssh(opt(map.next_value()?)),
                        ItemKind::Custom => KindContent::Custom(opt(map.next_value()?)),
                        ItemKind::Note => {
                            map.next_value::<IgnoredAny>()?;
                            KindContent::Note
                        }
                        ItemKind::Alias => {
                            map.next_value::<IgnoredAny>()?;
                            KindContent::Alias
                        }
                        ItemKind::Unknown(_) => {
                            map.next_value::<IgnoredAny>()?;
                            KindContent::Unknown(name.to_owned())
                        }
                    };
                }
                Ok(kind)
            }
        }
        d.deserialize_map(V)
    }
}

fn opt<T: Default>(value: Option<T>) -> T {
    value.unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// Conversion to summaries

impl RawItem {
    fn into_summary(self, share: &ShareId, vault_name: &str) -> ItemSummary {
        let share = self.share_id.map_or_else(|| share.clone(), ShareId);
        let mut summary = ItemSummary {
            key: ItemKey {
                share,
                item: ItemId(self.id),
            },
            vault_name: vault_name.to_owned(),
            kind: self
                .item_type
                .as_deref()
                .map_or(ItemKind::Unknown(String::new()), ItemKind::from_cli),
            title: self.title.unwrap_or_default(),
            username: None,
            email: None,
            subtitle: None,
            urls: Vec::new(),
            totp_fields: Vec::new(),
            fields: Vec::new(),
            modified_at: parse_timestamp(&self.modify_time).unwrap_or(0),
        };
        if let Some(content) = self.content {
            content.fill(&mut summary);
        }
        summary
    }
}

fn secret_if(fields: &mut Vec<FieldRef>, present: NonEmpty, name: &str, label: &str) {
    if present.0 {
        fields.push(FieldRef::secret(name, label));
    }
}

fn plain_if(fields: &mut Vec<FieldRef>, value: &Option<String>, name: &str, label: &str) {
    if let Some(v) = value {
        fields.push(FieldRef::plain(name, label, v.clone()));
    }
}

/// Field name and label for the `i`-th website of a login.
///
/// The first one keeps the plain `url` name that the copy-website shortcut binds to; the
/// rest are numbered. The action list already shows each value underneath its label, so a
/// number is enough to tell them apart and stays stable for URLs with no readable host.
fn website_field(i: usize) -> (String, String) {
    if i == 0 {
        ("url".to_owned(), "Website".to_owned())
    } else {
        (format!("url{}", i + 1), format!("Website {}", i + 1))
    }
}

fn custom_fields(s: &mut ItemSummary, raw: Vec<RawCustomField>) {
    for f in raw {
        match f.content {
            CustomFieldKind::Hidden => s.fields.push(FieldRef::secret(f.name.clone(), f.name)),
            CustomFieldKind::Totp => s.totp_fields.push(f.name),
            CustomFieldKind::Text | CustomFieldKind::Other => {
                s.fields.push(FieldRef::unstored(f.name.clone(), f.name));
            }
        }
    }
}

fn section_fields(s: &mut ItemSummary, sections: Vec<RawSection>) {
    for section in sections {
        custom_fields(s, section.section_fields);
    }
}

impl RawContent {
    fn fill(self, s: &mut ItemSummary) {
        s.title = self.title;
        let f = &mut s.fields;
        match self.content {
            KindContent::Login(b) => {
                s.kind = ItemKind::Login;
                plain_if(f, &b.username, "username", "Username");
                plain_if(f, &b.email, "email", "Email");
                secret_if(f, b.password, "password", "Password");
                for (i, url) in b.urls.iter().enumerate() {
                    let (name, label) = website_field(i);
                    f.push(FieldRef::plain(name, label, url.clone()));
                }
                if b.totp_uri.0 {
                    s.totp_fields.push("totp_uri".into());
                }
                s.subtitle = b.username.clone().or_else(|| b.email.clone());
                s.username = b.username;
                s.email = b.email;
                s.urls = b.urls;
            }
            KindContent::Card(b) => {
                s.kind = ItemKind::CreditCard;
                secret_if(f, b.number, "number", "Card number");
                plain_if(f, &b.cardholder_name, "cardholder_name", "Cardholder");
                plain_if(f, &b.expiration_date, "expiration_date", "Expiration date");
                secret_if(
                    f,
                    b.verification_number,
                    "verification_number",
                    "Security code",
                );
                secret_if(f, b.pin, "pin", "PIN");
                s.subtitle = b.cardholder_name;
            }
            KindContent::Identity(b) => {
                s.kind = ItemKind::Identity;
                plain_if(f, &b.full_name, "full_name", "Full name");
                plain_if(f, &b.email, "email", "Email");
                secret_if(
                    f,
                    b.social_security_number,
                    "social_security_number",
                    "Social security number",
                );
                secret_if(f, b.passport_number, "passport_number", "Passport number");
                secret_if(f, b.license_number, "license_number", "License number");
                s.email = b.email;
                s.subtitle = b.full_name;
            }
            KindContent::Wifi(b) => {
                s.kind = ItemKind::Wifi;
                secret_if(f, b.password, "password", "Password");
                plain_if(f, &b.ssid, "ssid", "Network name");
                s.subtitle = b.ssid;
                section_fields(s, b.sections);
            }
            KindContent::Ssh(b) => {
                s.kind = ItemKind::SshKey;
                secret_if(f, b.private_key, "private_key", "Private key");
                plain_if(f, &b.public_key, "public_key", "Public key");
                section_fields(s, b.sections);
            }
            KindContent::Custom(b) => {
                s.kind = ItemKind::Custom;
                section_fields(s, b.sections);
            }
            KindContent::Note => s.kind = ItemKind::Note,
            KindContent::Alias => s.kind = ItemKind::Alias,
            KindContent::Unknown(name) => s.kind = ItemKind::Unknown(name),
            KindContent::Missing => {}
        }
        custom_fields(s, self.extra_fields);
        if self.note.0 {
            let note = FieldRef::secret("note", "Note");
            if s.kind == ItemKind::Note {
                s.fields.insert(0, note);
            } else {
                s.fields.push(note);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ItemKey, ItemKind};
    use secrecy::ExposeSecret;
    use std::path::PathBuf;

    const MARKER: &str = "SECRET-FIXTURE-";

    fn fixture(dir: &str, name: &str) -> Vec<u8> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/pass-cli")
            .join(dir)
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn synthetic_items() -> Vec<ItemSummary> {
        let mut items = parse_items(
            &fixture("synthetic", "item-list-share-a.json"),
            &ShareId("share-a".into()),
            "Personal",
        )
        .unwrap();
        items.extend(
            parse_items(
                &fixture("synthetic", "item-list--share-b.json"),
                &ShareId("-share-b".into()),
                "Work",
            )
            .unwrap(),
        );
        items
    }

    fn find<'a>(items: &'a [ItemSummary], id: &str) -> &'a ItemSummary {
        items.iter().find(|i| i.key.item.0 == id).unwrap()
    }

    fn assert_no_secrets(items: &[ItemSummary]) {
        let debug = format!("{items:?}");
        let json = serde_json::to_string(items).unwrap();
        for text in [debug, json] {
            assert!(!text.contains(MARKER), "secret leaked: {text}");
            assert!(!text.contains("otpauth"), "totp uri leaked");
            assert!(!text.contains("SECRETFIXTURE"), "totp secret leaked");
        }
    }

    #[test]
    fn account_id() {
        let id = parse_account(&fixture("synthetic", "info.json")).unwrap();
        assert_eq!(id, AccountId("account-1".into()));
    }

    #[test]
    fn vaults() {
        let vaults = parse_vaults(&fixture("synthetic", "vault-list.json")).unwrap();
        assert_eq!(
            vaults,
            vec![
                Vault {
                    share_id: ShareId("share-a".into()),
                    name: "Personal".into()
                },
                Vault {
                    share_id: ShareId("-share-b".into()),
                    name: "Work".into()
                },
            ]
        );
    }

    #[test]
    fn vaults_accept_bare_array_and_camel_case() {
        let vaults = parse_vaults(br#"[{"shareId":"x","name":"X"}]"#).unwrap();
        assert_eq!(vaults[0].share_id, ShareId("x".into()));
    }

    #[test]
    fn items_parse_and_drop_trashed() {
        let items = synthetic_items();
        assert_eq!(items.len(), 11);
        assert!(items.iter().all(|i| i.key.item.0 != "login-old"));
    }

    #[test]
    fn items_contain_no_secret_values() {
        assert_no_secrets(&synthetic_items());
    }

    #[test]
    fn login_summary() {
        let items = synthetic_items();
        let gh = find(&items, "login-github");
        assert_eq!(gh.key, ItemKey::new("share-a", "login-github"));
        assert_eq!(gh.kind, ItemKind::Login);
        assert_eq!(gh.vault_name, "Personal");
        assert_eq!(gh.username.as_deref(), Some("octocat"));
        assert_eq!(gh.email.as_deref(), Some("octo@example.invalid"));
        assert_eq!(gh.subtitle.as_deref(), Some("octocat"));
        assert_eq!(gh.urls.len(), 2);
        assert_eq!(gh.totp_fields, vec!["totp_uri", "Backup TOTP"]);
        assert!(gh.field("password").is_some_and(|f| f.secret));
        assert!(gh.field("note").is_some_and(|f| f.secret));
        assert!(
            gh.field("Recovery code")
                .is_some_and(|f| f.secret && f.value.is_none())
        );
        assert!(
            gh.field("Account id")
                .is_some_and(|f| !f.secret && f.value.is_none())
        );
        assert_eq!(gh.modified_at, 1_770_091_506);
    }

    #[test]
    fn every_website_becomes_a_copyable_field() {
        let items = synthetic_items();
        let gh = find(&items, "login-github");
        let websites: Vec<_> = gh
            .fields
            .iter()
            .filter(|f| f.name.starts_with("url"))
            .map(|f| {
                (
                    f.name.as_str(),
                    f.label.as_str(),
                    f.value.as_deref(),
                    f.secret,
                )
            })
            .collect();
        assert_eq!(
            websites,
            vec![
                ("url", "Website", Some("https://github.com/login"), false),
                ("url2", "Website 2", Some("https://gist.github.com"), false),
            ]
        );
        let mail = find(&items, "login-mail");
        assert!(
            mail.field("url2").is_none(),
            "a single website must not be numbered"
        );
        assert_eq!(mail.field("url").map(|f| f.label.as_str()), Some("Website"));
    }

    #[test]
    fn login_without_username_uses_email_and_has_no_totp() {
        let items = synthetic_items();
        let mail = find(&items, "login-mail");
        assert_eq!(mail.username, None);
        assert_eq!(mail.subtitle.as_deref(), Some("me@example.invalid"));
        assert!(!mail.has_totp());
        assert!(
            mail.field("note").is_none(),
            "empty note must not be offered"
        );
    }

    #[test]
    fn other_kinds() {
        let items = synthetic_items();
        let note = find(&items, "note-wifi-codes");
        assert_eq!(note.kind, ItemKind::Note);
        assert!(note.field("note").is_some());
        // Notes show the title alone: the body is secret, so there is no preview to show.
        assert_eq!(note.subtitle, None);

        let card = find(&items, "card-visa");
        assert_eq!(card.kind, ItemKind::CreditCard);
        assert_eq!(card.subtitle.as_deref(), Some("Fixture Holder"));
        for name in ["number", "verification_number", "pin"] {
            assert!(card.field(name).is_some_and(|f| f.secret), "{name}");
        }
        assert_eq!(
            card.field("expiration_date")
                .and_then(|f| f.value.as_deref()),
            Some("2030-01")
        );

        let wifi = find(&items, "wifi-home");
        assert_eq!(wifi.subtitle.as_deref(), Some("fixture-ssid"));
        assert!(wifi.field("password").is_some_and(|f| f.secret));

        let ssh = find(&items, "ssh-server");
        assert!(ssh.field("private_key").is_some_and(|f| f.secret));
        assert!(ssh.field("Passphrase").is_some_and(|f| f.secret));
        assert!(ssh.field("Hostname").is_some_and(|f| !f.secret));

        let custom = find(&items, "custom-api");
        assert_eq!(custom.kind, ItemKind::Custom);
        assert_eq!(custom.display_title(), "(untitled)");
        assert!(custom.field("Token").is_some_and(|f| f.secret));

        let identity = find(&items, "identity-me");
        assert_eq!(identity.subtitle.as_deref(), Some("Fixture Person"));

        let future = find(&items, "future-kind");
        assert_eq!(future.kind, ItemKind::Unknown("PasskeyThing".into()));

        let alias = find(&items, "alias-shop");
        assert_eq!(alias.kind, ItemKind::Alias);
        assert_eq!(alias.vault_name, "Work");
        assert_eq!(alias.key.share, ShareId("-share-b".into()));
    }

    #[test]
    fn plain_listing_uses_item_type() {
        let items = parse_items(
            &fixture("synthetic", "item-list-plain-share-a.json"),
            &ShareId("share-a".into()),
            "Personal",
        )
        .unwrap();
        let kinds: Vec<_> = items.iter().map(|i| i.kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![ItemKind::Login, ItemKind::CreditCard, ItemKind::SshKey]
        );
        assert_eq!(items[0].title, "GitHub");
    }

    #[test]
    fn captured_fixtures_parse_without_secrets() {
        let dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pass-cli/captured");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        let mut parsed = 0;
        for entry in entries {
            let name = entry.unwrap().file_name().into_string().unwrap();
            if name.starts_with("item-list") {
                let items =
                    parse_items(&fixture("captured", &name), &ShareId("s".into()), "V").unwrap();
                assert!(!items.is_empty(), "{name}");
                assert_no_secrets(&items);
                parsed += 1;
            }
        }
        assert!(parsed > 0);
        parse_vaults(&fixture("captured", "vault-list.json")).unwrap();
        parse_account(&fixture("captured", "info.json")).unwrap();
    }

    #[test]
    fn snapshot_of_synthetic_items() {
        insta::assert_json_snapshot!(synthetic_items());
    }

    #[test]
    fn malformed_output_is_protocol_error() {
        let share = ShareId("s".into());
        assert!(matches!(
            parse_items(b"not json", &share, "V"),
            Err(PassError::Protocol { .. })
        ));
        assert!(matches!(
            parse_vaults(b"{}"),
            Err(PassError::Protocol { .. })
        ));
        assert!(matches!(
            parse_account(b"{}"),
            Err(PassError::Protocol { .. })
        ));
    }

    #[test]
    fn field_strips_one_trailing_newline() {
        let v = parse_field(b"line one\nline two\n\n").unwrap();
        assert_eq!(v.expose_secret(), "line one\nline two\n");
        let v = parse_field(b"secret\r\n").unwrap();
        assert_eq!(v.expose_secret(), "secret");
        assert!(matches!(
            parse_field(&[0xff, 0xfe]),
            Err(PassError::Protocol { .. })
        ));
    }

    #[test]
    fn totp_codes() {
        let codes =
            parse_totp(&fixture("synthetic", "item-totp-share-a-login-github.json")).unwrap();
        assert_eq!(codes["totp_uri"].expose_secret(), "123456");
        assert_eq!(codes["Backup TOTP"].expose_secret(), "654321");
        assert!(parse_totp(b"{}").unwrap().is_empty());
    }
}
