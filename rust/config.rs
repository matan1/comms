//! Reading `.comms/comms.toml` and the rite model it declares.
//!
//! The harness is config-driven: a profile's `comms.toml` declares its own
//! rites, and a rite is an ordered list of steps. Each step is a short
//! `"verb target"` string — the verb is a tool operation the binary knows how
//! to perform and how to detect, the target names what it acts on (an artifact
//! type declared in the same file, or a built-in noun like `session` or
//! `store`). The TOML sequences and binds; the tool supplies the behavior. This
//! keeps one rite flow from being hardcoded while keeping the syntax small.
//!
//! Rather than depend on a TOML crate (and lose the single-static-binary, pure-
//! Rust property), this reads exactly the subset `comms.toml` uses: dotted-key
//! tables, string and bool scalars, and single-line arrays of strings.

use std::collections::BTreeMap;
use std::path::Path;

/// A scalar or array value in a comms.toml table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TomlValue {
    Str(String),
    Bool(bool),
    Array(Vec<String>),
}

impl TomlValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            TomlValue::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            TomlValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&[String]> {
        match self {
            TomlValue::Array(a) => Some(a),
            _ => None,
        }
    }
}

/// A parsed comms.toml: table path (e.g. "" for root, "archive",
/// "rites.close", "artifact_types.letters") -> its key/value pairs.
#[derive(Debug, Default, Clone)]
pub struct Toml {
    pub tables: BTreeMap<String, BTreeMap<String, TomlValue>>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct TomlError {
    pub line: usize,
    pub msg: String,
}

impl std::fmt::Display for TomlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "comms.toml line {}: {}", self.line, self.msg)
    }
}

impl std::error::Error for TomlError {}

impl Toml {
    /// Read a scalar/array by table path and key.
    pub fn get(&self, table: &str, key: &str) -> Option<&TomlValue> {
        self.tables.get(table).and_then(|t| t.get(key))
    }

    /// Table paths that are direct children of `prefix` (e.g. the names under
    /// `rites.` or `artifact_types.`), returned as the leaf segment.
    pub fn children_of(&self, prefix: &str) -> Vec<String> {
        let dotted = format!("{prefix}.");
        let mut out: Vec<String> = self
            .tables
            .keys()
            .filter_map(|t| t.strip_prefix(&dotted))
            .filter(|rest| !rest.contains('.'))
            .map(str::to_owned)
            .collect();
        out.sort();
        out
    }
}

/// Parse comms.toml text. Deliberately small: see the module docs for the
/// supported subset. Anything outside it is a clear error rather than silently
/// mis-parsed.
pub fn parse(text: &str) -> Result<Toml, TomlError> {
    let mut toml = Toml::default();
    let mut current = String::new(); // root table
    toml.tables.entry(current.clone()).or_default();

    for (i, raw) in text.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        let lineno = i + 1;

        if let Some(header) = line.strip_prefix('[') {
            let header = header.strip_suffix(']').ok_or_else(|| TomlError {
                line: lineno,
                msg: "table header missing closing ']'".to_owned(),
            })?;
            current = header.trim().to_owned();
            if current.is_empty() {
                return Err(TomlError { line: lineno, msg: "empty table header".to_owned() });
            }
            toml.tables.entry(current.clone()).or_default();
            continue;
        }

        let (key, val) = line.split_once('=').ok_or_else(|| TomlError {
            line: lineno,
            msg: "expected 'key = value' or '[table]'".to_owned(),
        })?;
        let key = key.trim().to_owned();
        let value = parse_value(val.trim(), lineno)?;
        toml.tables.entry(current.clone()).or_default().insert(key, value);
    }
    Ok(toml)
}

/// Remove a trailing/whole-line `#` comment, respecting `#` inside quotes.
fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    for (idx, c) in line.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..idx],
            _ => {}
        }
    }
    line
}

fn parse_value(s: &str, line: usize) -> Result<TomlValue, TomlError> {
    if let Some(rest) = s.strip_prefix('[') {
        let inner = rest.strip_suffix(']').ok_or_else(|| TomlError {
            line,
            msg: "array missing closing ']' (multi-line arrays not supported)".to_owned(),
        })?;
        let mut items = Vec::new();
        for part in inner.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue; // tolerate a trailing comma / empty array
            }
            items.push(parse_string(part, line)?);
        }
        return Ok(TomlValue::Array(items));
    }
    match s {
        "true" => Ok(TomlValue::Bool(true)),
        "false" => Ok(TomlValue::Bool(false)),
        _ => Ok(TomlValue::Str(parse_string(s, line)?)),
    }
}

/// A double-quoted string. We do not use escapes in comms.toml, so the only
/// recognized escape is `\"`; everything else is literal.
fn parse_string(s: &str, line: usize) -> Result<String, TomlError> {
    let inner = s
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .ok_or_else(|| TomlError {
            line,
            msg: format!("expected a quoted string, got `{s}`"),
        })?;
    Ok(inner.replace("\\\"", "\""))
}

// ---- harness model ---------------------------------------------------------

/// One declared artifact type (`[artifact_types.<name>]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactType {
    pub name: String,
    pub dir: String,
    pub required_for: Vec<String>,
}

/// One step of a rite: a verb the tool performs, and an optional target it
/// acts on (an artifact-type name or a built-in noun).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub verb: String,
    pub target: Option<String>,
}

impl Step {
    /// Render back to the `"verb target"` form for display.
    pub fn display(&self) -> String {
        match &self.target {
            Some(t) => format!("{} {}", self.verb, t),
            None => self.verb.clone(),
        }
    }
}

/// One declared rite (`[rites.<name>]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rite {
    pub name: String,
    pub steps: Vec<Step>,
    pub allow_waivers: bool,
}

/// The whole harness configuration derived from comms.toml.
#[derive(Debug, Clone)]
pub struct HarnessConfig {
    pub profile: String,
    pub archive_mode: Option<String>,
    pub rites: Vec<Rite>,
    pub artifact_types: Vec<ArtifactType>,
}

impl HarnessConfig {
    pub fn from_toml(toml: &Toml) -> HarnessConfig {
        let profile = toml
            .get("", "profile")
            .and_then(TomlValue::as_str)
            .unwrap_or("default")
            .to_owned();
        let archive_mode = toml
            .get("archive", "mode")
            .and_then(TomlValue::as_str)
            .map(str::to_owned);

        let rites = toml
            .children_of("rites")
            .into_iter()
            .map(|name| {
                let table = format!("rites.{name}");
                let steps = toml
                    .get(&table, "steps")
                    .and_then(TomlValue::as_array)
                    .map(|a| a.iter().map(|s| parse_step(s)).collect())
                    .unwrap_or_default();
                let allow_waivers = toml
                    .get(&table, "allow_waivers")
                    .and_then(TomlValue::as_bool)
                    .unwrap_or(false);
                Rite { name, steps, allow_waivers }
            })
            .collect();

        let artifact_types = toml
            .children_of("artifact_types")
            .into_iter()
            .map(|name| {
                let table = format!("artifact_types.{name}");
                let dir = toml
                    .get(&table, "dir")
                    .and_then(TomlValue::as_str)
                    .unwrap_or(&name)
                    .to_owned();
                let required_for = toml
                    .get(&table, "required_for")
                    .and_then(TomlValue::as_array)
                    .map(|a| a.to_vec())
                    .unwrap_or_default();
                ArtifactType { name, dir, required_for }
            })
            .collect();

        HarnessConfig { profile, archive_mode, rites, artifact_types }
    }

    pub fn rite(&self, name: &str) -> Option<&Rite> {
        self.rites.iter().find(|r| r.name == name)
    }

    pub fn artifact_type(&self, name: &str) -> Option<&ArtifactType> {
        self.artifact_types.iter().find(|a| a.name == name)
    }
}

fn parse_step(s: &str) -> Step {
    let mut parts = s.split_whitespace();
    let verb = parts.next().unwrap_or("").to_owned();
    let target = parts.next().map(str::to_owned);
    Step { verb, target }
}

/// Load and parse `<comms_dir>/comms.toml`.
pub fn load(comms_dir: &Path) -> Result<HarnessConfig, String> {
    let path = comms_dir.join("comms.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let toml = parse(&text).map_err(|e| e.to_string())?;
    Ok(HarnessConfig::from_toml(&toml))
}

// ---- tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
schema = "comms-harness/1"
profile = "continuity"

[archive]
mode = "external"          # external | ignored-local
index = "sqlite"

[rites.open]
steps = ["mint session", "attest entry"]

[rites.close]
steps = ["attest transcript", "seal store", "shred session"]
allow_waivers = true

[artifact_types.transcripts]
dir = "transcripts"
required_for = ["close"]
"#;

    #[test]
    fn parses_tables_scalars_and_arrays() {
        let t = parse(SAMPLE).unwrap();
        assert_eq!(t.get("", "profile").unwrap().as_str(), Some("continuity"));
        assert_eq!(t.get("archive", "mode").unwrap().as_str(), Some("external"));
        assert_eq!(t.get("rites.close", "allow_waivers").unwrap().as_bool(), Some(true));
        assert_eq!(
            t.get("rites.open", "steps").unwrap().as_array(),
            Some(["mint session".to_owned(), "attest entry".to_owned()].as_slice())
        );
    }

    #[test]
    fn hash_inside_a_quoted_string_is_not_a_comment() {
        let t = parse("[a]\nk = \"v # not a comment\"\n").unwrap();
        assert_eq!(t.get("a", "k").unwrap().as_str(), Some("v # not a comment"));
    }

    #[test]
    fn children_of_lists_table_leaves() {
        let t = parse(SAMPLE).unwrap();
        assert_eq!(t.children_of("rites"), vec!["close", "open"]);
        assert_eq!(t.children_of("artifact_types"), vec!["transcripts"]);
    }

    #[test]
    fn config_model_binds_steps_and_artifacts() {
        let cfg = HarnessConfig::from_toml(&parse(SAMPLE).unwrap());
        assert_eq!(cfg.profile, "continuity");
        assert_eq!(cfg.archive_mode.as_deref(), Some("external"));

        let close = cfg.rite("close").unwrap();
        assert!(close.allow_waivers);
        assert_eq!(
            close.steps,
            vec![
                Step { verb: "attest".into(), target: Some("transcript".into()) },
                Step { verb: "seal".into(), target: Some("store".into()) },
                Step { verb: "shred".into(), target: Some("session".into()) },
            ]
        );

        let open = cfg.rite("open").unwrap();
        assert_eq!(open.steps[0].display(), "mint session");

        let tx = cfg.artifact_type("transcripts").unwrap();
        assert_eq!(tx.dir, "transcripts");
        assert_eq!(tx.required_for, vec!["close".to_owned()]);
    }

    #[test]
    fn malformed_lines_error_with_line_numbers() {
        assert_eq!(parse("[unclosed\n").unwrap_err().line, 1);
        assert_eq!(parse("profile\n").unwrap_err().line, 1); // no '='
        assert_eq!(parse("[a]\nk = [\"x\"\n").unwrap_err().line, 2); // unclosed array
    }
}
