//! MCP inventory. Read-only. No enable/disable, no writeback.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::content_hash;
use super::discovery;
use super::paths;
use super::scanner;
use super::v1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpTransport {
    Stdio,
    StreamableHttp,
    LegacySse,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InventoryScope {
    User,
    Project,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpEntry {
    pub id: String,
    pub name: String,
    pub transport: McpTransport,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub url: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub source_enabled: Option<bool>,
    pub scope: InventoryScope,
    pub source_harness: String,
    pub source_path: String,
    pub raw_config: Value,
    pub content_hash: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpHarnessStatus {
    pub harness: String,
    pub display_name: String,
    /// Transport observed in *this* harness's own config. Never sampled from
    /// another harness: same name + different config = different rows here.
    pub transport: McpTransport,
    /// `Some(true/false)` when the source config states it, `None` when the
    /// harness format carries no enable flag. `None` means "Configured",
    /// never "Enabled".
    pub source_enabled: Option<bool>,
    pub source_path: String,
    pub configured: bool,
}

/// One MCP server name across harnesses. V1 deliberately carries no
/// command/args/url/env/headers/raw_config to the WebView: this is an
/// inventory, not a debugger, and those fields routinely contain secrets
/// (`--api-key`, `Authorization: Bearer …`, `?token=` URLs).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpInventoryRow {
    pub name: String,
    pub sources: Vec<McpHarnessStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillLifecycle {
    Managed,
    Discovered,
    ReadOnly,
    System,
    Conflict,
    UpdateAvailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInventoryRow {
    pub name: String,
    pub status: SkillLifecycle,
    pub path: String,
    pub source_harness: String,
    pub source_display_name: String,
    pub description: Option<String>,
    pub fingerprint: Option<String>,
    pub system: bool,
    pub read_only: bool,
    pub native_consumers: Vec<String>,
}

fn hash_value(value: &Value) -> String {
    let serialized = serde_json::to_vec(value).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(&serialized);
    format!("{:x}", hasher.finalize())
}

fn classify_transport(command: Option<&str>, url: Option<&str>, type_hint: Option<&str>) -> McpTransport {
    let hint = type_hint.unwrap_or("").to_ascii_lowercase();
    if hint.contains("sse") {
        return McpTransport::LegacySse;
    }
    if hint.contains("http") || hint.contains("stream") || hint == "remote" {
        return McpTransport::StreamableHttp;
    }
    if url.is_some() {
        return McpTransport::StreamableHttp;
    }
    if command.is_some() {
        return McpTransport::Stdio;
    }
    McpTransport::Unknown
}

fn json_string_map(value: Option<&Value>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Some(Value::Object(map)) = value {
        for (k, v) in map {
            if let Some(s) = v.as_str() {
                out.insert(k.clone(), s.to_string());
            }
        }
    }
    out
}

fn json_string_vec(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn parse_mcp_object(
    name: &str,
    raw: &Value,
    harness: &str,
    source_path: &Path,
    scope: InventoryScope,
) -> McpEntry {
    let obj = raw.as_object();
    let type_hint = obj.and_then(|m| m.get("type")).and_then(|v| v.as_str());
    let mut command = obj
        .and_then(|m| m.get("command"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let mut args = json_string_vec(obj.and_then(|m| m.get("args")));
    if let Some(Value::Array(cmd)) = obj.and_then(|m| m.get("command")) {
        let mut parts: Vec<String> = cmd
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        if command.is_none() && !parts.is_empty() {
            command = Some(parts.remove(0));
            if args.is_empty() {
                args = parts;
            }
        }
    }
    let url = obj
        .and_then(|m| m.get("url"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let enabled = obj
        .and_then(|m| m.get("enabled"))
        .and_then(|v| v.as_bool())
        .or_else(|| obj.and_then(|m| m.get("disabled")).and_then(|v| v.as_bool()).map(|d| !d));
    let transport = classify_transport(command.as_deref(), url.as_deref(), type_hint);
    McpEntry {
        id: format!("{}:{}:{}", harness, name, paths::identity_key(source_path)),
        name: name.to_string(),
        transport,
        command,
        args,
        env: json_string_map(obj.and_then(|m| m.get("env"))),
        url,
        headers: json_string_map(obj.and_then(|m| m.get("headers"))),
        source_enabled: enabled,
        scope,
        source_harness: harness.to_string(),
        source_path: source_path.display().to_string(),
        raw_config: raw.clone(),
        content_hash: hash_value(raw),
        read_only: true,
    }
}

fn parse_json_mcp_file(path: &Path, harness: &str) -> Vec<McpEntry> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    let servers = value
        .get("mcpServers")
        .or_else(|| value.get("mcp"))
        .cloned()
        .unwrap_or(Value::Null);
    if let Value::Object(map) = servers {
        for (name, raw) in map {
            entries.push(parse_mcp_object(
                &name,
                &raw,
                harness,
                path,
                InventoryScope::User,
            ));
        }
    }
    entries
}

fn parse_toml_mcp_file(path: &Path, harness: &str) -> Vec<McpEntry> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = text.parse::<toml::Value>() else {
        return Vec::new();
    };
    let Some(servers) = value.get("mcp_servers").or_else(|| value.get("mcp")) else {
        return Vec::new();
    };
    let Ok(json) = serde_json::to_value(servers) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    if let Value::Object(map) = json {
        for (name, raw) in map {
            entries.push(parse_mcp_object(
                &name,
                &raw,
                harness,
                path,
                InventoryScope::User,
            ));
        }
    }
    entries
}

fn parse_yaml_mcp_file(path: &Path, harness: &str) -> Vec<McpEntry> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = serde_yaml::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let servers = value
        .get("mcpServers")
        .or_else(|| value.get("mcp"))
        .cloned()
        .unwrap_or(Value::Null);
    let mut entries = Vec::new();
    if let Value::Object(map) = servers {
        for (name, raw) in map {
            entries.push(parse_mcp_object(
                &name,
                &raw,
                harness,
                path,
                InventoryScope::User,
            ));
        }
    }
    entries
}

fn parse_mcp_path(path: &Path, harness: &str) -> Vec<McpEntry> {
    if !path.exists() {
        return Vec::new();
    }
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "toml" => parse_toml_mcp_file(path, harness),
        "yaml" | "yml" => parse_yaml_mcp_file(path, harness),
        _ => parse_json_mcp_file(path, harness),
    }
}

pub fn discover_mcp() -> Vec<McpEntry> {
    let mut entries = Vec::new();
    for adapter in discovery::v1_descriptors() {
        for path in adapter.expanded_mcp_user_paths() {
            entries.extend(parse_mcp_path(&path, &adapter.id));
        }
    }
    entries
}

pub fn inventory_rows(entries: &[McpEntry]) -> Vec<McpInventoryRow> {
    let mut names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
    names.sort();
    names.dedup();
    let adapters = discovery::v1_descriptors();
    names
        .into_iter()
        .map(|name| {
            let matching: Vec<&McpEntry> = entries.iter().filter(|e| e.name == name).collect();
            let sources = adapters
                .iter()
                .map(|adapter| {
                    let hit = matching.iter().find(|e| e.source_harness == adapter.id);
                    McpHarnessStatus {
                        harness: adapter.id.clone(),
                        display_name: adapter.name.clone(),
                        transport: hit
                            .map(|e| e.transport.clone())
                            .unwrap_or(McpTransport::Unknown),
                        source_enabled: hit.map(|e| e.source_enabled).unwrap_or(None),
                        source_path: hit.map(|e| e.source_path.clone()).unwrap_or_default(),
                        configured: hit.is_some(),
                    }
                })
                .collect();
            McpInventoryRow { name, sources }
        })
        .collect()
}

fn skill_dirs_in(root: &Path, recursive: bool) -> Vec<PathBuf> {
    if !root.exists() {
        return Vec::new();
    }
    if recursive {
        return scanner::collect_skill_dirs(root);
    }
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && super::skill_metadata::is_valid_skill_dir(&path) {
                out.push(path);
            }
        }
    }
    out
}

fn is_system_skill(path: &Path) -> bool {
    let key = paths::identity_key(path);
    key.contains("\\.codex\\skills\\.system\\") || key.contains("/.codex/skills/.system/")
}

pub fn discover_skills() -> Vec<SkillInventoryRow> {
    let mut rows = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let native: Vec<String> = v1::native_consumers()
        .iter()
        .map(|s| s.to_string())
        .collect();

    let managed_root = paths::user_agents_skills_dir();
    for path in skill_dirs_in(&managed_root, false) {
        let key = paths::identity_key(&path);
        if !seen.insert(key.clone()) {
            continue;
        }
        let name = super::skill_metadata::infer_skill_name(&path);
        let meta = super::skill_metadata::parse_skill_md(&path);
        rows.push(SkillInventoryRow {
            name,
            status: SkillLifecycle::Managed,
            path: path.display().to_string(),
            source_harness: "agents".to_string(),
            source_display_name: "User .agents".to_string(),
            description: meta.description,
            fingerprint: content_hash::hash_directory(&path).ok(),
            system: false,
            read_only: false,
            native_consumers: native.clone(),
        });
    }

    for adapter in discovery::v1_descriptors() {
        let system_roots = adapter.expanded_skill_system_paths();
        for root in adapter.expanded_skill_user_paths() {
            if paths::same_path(&root, &managed_root) {
                continue;
            }
            let recursive = adapter.id == "antigravity";
            for path in skill_dirs_in(&root, recursive) {
                let key = paths::identity_key(&path);
                if !seen.insert(key) {
                    continue;
                }
                if paths::same_path(path.parent().unwrap_or(&path), &managed_root) {
                    continue;
                }
                let system = is_system_skill(&path)
                    || system_roots.iter().any(|sys| {
                        paths::identity_key(&path).starts_with(&paths::identity_key(sys))
                    });
                let name = super::skill_metadata::infer_skill_name(&path);
                let meta = super::skill_metadata::parse_skill_md(&path);
                rows.push(SkillInventoryRow {
                    name,
                    status: if system {
                        SkillLifecycle::System
                    } else {
                        SkillLifecycle::Discovered
                    },
                    path: path.display().to_string(),
                    source_harness: adapter.id.clone(),
                    source_display_name: adapter.name.clone(),
                    description: meta.description,
                    fingerprint: content_hash::hash_directory(&path).ok(),
                    system,
                    read_only: system,
                    native_consumers: if adapter.native_consumer {
                        vec![adapter.id.clone()]
                    } else {
                        Vec::new()
                    },
                });
            }
        }
    }

    rows.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.path.cmp(&b.path))
    });
    rows
}

pub fn discover_project_skills(project_root: &Path) -> Vec<SkillInventoryRow> {
    let managed_root = paths::project_agents_skills_dir(project_root);
    discover_project_skills_at(&managed_root, project_root)
}

/// Project inventory against an already-resolved canonical root.
/// `managed_root` must be `<project>/.agents/skills` (resolved backend-side
/// from the project id); discovered rows still come from the harness-relative
/// project paths joined onto `project_root`.
pub fn discover_project_skills_at(
    managed_root: &Path,
    project_root: &Path,
) -> Vec<SkillInventoryRow> {
    let mut rows = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let native: Vec<String> = v1::native_consumers()
        .iter()
        .map(|s| s.to_string())
        .collect();

    for path in skill_dirs_in(&managed_root, false) {
        let key = paths::identity_key(&path);
        if !seen.insert(key) {
            continue;
        }
        let name = super::skill_metadata::infer_skill_name(&path);
        let meta = super::skill_metadata::parse_skill_md(&path);
        rows.push(SkillInventoryRow {
            name,
            status: SkillLifecycle::Managed,
            path: path.display().to_string(),
            source_harness: "agents".to_string(),
            source_display_name: "Project .agents".to_string(),
            description: meta.description,
            fingerprint: content_hash::hash_directory(&path).ok(),
            system: false,
            read_only: false,
            native_consumers: native.clone(),
        });
    }

    for adapter in discovery::v1_descriptors() {
        for rel in adapter.skill_project_paths {
            if v1::is_canonical_project_skills_dir(&rel) {
                continue;
            }
            let root = project_root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
            for path in skill_dirs_in(&root, false) {
                let key = paths::identity_key(&path);
                if !seen.insert(key) {
                    continue;
                }
                let name = super::skill_metadata::infer_skill_name(&path);
                let meta = super::skill_metadata::parse_skill_md(&path);
                rows.push(SkillInventoryRow {
                    name,
                    status: SkillLifecycle::Discovered,
                    path: path.display().to_string(),
                    source_harness: adapter.id.clone(),
                    source_display_name: adapter.name.clone(),
                    description: meta.description,
                    fingerprint: content_hash::hash_directory(&path).ok(),
                    system: false,
                    read_only: false,
                    native_consumers: Vec::new(),
                });
            }
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(harness: &str, name: &str, transport: McpTransport, enabled: Option<bool>) -> McpEntry {
        McpEntry {
            id: format!("{harness}:{name}"),
            name: name.to_string(),
            transport,
            command: None,
            args: Vec::new(),
            env: BTreeMap::new(),
            url: None,
            headers: BTreeMap::new(),
            source_enabled: enabled,
            scope: InventoryScope::User,
            source_harness: harness.to_string(),
            source_path: format!("/fake/{harness}"),
            raw_config: Value::Null,
            content_hash: "hash".to_string(),
            read_only: true,
        }
    }

    #[test]
    fn same_name_keeps_per_source_transport() {
        // exa over HTTP in one harness and stdio in another must not collapse
        // into a single sampled transport.
        let entries = vec![
            entry("opencode", "exa", McpTransport::StreamableHttp, Some(true)),
            entry("maka", "exa", McpTransport::Stdio, None),
        ];
        let rows = inventory_rows(&entries);
        assert_eq!(rows.len(), 1);
        let sources: std::collections::HashMap<_, _> = rows[0]
            .sources
            .iter()
            .filter(|s| s.configured)
            .map(|s| (s.harness.as_str(), s))
            .collect();
        assert_eq!(
            sources["opencode"].transport,
            McpTransport::StreamableHttp
        );
        assert_eq!(sources["maka"].transport, McpTransport::Stdio);
        assert_eq!(sources["opencode"].source_enabled, Some(true));
        // Unknown enablement stays unknown; the UI renders it "Configured".
        assert_eq!(sources["maka"].source_enabled, None);
    }

    #[test]
    fn unconfigured_harnesses_carry_no_transport() {
        let entries = vec![entry("maka", "solo", McpTransport::Stdio, Some(false))];
        let rows = inventory_rows(&entries);
        let unconfigured = rows[0]
            .sources
            .iter()
            .find(|s| s.harness == "codex")
            .unwrap();
        assert!(!unconfigured.configured);
        assert_eq!(unconfigured.transport, McpTransport::Unknown);
    }

    #[test]
    fn wire_rows_carry_no_secrets() {
        // The serialized row must not contain command/args/url/env/headers.
        let entries = vec![entry("maka", "solo", McpTransport::Stdio, None)];
        let rows = inventory_rows(&entries);
        let json = serde_json::to_value(&rows).unwrap().to_string();
        for forbidden in ["command", "args", "raw_config", "headers", "\"env\""] {
            assert!(!json.contains(forbidden), "leaked {forbidden}");
        }
    }
}
