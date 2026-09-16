//! Pure-Rust encoder for mihomo's `.mrs` (Meta Rule Set) binary rule format,
//! byte-compatible with `mihomo convert-ruleset` (MSRv1).
//!
//! Wire format, from the mihomo sources: the file is a zstd stream wrapping
//! `[M,R,S,0x01]` magic, a behavior byte (domain=0, ipcidr=1), a big-endian
//! i64 rule count, a reserved big-endian i64 extra length (always 0), and a
//! behavior-specific payload. The domain payload is a LOUDS succinct trie
//! (leaves/labelBitmap bit vectors plus edge labels) over reversed domain
//! keys; the ipcidr payload is a sorted list of merged `from`/`to` ranges,
//! each address stored as 16 bytes (IPv4 embedded as IPv4-mapped).

use std::collections::HashSet;
use std::net::{Ipv4Addr, Ipv6Addr};

use anyhow::{Context, Result, bail};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_yaml::Value;

const MAGIC: [u8; 4] = *b"MRS\x01";

/// Rule-provider behavior; `classical` lists cannot be represented as mrs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Behavior {
    Domain,
    IpCidr,
}

impl Behavior {
    fn byte(self) -> u8 {
        match self {
            Behavior::Domain => 0,
            Behavior::IpCidr => 1,
        }
    }

    /// Parse the `behavior` field of a rule-provider (mihomo accepts any case).
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "domain" => Some(Behavior::Domain),
            "ipcidr" => Some(Behavior::IpCidr),
            _ => None,
        }
    }
}

/// Source rule-list format of a rule-provider; yaml is the mihomo default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceFormat {
    Yaml,
    Text,
}

impl SourceFormat {
    /// Parse the `format` field of a rule-provider (mihomo accepts any case).
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "yaml" => Some(SourceFormat::Yaml),
            "text" => Some(SourceFormat::Text),
            _ => None, // "mrs" or unknown: nothing to convert
        }
    }
}

/// Convert a rule list into `.mrs` bytes. Fails when no valid rule remains,
/// mirroring mihomo's "empty rule" error.
pub fn convert(behavior: Behavior, format: SourceFormat, source: &[u8]) -> Result<Vec<u8>> {
    let entries = parse_entries(format, source)?;
    let (count, payload) = match behavior {
        Behavior::Domain => encode_domain(&entries),
        Behavior::IpCidr => encode_ipcidr(&entries),
    };
    if count == 0 {
        bail!("empty rule");
    }
    encode_container(behavior, count, &payload)
}

fn encode_container(behavior: Behavior, count: i64, payload: &[u8]) -> Result<Vec<u8>> {
    let mut raw = Vec::with_capacity(payload.len() + 24);
    raw.extend_from_slice(&MAGIC);
    raw.push(behavior.byte());
    raw.extend_from_slice(&count.to_be_bytes());
    // Extra section, reserved for future use in mihomo.
    raw.extend_from_slice(&0i64.to_be_bytes());
    raw.extend_from_slice(payload);
    zstd::bulk::compress(&raw, 3).context("failed to compress the mrs payload")
}

// ---------------------------------------------------------------------------
// Rule list parsing
// ---------------------------------------------------------------------------

fn parse_entries(format: SourceFormat, source: &[u8]) -> Result<Vec<String>> {
    let text = std::str::from_utf8(source).context("the rule list is not valid UTF-8")?;
    match format {
        SourceFormat::Text => Ok(text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("//"))
            .map(str::to_owned)
            .collect()),
        SourceFormat::Yaml => parse_yaml_entries(text),
    }
}

/// Extract the `payload` (domain/ipcidr) or `rules` (classical) string list
/// from a yaml rule provider body.
fn parse_yaml_entries(text: &str) -> Result<Vec<String>> {
    let value: Value = serde_yaml::from_str(text).context("the rule list is not valid YAML")?;
    for key in ["payload", "rules"] {
        if let Some(Value::Sequence(items)) = value.get(key) {
            return Ok(items
                .iter()
                .filter_map(Value::as_str)
                .filter(|entry| !entry.is_empty())
                .map(str::to_owned)
                .collect());
        }
    }
    bail!("the yaml rule list has no `payload` or `rules` section")
}

// ---------------------------------------------------------------------------
// Domain behavior
// ---------------------------------------------------------------------------

/// Build the mrs payload for domain rules; returns the rule count (valid
/// input entries, matching mihomo's counter) and the serialized trie.
fn encode_domain(entries: &[String]) -> (i64, Vec<u8>) {
    let mut count = 0i64;
    let mut keys: Vec<String> = Vec::new();
    for entry in entries {
        if entry.contains('/') {
            tracing::warn!(
                entry,
                "skip invalid domain from rule provider: slash is not allowed"
            );
            continue;
        }
        let Some(parts) = valid_and_split_domain(entry) else {
            tracing::warn!(entry, "skip invalid domain from rule provider");
            continue;
        };
        // "+.example.com" covers the domain itself and its subdomains, so it
        // contributes both the bare key and the "+"-marked wildcard key.
        if parts[0] == "+" {
            push_domain_key(&parts[1..], &mut keys);
            push_domain_key(&parts, &mut keys);
        } else {
            push_domain_key(&parts, &mut keys);
        }
        count += 1;
    }
    if count == 0 {
        // No container is written for an empty list (convert bails first).
        return (0, Vec::new());
    }
    let (leaves, label_bitmap, labels) = build_domain_trie(keys);
    (count, write_domain_bin(&leaves, &label_bitmap, &labels))
}

/// Join labels with dots and store the key in reversed order, as mihomo does.
fn push_domain_key(parts: &[String], keys: &mut Vec<String>) {
    let normalized;
    let parts: &[String] = if parts[0].is_empty() {
        // ".example.com" is the dot-wildcard form of "+.example.com".
        let mut owned = parts.to_vec();
        owned[0] = "+".to_owned();
        normalized = owned;
        &normalized
    } else {
        parts
    };
    keys.push(parts.join(".").chars().rev().collect());
}

/// Lowercase, validate and dot-split a domain pattern; `None` when invalid.
fn valid_and_split_domain(domain: &str) -> Option<Vec<String>> {
    if domain.is_empty() || domain.ends_with('.') {
        return None;
    }
    if domain.chars().next().is_some_and(char::is_whitespace)
        || domain.chars().last().is_some_and(char::is_whitespace)
    {
        return None;
    }

    let domain = domain.to_lowercase();
    let parts: Vec<String> = domain.split('.').map(str::to_owned).collect();
    if parts.len() == 1 {
        if parts[0].is_empty() {
            return None;
        }
    } else if parts[1..].iter().any(String::is_empty) {
        // An empty first segment is the ".example.com" dot-wildcard form.
        return None;
    }

    for (i, part) in parts.iter().enumerate() {
        if part.contains('+') && (part != "+" || i != 0 || parts.len() == 1) {
            return None;
        }
        if part.contains('*') && part != "*" {
            return None;
        }
    }
    Some(parts)
}

/// Build the LOUDS trie arrays over sorted reversed keys, ported from mihomo's
/// `buildDomainSet`: a BFS over key columns where each node contributes one
/// `0` bit plus label per distinct child byte and one terminating `1` bit.
fn build_domain_trie(mut keys: Vec<String>) -> (Vec<u64>, Vec<u64>, Vec<u8>) {
    keys.sort();
    keys.dedup();
    let keys: Vec<&[u8]> = keys.iter().map(String::as_bytes).collect();

    let mut leaves: Vec<u64> = Vec::new();
    let mut label_bitmap: Vec<u64> = Vec::new();
    let mut labels: Vec<u8> = Vec::new();
    let mut bit_index = 0usize;
    let mut node_id = 0usize;
    let mut queue: Vec<(usize, usize)> = vec![(0, keys.len())];
    let mut next: Vec<(usize, usize)> = Vec::new();
    let mut col = 0usize;

    while !queue.is_empty() {
        for &(start, end) in &queue {
            let mut start = start;
            // At most one key per node ends exactly here (keys are deduped and
            // sorted, so a shorter prefix always sorts first): a leaf.
            if col == keys[start].len() {
                start += 1;
                set_bit(&mut leaves, node_id, true);
            }
            let mut j = start;
            while j < end {
                let group_start = j;
                let label = keys[j][col];
                while j < end && keys[j][col] == label {
                    j += 1;
                }
                next.push((group_start, j));
                labels.push(label);
                set_bit(&mut label_bitmap, bit_index, false);
                bit_index += 1;
            }
            set_bit(&mut label_bitmap, bit_index, true);
            bit_index += 1;
            node_id += 1;
        }
        std::mem::swap(&mut queue, &mut next);
        next.clear();
        col += 1;
    }

    (leaves, label_bitmap, labels)
}

fn set_bit(bitmap: &mut Vec<u64>, index: usize, value: bool) {
    if index / 64 >= bitmap.len() {
        bitmap.resize(index / 64 + 1, 0);
    }
    if value {
        bitmap[index / 64] |= 1 << (index % 64);
    }
}

fn write_domain_bin(leaves: &[u64], label_bitmap: &[u64], labels: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(25 + 8 * (leaves.len() + label_bitmap.len()) + labels.len());
    out.push(1); // version
    out.extend_from_slice(&(leaves.len() as i64).to_be_bytes());
    for word in leaves {
        out.extend_from_slice(&word.to_be_bytes());
    }
    out.extend_from_slice(&(label_bitmap.len() as i64).to_be_bytes());
    for word in label_bitmap {
        out.extend_from_slice(&word.to_be_bytes());
    }
    out.extend_from_slice(&(labels.len() as i64).to_be_bytes());
    out.extend_from_slice(labels);
    out
}

// ---------------------------------------------------------------------------
// ipcidr behavior
// ---------------------------------------------------------------------------

/// Build the mrs payload for IP-CIDR rules: parse, then merge into sorted
/// disjoint inclusive ranges like mihomo's `IpCidrSet.Merge`.
fn encode_ipcidr(entries: &[String]) -> (i64, Vec<u8>) {
    let mut count = 0i64;
    let mut ranges: Vec<(u128, u128)> = Vec::new();
    for entry in entries {
        match parse_cidr(entry) {
            Some(range) => {
                ranges.push(range);
                count += 1;
            }
            None => tracing::warn!(entry, "invalid Ipcidr"),
        }
    }
    ranges.sort_unstable();
    if count == 0 {
        // No container is written for an empty list (convert bails first).
        return (0, Vec::new());
    }

    let mut merged: Vec<(u128, u128)> = Vec::with_capacity(ranges.len());
    for (from, to) in ranges {
        match merged.last_mut() {
            Some(last) if from <= last.1.saturating_add(1) => last.1 = last.1.max(to),
            _ => merged.push((from, to)),
        }
    }

    let mut out = Vec::with_capacity(9 + 32 * merged.len());
    out.push(1); // version
    out.extend_from_slice(&(merged.len() as i64).to_be_bytes());
    for (from, to) in merged {
        out.extend_from_slice(&from.to_be_bytes());
        out.extend_from_slice(&to.to_be_bytes());
    }
    (count, out)
}

/// Parse a `addr/bits` CIDR into an inclusive `from..=to` range as big-endian
/// u128 values of the 16-byte (IPv4-mapped for IPv4) wire representation.
fn parse_cidr(entry: &str) -> Option<(u128, u128)> {
    let (addr, bits) = entry.split_once('/')?;
    if bits.is_empty() || !bits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let bits: u32 = bits.parse().ok()?;
    if let Ok(v4) = addr.parse::<Ipv4Addr>() {
        if bits > 32 {
            return None;
        }
        let mask = if bits == 0 {
            0
        } else {
            u32::MAX << (32 - bits)
        };
        let from = u32::from(v4) & mask;
        let to = from | !mask;
        Some((
            u128::from(Ipv4Addr::from(from).to_ipv6_mapped()),
            u128::from(Ipv4Addr::from(to).to_ipv6_mapped()),
        ))
    } else if let Ok(v6) = addr.parse::<Ipv6Addr>() {
        if bits > 128 {
            return None;
        }
        let mask = if bits == 0 {
            0
        } else {
            u128::MAX << (128 - bits)
        };
        let from = u128::from(v6) & mask;
        Some((from, from | !mask))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Config inspection and rewriting
// ---------------------------------------------------------------------------

/// A rule-provider entry extracted from a config's `rule-providers` section.
#[derive(Clone, Debug)]
pub struct RuleProvider {
    pub name: String,
    pub url: String,
    pub behavior: String,
    pub format: String,
    pub provider_type: String,
}

/// Per-source download limit for mrs conversion.
const MAX_SOURCE_BYTES: usize = 10 * 1024 * 1024;

/// Result of converting one rule-provider.
pub enum ProviderOutcome {
    Converted {
        size: usize,
        url: String,
        content: Vec<u8>,
    },
    /// Nothing to do; `reason` is a stable code the UI translates.
    Skipped {
        reason: &'static str,
    },
    Failed {
        error: anyhow::Error,
    },
}

/// Download a rule-provider source and convert it to a hosted mrs file.
pub async fn convert_provider(
    client: &reqwest::Client,
    file_key: &str,
    base_url: &str,
    provider: &RuleProvider,
) -> ProviderOutcome {
    if provider.provider_type != "http" {
        return ProviderOutcome::Skipped {
            reason: "unsupported-type",
        };
    }
    let behavior = match Behavior::parse(&provider.behavior) {
        Some(behavior) => behavior,
        None => {
            let reason = if provider.behavior.eq_ignore_ascii_case("classical") {
                "classical"
            } else {
                "unsupported-behavior"
            };
            return ProviderOutcome::Skipped { reason };
        }
    };
    let format = match SourceFormat::parse(&provider.format) {
        Some(format) => format,
        None => {
            let reason = if provider.format.eq_ignore_ascii_case("mrs") {
                "already-mrs"
            } else {
                "unsupported-format"
            };
            return ProviderOutcome::Skipped { reason };
        }
    };
    if provider.url.is_empty() {
        return ProviderOutcome::Failed {
            error: anyhow::anyhow!("the provider has no url"),
        };
    }

    let source = match download(client, &provider.url, MAX_SOURCE_BYTES).await {
        Ok(source) => source,
        Err(error) => return ProviderOutcome::Failed { error },
    };
    match convert(behavior, format, &source) {
        Ok(content) => ProviderOutcome::Converted {
            size: content.len(),
            url: download_url(base_url, file_key, &provider.name),
            content,
        },
        Err(error) => ProviderOutcome::Failed { error },
    }
}

/// Download a source file, refusing bodies larger than `max_bytes`.
pub async fn download(client: &reqwest::Client, url: &str, max_bytes: usize) -> Result<Vec<u8>> {
    let response = client
        .get(url)
        .send()
        .await
        .context("failed to download the source")?;
    if !response.status().is_success() {
        bail!("the source server returned HTTP {}", response.status());
    }
    if response
        .content_length()
        .is_some_and(|length| length as usize > max_bytes)
    {
        bail!("the source exceeds the {max_bytes} byte size limit");
    }
    let body = response
        .bytes()
        .await
        .context("failed to read the source body")?;
    if body.len() > max_bytes {
        bail!("the source exceeds the {max_bytes} byte size limit");
    }
    Ok(body.to_vec())
}

pub fn parse_rule_providers(config: &str) -> Result<Vec<RuleProvider>> {
    let value: Value = serde_yaml::from_str(config).context("the config is not valid YAML")?;
    let Some(providers) = value.get("rule-providers").and_then(Value::as_mapping) else {
        bail!("the config has no rule-providers section");
    };

    let mut result = Vec::new();
    for (name, body) in providers {
        let Some(name) = name.as_str() else { continue };
        let field = |key: &str| {
            body.get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_default()
        };
        result.push(RuleProvider {
            name: name.to_owned(),
            url: field("url"),
            behavior: field("behavior"),
            format: {
                let format = field("format");
                if format.is_empty() {
                    "yaml".to_owned()
                } else {
                    format.to_ascii_lowercase()
                }
            },
            provider_type: {
                let provider_type = field("type");
                if provider_type.is_empty() {
                    "http".to_owned()
                } else {
                    provider_type.to_ascii_lowercase()
                }
            },
        });
    }
    Ok(result)
}

/// Public download URL for a token's converted rule file.
pub fn download_url(base_url: &str, file_key: &str, name: &str) -> String {
    format!(
        "{}/files/{}.mrs?key={}",
        base_url.trim_end_matches('/'),
        utf8_percent_encode(name, URL_SAFE),
        utf8_percent_encode(file_key, URL_SAFE),
    )
}

/// Rewrite the providers in `converted` to download their mrs variant from
/// this server, leaving every other part of the config untouched.
pub fn rewrite_config(
    config: &str,
    base_url: &str,
    file_key: &str,
    converted: &HashSet<String>,
) -> Result<String> {
    let mut value: Value = serde_yaml::from_str(config).context("the config is not valid YAML")?;
    let Some(providers) = value
        .get_mut("rule-providers")
        .and_then(Value::as_mapping_mut)
    else {
        bail!("the config has no rule-providers section");
    };

    for (name, body) in providers.iter_mut() {
        let Some(name) = name.as_str() else { continue };
        if !converted.contains(name) {
            continue;
        }
        let Some(body) = body.as_mapping_mut() else {
            continue;
        };
        let url = download_url(base_url, file_key, name);
        body.insert(Value::from("url"), Value::from(url));
        body.insert(Value::from("format"), Value::from("mrs"));
    }
    serde_yaml::to_string(&value).context("failed to serialize the rewritten config")
}

/// Unreserved URL characters (RFC 3986); safe for path segments and queries.
pub(crate) const URL_SAFE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
    }

    /// Our decompressed encoding must equal mihomo's decompressed golden file;
    /// the zstd layers themselves may legitimately differ between encoders.
    #[test]
    fn domain_text_matches_mihomo() {
        let source = std::fs::read(fixture("domain_text.list")).unwrap();
        let golden = std::fs::read(fixture("domain_text.mrs")).unwrap();
        let ours = convert(Behavior::Domain, SourceFormat::Text, &source).unwrap();
        let ours = zstd::stream::decode_all(&ours[..]).unwrap();
        let golden = zstd::stream::decode_all(&golden[..]).unwrap();
        assert_eq!(ours, golden);
    }

    #[test]
    fn domain_yaml_matches_mihomo() {
        let source = std::fs::read(fixture("domain_yaml.yaml")).unwrap();
        let golden = std::fs::read(fixture("domain_yaml.mrs")).unwrap();
        let ours = convert(Behavior::Domain, SourceFormat::Yaml, &source).unwrap();
        let ours = zstd::stream::decode_all(&ours[..]).unwrap();
        let golden = zstd::stream::decode_all(&golden[..]).unwrap();
        assert_eq!(ours, golden);
    }

    #[test]
    fn ipcidr_matches_mihomo() {
        let source = std::fs::read(fixture("ipcidr_text.list")).unwrap();
        let golden = std::fs::read(fixture("ipcidr_text.mrs")).unwrap();
        let ours = convert(Behavior::IpCidr, SourceFormat::Text, &source).unwrap();
        let ours = zstd::stream::decode_all(&ours[..]).unwrap();
        let golden = zstd::stream::decode_all(&golden[..]).unwrap();
        assert_eq!(ours, golden);
    }

    #[test]
    fn empty_rule_list_is_rejected() {
        let err = convert(Behavior::Domain, SourceFormat::Text, b"# only a comment\n").unwrap_err();
        assert!(err.to_string().contains("empty rule"));
    }

    #[test]
    fn domain_key_building() {
        let mut keys = Vec::new();
        push_domain_key(&["+".into(), "example".into(), "com".into()], &mut keys);
        push_domain_key(&["example".into(), "com".into()], &mut keys);
        // The dot-wildcard form only normalizes to the "+" marker key.
        push_domain_key(&["".into(), "example".into(), "com".into()], &mut keys);
        assert_eq!(keys, ["moc.elpmaxe.+", "moc.elpmaxe", "moc.elpmaxe.+"]);
    }

    #[test]
    fn domain_validation() {
        assert!(valid_and_split_domain("example.com").is_some());
        assert!(valid_and_split_domain(".example.com").is_some());
        assert!(valid_and_split_domain("+.example.com").is_some());
        assert!(valid_and_split_domain("*.example.com").is_some());
        assert!(valid_and_split_domain("").is_none());
        assert!(valid_and_split_domain("example.com.").is_none());
        assert!(valid_and_split_domain("a..b").is_none());
        assert!(valid_and_split_domain("+").is_none());
        assert!(valid_and_split_domain("a.+b.com").is_none());
        assert!(valid_and_split_domain("sub.+b.example.com").is_none());
        assert!(valid_and_split_domain("a*b.com").is_none());
        assert!(valid_and_split_domain(" example.com").is_none());
    }

    #[test]
    fn cidr_parsing_and_merge() {
        assert_eq!(parse_cidr("1.2.3.4/32"), parse_cidr("1.2.3.4/32"));
        // Host bits are masked away.
        let (from, _) = parse_cidr("1.2.3.4/24").unwrap();
        assert_eq!(from, u128::from(Ipv4Addr::new(1, 2, 3, 0).to_ipv6_mapped()));
        assert!(parse_cidr("1.2.3.4").is_none());
        assert!(parse_cidr("1.2.3.0/33").is_none());
        assert!(parse_cidr("::1/0").is_some());

        let (_, payload) = encode_ipcidr(&[
            "10.1.0.0/16".into(), // inside 10.0.0.0/8
            "10.0.0.0/8".into(),
            "192.168.1.1/32".into(),
            "2001:db8::/33".into(),
        ]);
        // 10/8 + 192.168.1.1 stay separate; the two /33 halves merge into /32.
        let merged_ranges = u64::from_be_bytes(payload[1..9].try_into().unwrap());
        assert_eq!(merged_ranges, 3);
        let (first_from, first_to) = (
            u128::from_be_bytes(payload[9..25].try_into().unwrap()),
            u128::from_be_bytes(payload[25..41].try_into().unwrap()),
        );
        assert_eq!(
            first_from,
            u128::from(Ipv4Addr::new(10, 0, 0, 0).to_ipv6_mapped())
        );
        assert_eq!(
            first_to,
            u128::from(Ipv4Addr::new(10, 255, 255, 255).to_ipv6_mapped())
        );
    }

    #[test]
    fn parse_providers_defaults() {
        let config = "\
rule-providers:
  google:
    url: https://example.com/google.yaml
    behavior: domain
  ads:
    type: http
    behavior: CLASSICAL
    format: text
    url: https://example.com/ads.list
";
        let providers = parse_rule_providers(config).unwrap();
        assert_eq!(providers.len(), 2);
        assert_eq!(providers[0].name, "google");
        assert_eq!(providers[0].format, "yaml");
        assert_eq!(providers[0].provider_type, "http");
        assert_eq!(providers[1].behavior, "CLASSICAL");
        assert_eq!(providers[1].format, "text");
    }

    #[test]
    fn rewrite_only_converted_providers() {
        let config = "\
rule-providers:
  google:
    url: https://example.com/google.yaml
    behavior: domain
    path: ./google.yaml
  ads:
    url: https://example.com/ads.list
    behavior: classical
";
        let converted: HashSet<String> = ["google".to_owned()].into();
        let rewritten = rewrite_config(config, "http://10.0.0.1:8080/", "abc", &converted).unwrap();
        assert!(rewritten.contains("http://10.0.0.1:8080/files/google.mrs?key=abc"));
        assert!(rewritten.contains("format: mrs"));
        assert!(rewritten.contains("path: ./google.yaml"));
        assert!(rewritten.contains("https://example.com/ads.list"));
    }
}
