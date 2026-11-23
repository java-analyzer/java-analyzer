//! Anchor-based benchmark probe manifest and UTF-16 position resolution.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Embedded default benchmark manifest.
pub const EMBEDDED_PROBES_TOML: &str = include_str!("../../bench/probes.toml");

/// Corpus size tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Small,
    Medium,
    Large,
}

impl Tier {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "small" => Ok(Self::Small),
            "medium" => Ok(Self::Medium),
            "large" => Ok(Self::Large),
            other => Err(format!(
                "invalid tier `{other}` (expected small, medium, or large)"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }
}

/// Repository build tool and module shape classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Shape {
    Plain,
    Maven,
    Gradle,
    Ant,
    SingleModule,
    MultiModule,
}

impl Shape {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "plain" => Ok(Self::Plain),
            "maven" => Ok(Self::Maven),
            "gradle" => Ok(Self::Gradle),
            "ant" => Ok(Self::Ant),
            "single-module" => Ok(Self::SingleModule),
            "multi-module" => Ok(Self::MultiModule),
            other => Err(format!(
                "invalid shape `{other}` (expected plain, maven, gradle, ant, single-module, or multi-module)"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Maven => "maven",
            Self::Gradle => "gradle",
            Self::Ant => "ant",
            Self::SingleModule => "single-module",
            Self::MultiModule => "multi-module",
        }
    }
}

/// Benchmark operation category filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpCategory {
    Lifecycle,
    Nav,
    Hierarchy,
    Refactor,
    Completion,
    Edit,
    Present,
}

impl OpCategory {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "lifecycle" => Ok(Self::Lifecycle),
            "nav" => Ok(Self::Nav),
            "hierarchy" => Ok(Self::Hierarchy),
            "refactor" => Ok(Self::Refactor),
            "completion" => Ok(Self::Completion),
            "edit" => Ok(Self::Edit),
            "present" => Ok(Self::Present),
            other => Err(format!(
                "invalid op category `{other}` (expected lifecycle, nav, hierarchy, refactor, completion, edit, or present)"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lifecycle => "lifecycle",
            Self::Nav => "nav",
            Self::Hierarchy => "hierarchy",
            Self::Refactor => "refactor",
            Self::Completion => "completion",
            Self::Edit => "edit",
            Self::Present => "present",
        }
    }

    /// All categories in display order.
    pub const ALL: [Self; 7] = [
        Self::Lifecycle,
        Self::Nav,
        Self::Hierarchy,
        Self::Refactor,
        Self::Completion,
        Self::Edit,
        Self::Present,
    ];
}

/// Top-level benchmark manifest parsed from `probes.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BenchManifest {
    #[serde(default)]
    pub repos: Vec<RepoSpec>,
}

impl BenchManifest {
    /// Parses a TOML probe manifest string.
    pub fn from_toml_str(text: &str) -> Result<Self, String> {
        toml::from_str::<Self>(text).map_err(|e| format!("invalid probes manifest: {e}"))
    }

    /// Parses the embedded `xtask/bench/probes.toml` manifest.
    pub fn embedded() -> Result<Self, String> {
        Self::from_toml_str(EMBEDDED_PROBES_TOML)
    }
}

/// Repository definition and its anchor-based LSP probes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSpec {
    pub id: String,
    pub tier: Tier,
    #[serde(default)]
    pub shapes: Vec<Shape>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub approx_files: usize,
    #[serde(default)]
    pub ready_probe: Option<String>,
    #[serde(default)]
    pub clean_files: Vec<String>,
    #[serde(default)]
    pub probes: BTreeMap<String, ProbeSpec>,
    #[serde(default)]
    pub edits: BTreeMap<String, EditProbeSpec>,
    #[serde(default)]
    pub completions: BTreeMap<String, CompletionProbeSpec>,
}

/// Anchor-based LSP request probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeSpec {
    pub file: String,
    #[serde(default)]
    pub anchor: String,
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub expect: Option<String>,
    #[serde(default)]
    pub new_name: Option<String>,
    #[serde(default)]
    pub end_anchor: Option<String>,
    #[serde(default)]
    pub end_symbol: Option<String>,
}

/// In-memory buffer edit probe (`didChange` -> follow-up request -> revert).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditProbeSpec {
    pub file: String,
    pub anchor: String,
    #[serde(default)]
    pub symbol: String,
    pub replace: String,
    pub with: String,
    #[serde(default)]
    pub hover_anchor: Option<String>,
    #[serde(default)]
    pub hover_symbol: Option<String>,
    #[serde(default)]
    pub expect: Option<String>,
}

/// In-memory completion probe (`didChange` -> `completion` -> revert).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionProbeSpec {
    pub file: String,
    pub anchor: String,
    pub replace: String,
    pub with: String,
    pub cursor_after: String,
    #[serde(default)]
    pub trigger_character: Option<String>,
    #[serde(default)]
    pub expect: Option<String>,
}

/// Resolves `(anchor, symbol)` inside `text` to 0-based LSP `(line, utf16_character)`.
///
/// Locates the first line containing `anchor`. When `symbol` is non-empty, locates `symbol`
/// within the matched `anchor` substring first (falling back to elsewhere on that line), then
/// converts the UTF-8 byte column to UTF-16 code units.
pub fn resolve_anchor_utf16(text: &str, anchor: &str, symbol: &str) -> Result<(u32, u32), String> {
    if anchor.is_empty() {
        return Err("anchor must not be empty".to_string());
    }
    for (line_idx, raw_line) in text.split('\n').enumerate() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let Some(anchor_col) = line.find(anchor) else {
            continue;
        };
        let byte_col = if symbol.is_empty() {
            anchor_col
        } else {
            let anchor_end = anchor_col + anchor.len();
            let in_anchor = line
                .get(anchor_col..anchor_end)
                .and_then(|slice| slice.find(symbol))
                .map(|rel| anchor_col + rel);
            let after_anchor = line
                .get(anchor_col..)
                .and_then(|slice| slice.find(symbol))
                .map(|rel| anchor_col + rel);
            let anywhere = line.find(symbol);
            in_anchor
                .or(after_anchor)
                .or(anywhere)
                .ok_or_else(|| format!("symbol `{symbol}` not found on line matching `{anchor}`"))?
        };
        let prefix = line
            .get(..byte_col)
            .ok_or_else(|| "byte column out of bounds".to_string())?;
        let utf16_col = prefix.encode_utf16().count();
        let line_u32 = u32::try_from(line_idx).map_err(|_| "line index overflow".to_string())?;
        let col_u32 =
            u32::try_from(utf16_col).map_err(|_| "utf16 column index overflow".to_string())?;
        return Ok((line_u32, col_u32));
    }
    Err(format!("anchor `{anchor}` not found"))
}

/// Resolves the cursor position immediately after `cursor_after` on the line containing `anchor_line_hint`.
pub fn resolve_cursor_after_utf16(
    text: &str,
    anchor_line_hint: &str,
    cursor_after: &str,
) -> Result<(u32, u32), String> {
    for (line_idx, raw_line) in text.split('\n').enumerate() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if !anchor_line_hint.is_empty() && !line.contains(anchor_line_hint) {
            continue;
        }
        let Some(pos) = line.find(cursor_after) else {
            continue;
        };
        let end_byte = pos + cursor_after.len();
        let prefix = line
            .get(..end_byte)
            .ok_or_else(|| "cursor byte column out of bounds".to_string())?;
        let utf16_col = prefix.encode_utf16().count();
        let line_u32 = u32::try_from(line_idx).map_err(|_| "line index overflow".to_string())?;
        let col_u32 =
            u32::try_from(utf16_col).map_err(|_| "utf16 column index overflow".to_string())?;
        return Ok((line_u32, col_u32));
    }
    Err(format!(
        "cursor_after `{cursor_after}` not found (hint `{anchor_line_hint}`)"
    ))
}
