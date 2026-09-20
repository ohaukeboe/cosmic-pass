//! Lenient parsing of `pass-cli` JSON output; strips secret fields.
//!
//! Items are deserialized into typed structs that declare only non-secret fields.
//! Secret values are skipped by serde without being copied into owned strings, except
//! where a field's emptiness matters ([`NonEmpty`]), which only borrows or zeroizes.

use std::collections::{BTreeMap, HashSet};
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

/// Every member `pass-cli` emits for an identity. Only `full_name` and `email` are kept as
/// values; the rest are personal data, so only their presence is recorded and the value is
/// fetched on demand.
#[derive(Deserialize, Default)]
struct IdentityBody {
    #[serde(default, deserialize_with = "text")]
    full_name: Option<String>,
    #[serde(default, deserialize_with = "text")]
    email: Option<String>,
    #[serde(default)]
    phone_number: NonEmpty,
    #[serde(default)]
    first_name: NonEmpty,
    #[serde(default)]
    middle_name: NonEmpty,
    #[serde(default)]
    last_name: NonEmpty,
    #[serde(default)]
    birthdate: NonEmpty,
    #[serde(default)]
    gender: NonEmpty,
    #[serde(default)]
    extra_personal_details: Vec<RawCustomField>,
    #[serde(default)]
    organization: NonEmpty,
    #[serde(default)]
    street_address: NonEmpty,
    #[serde(default)]
    zip_or_postal_code: NonEmpty,
    #[serde(default)]
    city: NonEmpty,
    #[serde(default)]
    state_or_province: NonEmpty,
    #[serde(default)]
    country_or_region: NonEmpty,
    #[serde(default)]
    floor: NonEmpty,
    #[serde(default)]
    county: NonEmpty,
    #[serde(default)]
    extra_address_details: Vec<RawCustomField>,
    #[serde(default)]
    social_security_number: NonEmpty,
    #[serde(default)]
    passport_number: NonEmpty,
    #[serde(default)]
    license_number: NonEmpty,
    #[serde(default)]
    website: NonEmpty,
    #[serde(default)]
    x_handle: NonEmpty,
    #[serde(default)]
    second_phone_number: NonEmpty,
    #[serde(default)]
    linkedin: NonEmpty,
    #[serde(default)]
    reddit: NonEmpty,
    #[serde(default)]
    facebook: NonEmpty,
    #[serde(default)]
    yahoo: NonEmpty,
    #[serde(default)]
    instagram: NonEmpty,
    #[serde(default)]
    extra_contact_details: Vec<RawCustomField>,
    #[serde(default)]
    company: NonEmpty,
    #[serde(default)]
    job_title: NonEmpty,
    #[serde(default)]
    personal_website: NonEmpty,
    #[serde(default)]
    work_phone_number: NonEmpty,
    #[serde(default)]
    work_email: NonEmpty,
    #[serde(default)]
    extra_work_details: Vec<RawCustomField>,
    /// Identities name their sections `extra_sections`; the other kinds call theirs `sections`.
    #[serde(default, rename = "extra_sections", alias = "sections")]
    sections: Vec<RawSection>,
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
        dedupe_selectors(&mut summary);
        summary
    }
}

/// Drops every field and one-time code whose `pass-cli` selector another entry of the same
/// item repeats, keeping the one `pass-cli` actually resolves.
///
/// [`FieldRef::name`] is the selector, not just a label. `item view --field=` resolves a
/// repeated name to the LAST field carrying it (verified against `pass-cli` with a login
/// holding a password beside a custom Hidden field of the same name), and `item totp`
/// returns a map keyed by name, so an earlier namesake cannot be addressed at all. Offering
/// one anyway rendered the last field's value under the earlier field's label, which could
/// put a login password on screen or on the clipboard under an unrelated name
/// (cosmic-pass-wqx.41). The survivor keeps its own position and label; the shadowed entry
/// is unreachable either way, because `pass-cli` exposes no section-qualified or positional
/// selector to address it with.
///
/// This runs once per item, after every source has pushed into the summary - the kind body,
/// `extra_fields`, each section, and the trailing note - because a collision can span any
/// two of them. Fields and one-time codes are separate namespaces: they are read back by
/// different commands.
fn dedupe_selectors(s: &mut ItemSummary) {
    fn keep_last<T>(items: &mut Vec<T>, name: impl Fn(&T) -> &str) {
        let mut seen: HashSet<String> = HashSet::new();
        items.reverse();
        items.retain(|i| seen.insert(name(i).to_owned()));
        items.reverse();
    }
    keep_last(&mut s.fields, |f| f.name.as_str());
    keep_last(&mut s.totp_fields, |n| n.as_str());
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

/// Offers each present member as a non-secret field whose value is fetched on demand.
fn unstored_if<const N: usize>(fields: &mut Vec<FieldRef>, members: [(NonEmpty, &str, &str); N]) {
    for (present, name, label) in members {
        if present.0 {
            fields.push(FieldRef::unstored(name, label));
        }
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

/// Offers the custom fields whose value the app can produce on demand.
///
/// `pass-cli` reads a hidden field and a one-time code back by name, so those become a secret
/// field and a code. It hands the app no value for a text field — nor for a content kind this
/// version has no name for — so a row for one could only ever render the empty placeholder and
/// could never be copied; both are dropped here (FR-113). The parser is the right place for
/// that: every consumer reads from it, so a dropped field reaches neither the field list, nor
/// a copy shortcut, nor the primary-field choice, nor the on-disk cache.
fn custom_fields(s: &mut ItemSummary, raw: Vec<RawCustomField>) {
    for f in raw {
        match f.content {
            CustomFieldKind::Hidden => s.fields.push(FieldRef::secret(f.name.clone(), f.name)),
            CustomFieldKind::Totp => s.totp_fields.push(f.name),
            CustomFieldKind::Text | CustomFieldKind::Other => {}
        }
    }
}

fn section_fields(s: &mut ItemSummary, sections: Vec<RawSection>) {
    for section in sections {
        custom_fields(s, section.section_fields);
    }
}

impl IdentityBody {
    /// Fills the summary in the order the Proton Pass UI groups identity members: personal
    /// details, address, contact, then work, each followed by its own custom fields.
    fn fill(self, s: &mut ItemSummary) {
        let f = &mut s.fields;
        plain_if(f, &self.full_name, "full_name", "Full name");
        plain_if(f, &self.email, "email", "Email");
        unstored_if(
            f,
            [
                (self.phone_number, "phone_number", "Phone number"),
                (self.first_name, "first_name", "First name"),
                (self.middle_name, "middle_name", "Middle name"),
                (self.last_name, "last_name", "Last name"),
                (self.birthdate, "birthdate", "Birthdate"),
                (self.gender, "gender", "Gender"),
            ],
        );
        custom_fields(s, self.extra_personal_details);
        unstored_if(
            &mut s.fields,
            [
                (self.organization, "organization", "Organization"),
                (self.street_address, "street_address", "Street address"),
                (
                    self.zip_or_postal_code,
                    "zip_or_postal_code",
                    "ZIP or postal code",
                ),
                (self.city, "city", "City"),
                (
                    self.state_or_province,
                    "state_or_province",
                    "State or province",
                ),
                (
                    self.country_or_region,
                    "country_or_region",
                    "Country or region",
                ),
                (self.floor, "floor", "Floor"),
                (self.county, "county", "County"),
            ],
        );
        custom_fields(s, self.extra_address_details);
        let f = &mut s.fields;
        secret_if(
            f,
            self.social_security_number,
            "social_security_number",
            "Social security number",
        );
        secret_if(
            f,
            self.passport_number,
            "passport_number",
            "Passport number",
        );
        secret_if(f, self.license_number, "license_number", "License number");
        unstored_if(
            f,
            [
                (self.website, "website", "Website"),
                (self.x_handle, "x_handle", "X handle"),
                (
                    self.second_phone_number,
                    "second_phone_number",
                    "Second phone number",
                ),
                (self.linkedin, "linkedin", "LinkedIn"),
                (self.reddit, "reddit", "Reddit"),
                (self.facebook, "facebook", "Facebook"),
                (self.yahoo, "yahoo", "Yahoo"),
                (self.instagram, "instagram", "Instagram"),
            ],
        );
        custom_fields(s, self.extra_contact_details);
        unstored_if(
            &mut s.fields,
            [
                (self.company, "company", "Company"),
                (self.job_title, "job_title", "Job title"),
                (
                    self.personal_website,
                    "personal_website",
                    "Personal website",
                ),
                (
                    self.work_phone_number,
                    "work_phone_number",
                    "Work phone number",
                ),
                (self.work_email, "work_email", "Work email"),
            ],
        );
        custom_fields(s, self.extra_work_details);
        section_fields(s, self.sections);
        s.email = self.email;
        s.subtitle = self.full_name;
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
                b.fill(s);
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

    /// `state` is the only reason the parser drops an item: any value other than `trashed`
    /// (in any case) is kept, so a state `pass-cli` adds later still shows up.
    #[test]
    fn only_trashed_state_drops_an_item() {
        let json = br#"{"items":[
            {"id":"active","state":"Active"},
            {"id":"trashed-capitalised","state":"Trashed"},
            {"id":"trashed-lower","state":"trashed"},
            {"id":"empty-state","state":""},
            {"id":"no-state"},
            {"id":"archived","state":"Archived"},
            {"id":"inactive","state":"Inactive"}
        ]}"#;
        let items = parse_items(json, &ShareId("s".into()), "V").unwrap();
        let ids: Vec<_> = items.iter().map(|i| i.key.item.0.as_str()).collect();
        assert_eq!(
            ids,
            ["active", "empty-state", "no-state", "archived", "inactive"]
        );
    }

    /// Every identity member Proton Pass emits, as captured from a real vault.
    fn identity_item() -> &'static [u8] {
        br#"{"items":[{"id":"identity-full","state":"Active","modify_time":"2026-02-03T04:05:06",
          "content":{"title":"Me","note":"","content":{"Identity":{
            "full_name":"Fixture Person","email":"person@example.invalid",
            "phone_number":"+1 555 0100","first_name":"Fixture","middle_name":"","last_name":"Person",
            "birthdate":"1990-01-01","gender":"unspecified",
            "extra_personal_details":[{"name":"Nickname","content":{"Text":"fix"}}],
            "organization":"","street_address":"1 Fixture Road","zip_or_postal_code":"0001",
            "city":"Town","state_or_province":"","country_or_region":"Countryland","floor":"","county":"",
            "extra_address_details":[{"name":"Door code","content":{"Hidden":"SECRET-FIXTURE-door"}}],
            "social_security_number":"SECRET-FIXTURE-ssn","passport_number":"","license_number":"",
            "website":"https://example.invalid","x_handle":"@fixture","second_phone_number":"",
            "linkedin":"","reddit":"","facebook":"","yahoo":"","instagram":"",
            "extra_contact_details":[],
            "company":"Fixture Inc","job_title":"Tester","personal_website":"",
            "work_phone_number":"","work_email":"work@example.invalid","extra_work_details":[],
            "extra_sections":[{"section_name":"Membership","section_fields":[
              {"name":"Member id","content":{"Hidden":"SECRET-FIXTURE-member"}}]}]}},
          "extra_fields":[]}}]}"#
    }

    #[test]
    fn identity_offers_every_populated_member() {
        let items = parse_items(identity_item(), &ShareId("share-a".into()), "Personal").unwrap();
        let identity = find(&items, "identity-full");
        let names: Vec<_> = identity.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "full_name",
                "email",
                "phone_number",
                "first_name",
                "last_name",
                "birthdate",
                "gender",
                "street_address",
                "zip_or_postal_code",
                "city",
                "country_or_region",
                "Door code",
                "social_security_number",
                "website",
                "x_handle",
                "company",
                "job_title",
                "work_email",
                "Member id",
            ],
            "empty members and user-defined text fields must not be offered, and the groups \
             must stay in UI order"
        );
        assert_eq!(identity.subtitle.as_deref(), Some("Fixture Person"));
        assert_eq!(identity.email.as_deref(), Some("person@example.invalid"));
    }

    #[test]
    fn identity_members_are_fetched_on_demand_and_never_cached() {
        let items = parse_items(identity_item(), &ShareId("share-a".into()), "Personal").unwrap();
        let identity = find(&items, "identity-full");
        // Only the two members the contract calls non-secret carry a cached value.
        let cached: Vec<_> = identity
            .fields
            .iter()
            .filter(|f| f.value.is_some())
            .map(|f| f.name.as_str())
            .collect();
        assert_eq!(cached, ["full_name", "email"]);
        assert!(
            identity
                .field("first_name")
                .is_some_and(|f| !f.secret && f.value.is_none())
        );
        for name in ["social_security_number", "Door code", "Member id"] {
            assert!(identity.field(name).is_some_and(|f| f.secret), "{name}");
        }
        assert_no_secrets(&items);
    }

    /// One login whose `extra_fields` are the given custom-field JSON objects.
    fn custom_field_item(extra: &str) -> ItemSummary {
        let json = format!(
            r#"{{"items":[{{"id":"custom","state":"Active","content":{{"title":"Custom",
              "note":"","content":{{"Login":{{}}}},"extra_fields":[{extra}]}}}}]}}"#
        );
        parse_items(json.as_bytes(), &ShareId("share-a".into()), "Personal")
            .unwrap()
            .remove(0)
    }

    /// `pass-cli` hands the app no value for a user-defined text field, so a row for one
    /// could only ever show a dash and could never be copied. Hidden and one-time-code
    /// siblings carry values the app can fetch, so they stay (FR-113, FR-114).
    #[test]
    fn a_user_defined_text_field_is_dropped_and_its_siblings_are_not() {
        let item = custom_field_item(
            r#"{"name":"Nickname","content":{"Text":"fix"}},
               {"name":"Door code","content":{"Hidden":"SECRET-FIXTURE-door"}},
               {"name":"Authenticator","content":{"Totp":"otpauth://x"}}"#,
        );
        let names: Vec<_> = item.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Door code"]);
        assert!(item.field("Door code").is_some_and(|f| f.secret));
        assert_eq!(item.totp_fields, ["Authenticator"]);
    }

    /// One login carrying a password plus the given custom-field JSON objects.
    fn login_with_password_and_extras(extra: &str) -> ItemSummary {
        let json = format!(
            r#"{{"items":[{{"id":"dupe","state":"Active","content":{{"title":"Dupe",
              "note":"","content":{{"Login":{{"password":"SECRET-FIXTURE-standard"}}}},
              "extra_fields":[{extra}]}}}}]}}"#
        );
        parse_items(json.as_bytes(), &ShareId("share-a".into()), "Personal")
            .unwrap()
            .remove(0)
    }

    /// `FieldRef::name` is the `pass-cli` selector, and `item view --field=` resolves a
    /// repeated name to the LAST field carrying it. Offering both would render the last
    /// field's value under the first field's label, so only the addressable one survives
    /// (cosmic-pass-wqx.41).
    #[test]
    fn a_custom_field_shadowing_a_standard_one_leaves_only_the_addressable_row() {
        let item = login_with_password_and_extras(
            r#"{"name":"password","content":{"Hidden":"SECRET-FIXTURE-custom"}}"#,
        );
        let names: Vec<_> = item.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["password"]);
        // The surviving row is the custom field, whose label is its own name, not the
        // built-in "Password" label of the shadowed standard field.
        assert_eq!(
            item.field("password").map(|f| f.label.as_str()),
            Some("password")
        );
    }

    /// Two custom fields can repeat a name within one item just as easily as a custom field
    /// can shadow a standard one; the same last-wins rule applies.
    #[test]
    fn repeated_custom_field_names_collapse_to_the_last() {
        let item = login_with_password_and_extras(
            r#"{"name":"PIN","content":{"Hidden":"SECRET-FIXTURE-first"}},
               {"name":"Keep","content":{"Hidden":"SECRET-FIXTURE-keep"}},
               {"name":"PIN","content":{"Hidden":"SECRET-FIXTURE-second"}}"#,
        );
        let names: Vec<_> = item.fields.iter().map(|f| f.name.as_str()).collect();
        // The standard `password` has no namesake here, so it is untouched.
        assert_eq!(names, ["password", "Keep", "PIN"]);
    }

    /// A name repeated across two different sections collides exactly like one repeated
    /// inside a single field list, so the rule has to span every source that pushes into
    /// `ItemSummary::fields`, not one `custom_fields` call.
    #[test]
    fn a_name_repeated_across_sections_collapses_too() {
        let json = r#"{"items":[{"id":"sections","state":"Active","content":{"title":"Sections",
          "note":"","content":{"Custom":{"sections":[
            {"section_name":"One","section_fields":[
              {"name":"PIN","content":{"Hidden":"SECRET-FIXTURE-one"}}]},
            {"section_name":"Two","section_fields":[
              {"name":"PIN","content":{"Hidden":"SECRET-FIXTURE-two"}}]}]}},
          "extra_fields":[]}}]}"#;
        let item = parse_items(json.as_bytes(), &ShareId("share-a".into()), "Personal")
            .unwrap()
            .remove(0);
        let names: Vec<_> = item.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["PIN"]);
    }

    /// `item totp` returns a map keyed by field name, so a repeated one-time-code name is
    /// as unaddressable as a repeated field name and collapses the same way.
    #[test]
    fn repeated_one_time_code_names_collapse_to_the_last() {
        let item = login_with_password_and_extras(
            r#"{"name":"Authenticator","content":{"Totp":"otpauth://first"}},
               {"name":"Authenticator","content":{"Totp":"otpauth://second"}}"#,
        );
        assert_eq!(item.totp_fields, ["Authenticator"]);
    }

    /// Distinct names are untouched: collapsing must not reorder or drop anything that
    /// `pass-cli` can still address.
    #[test]
    fn distinct_field_names_keep_their_order() {
        let item = login_with_password_and_extras(
            r#"{"name":"Door code","content":{"Hidden":"SECRET-FIXTURE-door"}},
               {"name":"Member id","content":{"Hidden":"SECRET-FIXTURE-member"}}"#,
        );
        let names: Vec<_> = item.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["password", "Door code", "Member id"]);
    }

    /// A content variant this version has no name for is as unshowable as a text field, so
    /// it goes the same way rather than becoming a row that can only ever render a dash.
    #[test]
    fn an_unrecognised_custom_field_is_dropped_too() {
        let item = custom_field_item(r#"{"name":"Mystery","content":{"Timestamp":"2026-01-01"}}"#);
        assert!(
            item.fields.is_empty(),
            "unexpected fields: {:?}",
            item.fields
        );
    }

    /// Built-in members are cached by name only, like a custom text field, but unlike one the
    /// app can fetch their value on demand — so they stay listed and copyable (FR-115).
    #[test]
    fn built_in_fetch_on_demand_members_are_still_offered() {
        let items = parse_items(identity_item(), &ShareId("share-a".into()), "Personal").unwrap();
        let identity = find(&items, "identity-full");
        for name in [
            "phone_number",
            "first_name",
            "birthdate",
            "city",
            "job_title",
        ] {
            assert!(
                identity
                    .field(name)
                    .is_some_and(|f| !f.secret && f.value.is_none()),
                "{name} must still be offered as an unstored field"
            );
        }
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
            gh.field("Account id").is_none(),
            "a user-defined text field is one `pass-cli` gives no value for, so it is dropped"
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
        assert!(mail.totp_fields.is_empty());
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
        assert!(
            ssh.field("Hostname").is_none(),
            "a section's text field goes the same way"
        );

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
