use crate::Diagnostic;
use std::collections::HashSet;

/// Filtra diagnósticos com base em comentários `# noqa`.
///
/// - `# noqa` → suprime tudo na linha.
/// - `# noqa: FOR001` → suprime apenas a regra na linha.
/// - `# noqa: FOR001, FOR002` → suprime várias.
pub fn filter_suppressed(
    diagnostics: Vec<Diagnostic>,
    source: &str,
) -> Vec<Diagnostic> {
    let entries = parse_noqa(source);
    diagnostics
        .into_iter()
        .filter(|d| !is_suppressed(d, &entries))
        .collect()
}

struct NoqaEntry {
    line: usize,
    codes: Option<HashSet<String>>,
}

fn parse_noqa(source: &str) -> Vec<NoqaEntry> {
    let mut out = Vec::new();
    for (idx, line) in source.lines().enumerate() {
        if let Some(entry) = parse_noqa_line(line, idx) {
            out.push(entry);
        }
    }
    out
}

fn parse_noqa_line(line: &str, line_idx: usize) -> Option<NoqaEntry> {
    let idx = line.find("# noqa")?;
    // Evita falso positivo dentro de string literal: exige que antes do `#`
    // não haja aspas desbalanceadas.
    let before = &line[..idx];
    let dq = before.matches('"').count();
    let sq = before.matches('\'').count();
    if dq % 2 == 1 || sq % 2 == 1 {
        return None;
    }
    let after = &line[idx + "# noqa".len()..];
    let after = after.trim_start();
    if let Some(rest) = after.strip_prefix(':') {
        let codes: HashSet<String> = rest
            .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Some(NoqaEntry {
            line: line_idx,
            codes: Some(codes),
        })
    } else {
        Some(NoqaEntry {
            line: line_idx,
            codes: None,
        })
    }
}

fn is_suppressed(d: &Diagnostic, entries: &[NoqaEntry]) -> bool {
    for entry in entries {
        if entry.line != d.range.start_line {
            continue;
        }
        match &entry.codes {
            None => return true,
            Some(codes) => {
                if codes.contains(&d.code) {
                    return true;
                }
            }
        }
    }
    false
}
