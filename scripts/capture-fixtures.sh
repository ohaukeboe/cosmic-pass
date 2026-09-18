#!/usr/bin/env bash
# Capture redacted `pass-cli` output as test fixtures.
#
# Requires a signed-in `pass-cli` and `jq`. Raw output is only ever held in shell
# variables; every string value is replaced before anything is written:
#   ids            -> share-N / item-N / vault-N (consistent across files)
#   secret values  -> "SECRET-FIXTURE-<key>"
#   other strings  -> "REDACTED-<key>"
#   timestamps, states, and item kinds are kept.
# At most MAX_PER_KIND active items per kind (plus one trashed item) are kept per vault.
set -euo pipefail

out="${1:-tests/fixtures/pass-cli/captured}"
max_per_kind="${MAX_PER_KIND:-2}"
export PASS_LOG_LEVEL=off PROTON_PASS_NO_UPDATE_CHECK=1
mkdir -p "$out"

# Shared jq redaction program. $ids maps real id -> placeholder.
read -r -d '' REDACT <<'JQ' || true
def secret_keys: ["password","totp_uri","number","verification_number","pin","private_key",
  "note","social_security_number","passport_number","license_number","Hidden","Totp"];
def keep_keys: ["state","create_time","modify_time","item_type","card_type","security"];
def id_keys: ["id","share_id","vault_id","item_uuid"];
def redact($k):
  if type == "object" then with_entries(.key as $kk | .value |= redact($kk))
  elif type == "array" then map(redact($k))
  elif type == "string" then
    if (id_keys | index($k)) then ($ids[.] // "unmapped-id")
    elif (keep_keys | index($k)) then .
    elif . == "" then ""
    elif (secret_keys | index($k)) then "SECRET-FIXTURE-\($k)"
    else "REDACTED-\($k)" end
  else . end;
redact("")
JQ

vaults_raw=$(pass-cli vault list --output json)
mapfile -t shares < <(jq -r '.vaults[].share_id' <<<"$vaults_raw")

ids='{}'
add_id() { ids=$(jq -c --arg k "$1" --arg v "$2" '.[$k] = $v' <<<"$ids"); }

n=0
for s in "${shares[@]}"; do
    n=$((n + 1)); add_id "$s" "share-$n"
    add_id "$(jq -r --arg s "$s" '.vaults[] | select(.share_id == $s) | .vault_id' <<<"$vaults_raw")" "vault-$n"
done

jq --argjson ids "$ids" "$REDACT" <<<"$vaults_raw" > "$out/vault-list.json"
jq --argjson ids '{}' "$REDACT" <(pass-cli info --output json) > "$out/info.json"

item_n=0
for s in "${shares[@]}"; do
    alias=$(jq -r --arg s "$s" '.[$s]' <<<"$ids")
    for mode in plain secrets; do
        flag=(); [ "$mode" = secrets ] && flag=(--show-secrets)
        raw=$(pass-cli item list --share-id="$s" --output json "${flag[@]}")
        # Select a small sample: first N active items per kind, plus one trashed item.
        sample=$(jq -c --argjson max "$max_per_kind" '
            def kind: .item_type // (.content.content | keys[0]);
            [.items[] | select(.state == "Active")] | group_by(kind) | map(.[:$max]) | add // []
            | . + ([input.items[] | select(.state == "Trashed")][:1])' <<<"$raw"$'\n'"$raw")
        for id in $(jq -r '.[].id' <<<"$sample"); do
            if ! jq -e --arg k "$id" 'has($k)' <<<"$ids" >/dev/null; then
                item_n=$((item_n + 1)); add_id "$id" "item-$item_n"
            fi
            add_id "$(jq -r --arg i "$id" '.[] | select(.id == $i) | .content.item_uuid // empty' <<<"$sample")" "uuid-$item_n" 2>/dev/null || true
        done
        suffix=""; [ "$mode" = plain ] && suffix="-plain"
        jq --argjson ids "$ids" "{items: .} | $REDACT" <<<"$sample" > "$out/item-list$suffix-$alias.json"
        unset raw sample
    done
done

# Error texts (ids removed).
{
    echo "# missing item"
    pass-cli item view "pass://${shares[0]}/nonexistent-item/password" 2>&1 >/dev/null || true
    echo "# bad share id"
    pass-cli item list --share-id=bogus --output json 2>&1 >/dev/null || true
} | sed -E 's/[A-Za-z0-9_=-]{30,}/<ID>/g' > "$out/errors.txt"

echo "Wrote fixtures to $out. Review them for personal data before committing."
