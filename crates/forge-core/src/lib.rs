use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub mod noqa;

// ---------------------------------------------------------------------------
// Diagnóstico
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Range {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    pub range: Range,
    pub severity: Severity,
}

impl Diagnostic {
    pub fn new(code: &str, message: &str, range: Range, severity: Severity) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
            range,
            severity,
        }
    }

    pub fn format(&self, filepath: &str, use_color: bool) -> String {
        let sev = self.severity.as_str();
        let loc = format!(
            "{}:{}:{}",
            filepath,
            self.range.start_line + 1,
            self.range.start_col + 1
        );
        if !use_color {
            return format!("{} - {} {} - {}", loc, self.code, sev, self.message);
        }
        match self.severity {
            Severity::Error => format!(
                "{} - {} {} - {}",
                loc.blue(),
                self.code.red().bold(),
                "error".red().bold(),
                self.message
            ),
            Severity::Warning => format!(
                "{} - {} {} - {}",
                loc.blue(),
                self.code.yellow().bold(),
                "warning".yellow().bold(),
                self.message
            ),
            Severity::Info => format!(
                "{} - {} {} - {}",
                loc.blue(),
                self.code.cyan().bold(),
                "info".cyan().bold(),
                self.message
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Edição (autofix)
// ---------------------------------------------------------------------------

/// Uma edição textual num arquivo. Os offsets são **byte offsets** na fonte
/// original (UTF-8), como os que o tree-sitter expõe.
#[derive(Debug, Clone)]
pub struct Edit {
    pub start_byte: usize,
    pub end_byte: usize,
    pub replacement: String,
}

impl Edit {
    pub fn delete(start_byte: usize, end_byte: usize) -> Self {
        Self {
            start_byte,
            end_byte,
            replacement: String::new(),
        }
    }

    pub fn replace(start_byte: usize, end_byte: usize, replacement: impl Into<String>) -> Self {
        Self {
            start_byte,
            end_byte,
            replacement: replacement.into(),
        }
    }
}

/// Aplica uma lista de edits a um texto. Ordena do fim para o começo para
/// que offsets anteriores permaneçam válidos.
pub fn apply_edits(source: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|e| std::cmp::Reverse(e.start_byte));
    let mut out = source.to_string();
    for e in edits {
        debug_assert!(
            out.is_char_boundary(e.start_byte) && out.is_char_boundary(e.end_byte),
            "edit em offset fora de char boundary"
        );
        out.replace_range(e.start_byte..e.end_byte, &e.replacement);
    }
    out
}

// ---------------------------------------------------------------------------
// Contexto
// ---------------------------------------------------------------------------

pub struct Context<'a> {
    pub source: &'a str,
    pub filepath: &'a str,
    pub config: &'a Config,
}

// ---------------------------------------------------------------------------
// Configuração
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub lint: LintConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LintConfig {
    pub select: Vec<String>,
    pub ignore: Vec<String>,
    pub severity: HashMap<String, String>,
    pub options: HashMap<String, toml::Value>,
}

impl LintConfig {
    pub fn is_enabled(&self, code: &str) -> bool {
        if self.ignore.iter().any(|c| c == code) {
            return false;
        }
        if self.select.is_empty() {
            return true;
        }
        self.select.iter().any(|c| c == code)
    }

    pub fn severity_for(&self, code: &str) -> Option<Severity> {
        self.severity.get(code).and_then(|s| match s.as_str() {
            "error" => Some(Severity::Error),
            "warning" => Some(Severity::Warning),
            "info" => Some(Severity::Info),
            _ => None,
        })
    }

    pub fn option_usize(&self, code: &str, key: &str) -> Option<usize> {
        self.options
            .get(code)
            .and_then(|v| v.get(key))
            .and_then(|v| v.as_integer())
            .map(|i| i.max(0) as usize)
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(toml::de::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "erro de I/O: {}", e),
            ConfigError::Parse(e) => write!(f, "erro de parsing: {}", e),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        ConfigError::Io(e)
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(e: toml::de::Error) -> Self {
        ConfigError::Parse(e)
    }
}

impl Config {
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        let cfg: Config = toml::from_str(&text)?;
        Ok(cfg)
    }

    pub fn find_and_load(start: &Path) -> Result<Option<(PathBuf, Self)>, ConfigError> {
        let mut current = Some(start);
        while let Some(dir) = current {
            let candidate = dir.join("forge.toml");
            if candidate.is_file() {
                let cfg = Self::load_from(&candidate)?;
                return Ok(Some((candidate, cfg)));
            }
            current = dir.parent();
        }
        Ok(None)
    }
}