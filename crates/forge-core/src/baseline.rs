use crate::Diagnostic;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineEntry {
    pub code: String,
    pub file: String,
    pub line_content: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Baseline {
    pub entries: Vec<BaselineEntry>,
}

impl Baseline {
    pub fn load(path: &Path) -> Result<Self, std::io::Error> {
        let text = std::fs::read_to_string(path)?;
        let b: Baseline = serde_json::from_str(&text)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(b)
    }

    pub fn save(&self, path: &Path) -> Result<(), std::io::Error> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, text)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Remove de `diagnostics` tudo que casa com uma entrada do baseline.
    /// A chave é `(code, file, line_content)` — não depende do número de
    /// linha, então o baseline resiste a mudanças de posição no arquivo.
    pub fn filter(
        &self,
        diagnostics: Vec<Diagnostic>,
        filepath: &str,
        source: &str,
    ) -> Vec<Diagnostic> {
        if self.entries.is_empty() {
            return diagnostics;
        }
        let set: HashSet<(&str, &str, &str)> = self
            .entries
            .iter()
            .map(|e| (e.code.as_str(), e.file.as_str(), e.line_content.as_str()))
            .collect();
        let lines: Vec<&str> = source.lines().collect();
        diagnostics
            .into_iter()
            .filter(|d| {
                let Some(line) = lines.get(d.range.start_line) else {
                    return true;
                };
                let key = (d.code.as_str(), filepath, *line);
                !set.contains(&key)
            })
            .collect()
    }
}

/// Constrói entradas de baseline a partir dos diagnósticos atuais.
pub fn build_from(diagnostics: &[Diagnostic], filepath: &str, source: &str) -> Vec<BaselineEntry> {
    let lines: Vec<&str> = source.lines().collect();
    diagnostics
        .iter()
        .filter_map(|d| {
            let line = lines.get(d.range.start_line)?;
            Some(BaselineEntry {
                code: d.code.clone(),
                file: filepath.to_string(),
                line_content: line.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Diagnostic, Range, Severity};

    fn diag(code: &str, line: usize, col: usize) -> Diagnostic {
        Diagnostic::new(
            code,
            "msg",
            Range {
                start_line: line,
                start_col: col,
                end_line: line,
                end_col: col + 1,
            },
            Severity::Warning,
        )
    }

    #[test]
    fn filter_suprime_por_codigo_e_linha() {
        let source = "import os\nimport sys\n";
        let diags = vec![diag("FOR006", 0, 0), diag("FOR006", 1, 0)];
        let baseline = Baseline {
            entries: vec![BaselineEntry {
                code: "FOR006".to_string(),
                file: "a.py".to_string(),
                line_content: "import os".to_string(),
            }],
        };
        let rest = baseline.filter(diags, "a.py", source);
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].range.start_line, 1);
    }

    #[test]
    fn filter_nao_suprime_se_linha_mudou() {
        let source = "import os  # editado\n";
        let diags = vec![diag("FOR006", 0, 0)];
        let baseline = Baseline {
            entries: vec![BaselineEntry {
                code: "FOR006".to_string(),
                file: "a.py".to_string(),
                line_content: "import os".to_string(),
            }],
        };
        let rest = baseline.filter(diags, "a.py", source);
        assert_eq!(rest.len(), 1);
    }

    #[test]
    fn filter_nao_suprime_outro_arquivo() {
        let source = "import os\n";
        let diags = vec![diag("FOR006", 0, 0)];
        let baseline = Baseline {
            entries: vec![BaselineEntry {
                code: "FOR006".to_string(),
                file: "b.py".to_string(),
                line_content: "import os".to_string(),
            }],
        };
        let rest = baseline.filter(diags, "a.py", source);
        assert_eq!(rest.len(), 1);
    }

    #[test]
    fn build_from_extrai_linhas() {
        let source = "import os\nprint(1)\n";
        let diags = vec![diag("FOR006", 0, 0)];
        let entries = build_from(&diags, "a.py", source);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].code, "FOR006");
        assert_eq!(entries[0].file, "a.py");
        assert_eq!(entries[0].line_content, "import os");
    }
}
