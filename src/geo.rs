//! Host mihomo's geo databases (`geox-url`) on behalf of a token.
//!
//! Unlike mrs conversion there is nothing to re-encode: the `.dat`/`.mmdb`
//! files are downloaded from their source and re-served as-is, so the client
//! no longer has to reach the (often unreachable) upstream itself. Each file
//! is hosted under its `geox-url` key name (`geoip`, `geosite`, `mmdb`,
//! `asn`).

use std::collections::HashSet;

use anyhow::{Context, Result};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_yaml::Value;

/// One hosted geo file: its `geox-url` key and mihomo's built-in default
/// source (used when the config does not override the URL).
pub struct GeoFile {
    pub key: &'static str,
    pub default_url: &'static str,
}

/// mihomo's built-in defaults, from `DefaultRawConfig` in mihomo sources.
pub const GEO_FILES: [GeoFile; 4] = [
    GeoFile {
        key: "geoip",
        default_url: "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.dat",
    },
    GeoFile {
        key: "geosite",
        default_url: "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geosite.dat",
    },
    GeoFile {
        key: "mmdb",
        default_url: "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.metadb",
    },
    GeoFile {
        key: "asn",
        default_url: "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/GeoLite2-ASN.mmdb",
    },
];

/// Geo databases are far larger than rule lists; a full `geosite.dat` is a
/// few dozen MB.
pub const MAX_GEO_SOURCE_BYTES: usize = 128 * 1024 * 1024;

/// The geo file names hosted for a token; mrs conversion must not prune them.
pub fn geo_names() -> HashSet<String> {
    GEO_FILES.iter().map(|file| file.key.to_owned()).collect()
}

/// Resolve the download source for every geo file: the config's `geox-url`
/// override when present, the built-in default otherwise.
pub fn resolve_sources(config: &str) -> Result<Vec<(&'static str, String)>> {
    let value: Value = serde_yaml::from_str(config).context("the config is not valid YAML")?;
    let geox = value.get("geox-url");
    Ok(GEO_FILES
        .iter()
        .map(|file| {
            let url = geox
                .and_then(|geox| geox.get(file.key))
                .and_then(Value::as_str)
                .filter(|url| !url.is_empty())
                .unwrap_or(file.default_url);
            (file.key, url.to_owned())
        })
        .collect())
}

/// Public download URL for a token's hosted geo file.
pub fn download_url(base_url: &str, file_key: &str, name: &str) -> String {
    format!(
        "{}/files/{}?key={}",
        base_url.trim_end_matches('/'),
        utf8_percent_encode(name, URL_SAFE),
        utf8_percent_encode(file_key, URL_SAFE),
    )
}

/// Rewrite the config's `geox-url` section so the converted files are served
/// by this server; keys that failed to convert keep their old source. The
/// section is created when the config has none.
pub fn rewrite_config(
    config: &str,
    base_url: &str,
    file_key: &str,
    converted: &HashSet<String>,
) -> Result<String> {
    if converted.is_empty() {
        return Ok(config.to_owned());
    }
    let mut value: Value = serde_yaml::from_str(config).context("the config is not valid YAML")?;
    // An empty config parses to `null`; make room for the new section.
    if value.is_null() {
        value = Value::Mapping(serde_yaml::Mapping::new());
    }
    let Some(config_map) = value.as_mapping_mut() else {
        anyhow::bail!("the config is not a mapping");
    };
    let geox = config_map
        .entry(Value::from("geox-url"))
        .or_insert_with(|| Value::Mapping(serde_yaml::Mapping::new()));
    let Some(geox) = geox.as_mapping_mut() else {
        anyhow::bail!("the config's geox-url is not a mapping");
    };
    for file in &GEO_FILES {
        if converted.contains(file.key) {
            geox.insert(
                Value::from(file.key),
                Value::from(download_url(base_url, file_key, file.key)),
            );
        }
    }
    serde_yaml::to_string(&value).context("failed to serialize the rewritten config")
}

/// Unreserved URL characters (RFC 3986); safe for path segments and queries.
const URL_SAFE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_fall_back_to_defaults() {
        let sources = resolve_sources("mode: rule\n").unwrap();
        assert_eq!(sources.len(), 4);
        assert_eq!(sources[0].0, "geoip");
        assert_eq!(sources[0].1, GEO_FILES[0].default_url);
    }

    #[test]
    fn sources_use_configured_overrides() {
        let config = "\
geox-url:
  mmdb: https://mirror.example.com/country.mmdb
  asn: \"\"
";
        let sources = resolve_sources(config).unwrap();
        let get = |key: &str| {
            sources
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, url)| url.clone())
                .unwrap()
        };
        assert_eq!(get("mmdb"), "https://mirror.example.com/country.mmdb");
        // Empty and missing entries fall back to the default.
        assert_eq!(get("asn"), GEO_FILES[3].default_url);
        assert_eq!(get("geosite"), GEO_FILES[1].default_url);
    }

    #[test]
    fn rewrite_creates_section_and_keeps_failures() {
        let config = "mode: rule\n";
        let converted: HashSet<String> = ["geoip".to_owned()].into();
        let rewritten = rewrite_config(config, "http://10.0.0.1:8080/", "abc", &converted).unwrap();
        assert!(rewritten.contains("geoip: http://10.0.0.1:8080/files/geoip?key=abc"));
        assert!(!rewritten.contains("geosite:"));

        // An existing section keeps its untouched keys.
        let config = "geox-url:\n  mmdb: https://github.com/x/mmdb\n";
        let converted: HashSet<String> = ["mmdb".to_owned(), "asn".to_owned()].into();
        let rewritten = rewrite_config(config, "http://10.0.0.1:8080/", "abc", &converted).unwrap();
        assert!(rewritten.contains("mmdb: http://10.0.0.1:8080/files/mmdb?key=abc"));
        assert!(rewritten.contains("asn: http://10.0.0.1:8080/files/asn?key=abc"));
    }

    #[test]
    fn rewrite_empty_conversion_is_identity() {
        let config = "mode: rule\n";
        let converted: HashSet<String> = HashSet::new();
        assert_eq!(
            rewrite_config(config, "http://10.0.0.1:8080/", "abc", &converted).unwrap(),
            config
        );
    }
}
