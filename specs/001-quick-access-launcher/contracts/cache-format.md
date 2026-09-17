# Contract: Encrypted metadata cache

## Location and permissions

- Path: `$XDG_CACHE_HOME/cosmic-pass/cache.bin` (fallback `~/.cache/cosmic-pass/cache.bin`).
- Directory mode `0700`, file mode `0600`.
- Written atomically: temp file in the same directory → `fsync` → rename → `fsync` directory.

## Key

- 32 random bytes from the OS CSPRNG.
- Stored in the Secret Service (`oo7`) with attributes:
  - `application` = `io.github.ohaukeboe.CosmicPass`
  - `purpose` = `cache-key`
  - label: `COSMIC Pass cache key`
- Created on first successful write. Never written to disk by the app.

## File layout (little-endian)

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Magic `CPC1` |
| 4 | 2 | Envelope version `1` |
| 6 | 24 | XChaCha20-Poly1305 nonce (random per write) |
| 30 | rest | Ciphertext + 16-byte tag |

AAD = bytes 0..30. Plaintext = `postcard`-encoded `CacheFile` (see
[data-model.md](../data-model.md#cachefile-encrypted-on-disk)).

## Rules

| Situation | Behavior |
|-----------|----------|
| Keyring unavailable, locked, or `COSMIC_PASS_NO_KEYRING=1` | Do not read or write the file. Memory only (FR-024a). |
| Magic/version mismatch, decrypt failure, decode failure | Delete file; continue with empty data and refresh. |
| `format_version` newer than supported | Delete file; refresh. |
| `account` ≠ current signed-in account | Delete file; refresh (FR-024b). |
| `pass-cli` reports signed out | Delete file and keyring item (FR-024b). |
| Successful refresh or usage update | Rewrite file (debounced to at most once per 2 s). |

## Content guarantees

The plaintext MUST contain only fields listed for `ItemSummary`, `Vault`, and `UsageRecord`.
A test MUST encode a cache built from fixtures that include every secret field and assert
that no secret fixture value appears in the plaintext bytes (SC-006).
