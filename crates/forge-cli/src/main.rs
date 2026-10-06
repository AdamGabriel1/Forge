use clap::{Parser, Subcommand, ValueEnum};
use forge_core::{
    apply_edits, baseline::build_from, baseline::Baseline, noqa, Config, Diagnostic, Severity,
};
use forge_format::format_source;
use forge_lint::{default_registry, Context, RuleRegistry};
use forge_parser::{get_parser, parse_python_source};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::Path;
use std::{fs, path::PathBuf, process};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "forge")]
#[command(author = "Adam")]
#[command(version = "0.1.0")]
#[command(about = "Linter e Formatter para Python", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Copy, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
enum SummaryMode {
    None,
    ByRule,
    ByFile,
}

#[derive(Subcommand)]
enum Commands {
    /// Roda o linter nos arquivos/diretórios
    Check {
        path: PathBuf,
        /// Trata warnings como erros no exit code.
        #[arg(long)]
        strict: bool,
        /// Formato de saída.
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
        /// Silencia diagnósticos que já estão neste baseline (JSON).
        #[arg(long)]
        baseline: Option<PathBuf>,
        /// Agrupa a saída por regra ou arquivo em vez de listar tudo.
        #[arg(long, value_enum, default_value_t = SummaryMode::None)]
        summary: SummaryMode,
    },
    /// Formata o código
    Fmt {
        path: PathBuf,
        /// Não escreve no disco; sai com código 1 se algum arquivo
        /// precisa ser formatado. Útil em CI.
        #[arg(long)]
        check: bool,
    },
    /// Aplica correções automáticas
    Fix {
        path: PathBuf,
        /// Não escreve no disco, só mostra o que seria feito.
        #[arg(long)]
        dry_run: bool,
        /// Sai com código 1 se houver correções pendentes. Útil em CI.
        #[arg(long)]
        check: bool,
    },
    /// Gera um baseline a partir dos diagnósticos atuais
    Baseline {
        path: PathBuf,
        /// Onde salvar o baseline (default: forge-baseline.json no CWD).
        #[arg(long, default_value = "forge-baseline.json")]
        output: PathBuf,
    },
    /// Explica uma regra (ou lista todas com --list)
    Explain {
        code: Option<String>,
        #[arg(long)]
        list: bool,
    },
}

#[derive(Serialize)]
struct JsonDiagnostic<'a> {
    code: &'a str,
    severity: &'a str,
    message: &'a str,
    file: &'a str,
    line: usize,
    column: usize,
    end_line: usize,
    end_column: usize,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Check {
            path,
            strict,
            format,
            baseline,
            summary,
        } => run_check(path, strict, format, baseline, summary),
        Commands::Fmt { path, check } => run_fmt(path, check),
        Commands::Fix {
            path,
            dry_run,
            check,
        } => run_fix(path, dry_run, check),
        Commands::Baseline { path, output } => run_baseline(path, output),
        Commands::Explain { code, list } => run_explain(code, list),
    }
}

fn load_config(path: &Path) -> Config {
    let start = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(Path::new("."))
    };
    match Config::find_and_load(start) {
        Ok(Some((cfg_path, cfg))) => {
            eprintln!("Carregando config: {}", cfg_path.display());
            cfg
        }
        Ok(None) => Config::default(),
        Err(e) => {
            eprintln!("Erro ao carregar forge.toml: {}", e);
            process::exit(2);
        }
    }
}

/// Coleta todos os arquivos `.py` sob `path` em ordem determinística.
fn collect_python_files(path: &Path) -> Vec<PathBuf> {
    WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "py"))
        .map(|e| e.into_path())
        .collect()
}

fn collect_diagnostics(
    source: &str,
    filepath: &str,
    config: &Config,
    registry: &RuleRegistry,
    parser: &mut tree_sitter::Parser,
) -> Vec<Diagnostic> {
    let Some(tree) = parse_python_source(parser, source) else {
        return Vec::new();
    };
    let ctx = Context::new(source, filepath, config, tree.root_node());
    let mut diags: Vec<Diagnostic> = Vec::new();
    for rule in registry.all() {
        if !config.lint.is_enabled(rule.code()) {
            continue;
        }
        let mut found = rule.check(tree.root_node(), &ctx);
        for d in &mut found {
            if let Some(sev) = config.lint.severity_for(&d.code) {
                d.severity = sev;
            }
        }
        diags.extend(found);
    }
    diags
}

fn load_baseline(path: &Option<PathBuf>) -> Option<Baseline> {
    let path = path.as_ref()?;
    match Baseline::load(path) {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("Erro ao carregar baseline {}: {}", path.display(), e);
            process::exit(2);
        }
    }
}

fn sort_diagnostics(v: &mut [(String, Diagnostic)]) {
    v.sort_by(|(fa, da), (fb, db)| {
        fa.cmp(fb)
            .then(da.range.start_line.cmp(&db.range.start_line))
            .then(da.range.start_col.cmp(&db.range.start_col))
            .then(da.code.cmp(&db.code))
    });
}

fn run_check(
    path: PathBuf,
    strict: bool,
    format: OutputFormat,
    baseline_path: Option<PathBuf>,
    summary: SummaryMode,
) {
    let config = load_config(&path);
    let registry = default_registry();
    let baseline = load_baseline(&baseline_path);

    let files = collect_python_files(&path);

    let per_file: Vec<(String, Vec<Diagnostic>)> = files
        .par_iter()
        .map_init(
            get_parser,
            |parser, filepath| -> Option<(String, Vec<Diagnostic>)> {
                let source = match fs::read_to_string(filepath) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Erro ao ler {}: {}", filepath.display(), e);
                        return None;
                    }
                };
                let filepath_str = filepath.to_string_lossy().into_owned();
                let diags = collect_diagnostics(&source, &filepath_str, &config, &registry, parser);
                let diags = noqa::filter_suppressed(diags, &source);
                let diags = match &baseline {
                    Some(b) => b.filter(diags, &filepath_str, &source),
                    None => diags,
                };
                Some((filepath_str, diags))
            },
        )
        .filter_map(|x| x)
        .collect();

    let mut collected: Vec<(String, Diagnostic)> = per_file
        .into_iter()
        .flat_map(|(f, ds)| ds.into_iter().map(move |d| (f.clone(), d)))
        .collect();
    sort_diagnostics(&mut collected);

    let has_error = collected
        .iter()
        .any(|(_, d)| matches!(d.severity, Severity::Error));
    let has_warning = collected
        .iter()
        .any(|(_, d)| matches!(d.severity, Severity::Warning));

    match (summary, format) {
        (SummaryMode::None, OutputFormat::Human) => {
            let use_color = std::io::stdout().is_terminal();
            if collected.is_empty() {
                println!("Nenhum problema encontrado!");
            } else {
                for (file, d) in &collected {
                    println!("{}", d.format(file, use_color));
                }
            }
        }
        (SummaryMode::None, OutputFormat::Json) => {
            let arr: Vec<JsonDiagnostic> = collected
                .iter()
                .map(|(file, d)| JsonDiagnostic {
                    code: &d.code,
                    severity: d.severity.as_str(),
                    message: &d.message,
                    file,
                    line: d.range.start_line + 1,
                    column: d.range.start_col + 1,
                    end_line: d.range.end_line + 1,
                    end_column: d.range.end_col + 1,
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&arr).unwrap_or_else(|_| "[]".to_string())
            );
        }
        (SummaryMode::ByRule, _) => print_summary_by_rule(&collected, &registry),
        (SummaryMode::ByFile, _) => print_summary_by_file(&collected),
    }

    if has_error || (strict && has_warning) {
        process::exit(1);
    }
}

fn print_summary_by_rule(collected: &[(String, Diagnostic)], registry: &RuleRegistry) {
    if collected.is_empty() {
        println!("Nenhum problema encontrado!");
        return;
    }
    // Ordena por contagem desc, desempate por código.
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (_, d) in collected {
        *counts.entry(d.code.clone()).or_insert(0) += 1;
    }
    let mut entries: Vec<(String, usize)> = counts.into_iter().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    let width = entries
        .iter()
        .map(|(_, n)| n.to_string().len())
        .max()
        .unwrap_or(0);
    let total: usize = entries.iter().map(|(_, n)| n).sum();
    println!("{total} problema(s):");
    for (code, n) in entries {
        let name = registry
            .find(&code)
            .map(|r| r.name())
            .unwrap_or("<desconhecida>");
        println!("  {n:>width$}  {code}  {name}");
    }
}

fn print_summary_by_file(collected: &[(String, Diagnostic)]) {
    if collected.is_empty() {
        println!("Nenhum problema encontrado!");
        return;
    }
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (file, _) in collected {
        *counts.entry(file.clone()).or_insert(0) += 1;
    }
    let mut entries: Vec<(String, usize)> = counts.into_iter().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    let width = entries
        .iter()
        .map(|(_, n)| n.to_string().len())
        .max()
        .unwrap_or(0);
    let total: usize = entries.iter().map(|(_, n)| n).sum();
    println!("{total} problema(s) em {} arquivo(s):", entries.len());
    for (file, n) in entries {
        println!("  {n:>width$}  {file}");
    }
}

fn run_fmt(path: PathBuf, check: bool) {
    let files = collect_python_files(&path);

    let per_file: Vec<(String, String)> = files
        .par_iter()
        .filter_map(|filepath| {
            let source = match fs::read_to_string(filepath) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Erro ao ler {}: {}", filepath.display(), e);
                    return None;
                }
            };
            let formatted = match format_source(&source) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("Erro ao parsear {}: {}", filepath.display(), e);
                    return None;
                }
            };
            if formatted == source {
                return None;
            }
            Some((filepath.to_string_lossy().into_owned(), formatted))
        })
        .collect();

    let mut per_file = per_file;
    per_file.sort_by(|a, b| a.0.cmp(&b.0));

    let mut changed = 0usize;
    for (filepath_str, formatted) in per_file {
        changed += 1;
        if check {
            println!("{}: precisa formatar", filepath_str);
            continue;
        }
        if let Err(e) = fs::write(&filepath_str, &formatted) {
            eprintln!("Erro ao escrever {}: {}", filepath_str, e);
            continue;
        }
        println!("{}: formatado", filepath_str);
    }

    if check && changed > 0 {
        process::exit(1);
    }
    if changed == 0 {
        println!("Tudo formatado.");
    } else if !check {
        println!("Resumo: {} arquivo(s) formatado(s).", changed);
    }
}

fn run_baseline(path: PathBuf, output: PathBuf) {
    let config = load_config(&path);
    let registry = default_registry();

    let files = collect_python_files(&path);

    let per_file: Vec<Vec<forge_core::baseline::BaselineEntry>> = files
        .par_iter()
        .map_init(get_parser, |parser, filepath| {
            let source = match fs::read_to_string(filepath) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Erro ao ler {}: {}", filepath.display(), e);
                    return Vec::new();
                }
            };
            let filepath_str = filepath.to_string_lossy().into_owned();
            let diags = collect_diagnostics(&source, &filepath_str, &config, &registry, parser);
            let diags = noqa::filter_suppressed(diags, &source);
            build_from(&diags, &filepath_str, &source)
        })
        .collect();

    let mut entries: Vec<forge_core::baseline::BaselineEntry> =
        per_file.into_iter().flatten().collect();
    entries.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.line_content.cmp(&b.line_content))
            .then(a.code.cmp(&b.code))
    });

    let baseline = Baseline { entries };
    if let Err(e) = baseline.save(&output) {
        eprintln!("Erro ao salvar baseline: {}", e);
        process::exit(2);
    }
    println!(
        "Baseline salvo em {} ({} entrada(s)).",
        output.display(),
        baseline.entries.len()
    );
}

fn run_fix(path: PathBuf, dry_run: bool, check: bool) {
    let config = load_config(&path);
    let registry = default_registry();

    let files = collect_python_files(&path);

    let per_file: Vec<(String, usize, String)> = files
        .par_iter()
        .map_init(
            get_parser,
            |parser, filepath| -> Option<(String, usize, String)> {
                let source = match fs::read_to_string(filepath) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Erro ao ler {}: {}", filepath.display(), e);
                        return None;
                    }
                };
                let tree = parse_python_source(parser, &source)?;
                let filepath_str = filepath.to_string_lossy().into_owned();
                let ctx = Context::new(&source, &filepath_str, &config, tree.root_node());
                let mut edits = Vec::new();
                for rule in registry.all() {
                    if !config.lint.is_enabled(rule.code()) {
                        continue;
                    }
                    let diags = rule.check(tree.root_node(), &ctx);
                    if diags.is_empty() {
                        continue;
                    }
                    edits.extend(rule.fix(tree.root_node(), &ctx, &diags));
                }
                if edits.is_empty() {
                    return None;
                }
                let n = edits.len();
                let novo = apply_edits(&source, edits);
                Some((filepath_str, n, novo))
            },
        )
        .filter_map(|x| x)
        .collect();

    let mut per_file = per_file;
    per_file.sort_by(|a, b| a.0.cmp(&b.0));

    let mut total_edits = 0usize;
    let mut files_changed = 0usize;

    for (filepath_str, n_edits, novo) in per_file {
        total_edits += n_edits;
        files_changed += 1;

        if check {
            println!("{}: {} correção(ões) disponível(is)", filepath_str, n_edits);
            continue;
        }
        if dry_run {
            println!("--- {} (dry-run) ---", filepath_str);
            println!("{}", novo);
            continue;
        }
        if let Err(e) = fs::write(&filepath_str, &novo) {
            eprintln!("Erro ao escrever {}: {}", filepath_str, e);
            continue;
        }
        println!("{}: {} correção(ões) aplicada(s)", filepath_str, n_edits);
    }

    if check && files_changed > 0 {
        process::exit(1);
    }
    if total_edits == 0 {
        println!("Nada a corrigir.");
    } else if !dry_run && !check {
        println!(
            "Resumo: {} correção(ões) em {} arquivo(s).",
            total_edits, files_changed
        );
    }
}

fn run_explain(code: Option<String>, list: bool) {
    let registry = default_registry();
    if list || code.is_none() {
        list_rules(&registry);
        return;
    }
    let code = code.unwrap();
    match registry.find(&code) {
        Some(rule) => {
            println!("{}: {}", rule.code(), rule.name());
            println!("Descrição: {}", rule.description());
            println!("Correção:  {}", rule.fix_hint());
        }
        None => {
            eprintln!("Regra {} não encontrada.", code);
            process::exit(2);
        }
    }
}

fn list_rules(registry: &RuleRegistry) {
    println!("Regras registradas:");
    for rule in registry.all() {
        println!("  {} — {}", rule.code(), rule.name());
    }
}
