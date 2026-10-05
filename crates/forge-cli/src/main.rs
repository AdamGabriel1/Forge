use clap::{Parser, Subcommand, ValueEnum};
use forge_core::{apply_edits, noqa, Config, Context, Diagnostic, Severity};
use forge_lint::{default_registry, RuleRegistry};
use forge_parser::{get_parser, parse_python_source};
use serde::Serialize;
use std::io::IsTerminal;
use std::{fs, path::PathBuf, process};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "forge")]
#[command(author = "Você")]
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
    },
    /// Formata o código (ainda não implementado)
    Fmt {
        path: PathBuf,
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
        } => run_check(path, strict, format),
        Commands::Fmt { path: _, check: _ } => println!("Formatter ainda não implementado."),
        Commands::Fix {
            path,
            dry_run,
            check,
        } => run_fix(path, dry_run, check),
        Commands::Explain { code, list } => run_explain(code, list),
    }
}

fn load_config(path: &std::path::Path) -> Config {
    let start = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(std::path::Path::new("."))
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

fn run_check(path: PathBuf, strict: bool, format: OutputFormat) {
    let config = load_config(&path);
    let registry = default_registry();
    let mut parser = get_parser();

    let mut collected: Vec<(String, Diagnostic)> = Vec::new();

    for entry in WalkDir::new(&path).into_iter().filter_map(|e| e.ok()) {
        if !entry.path().extension().map_or(false, |ext| ext == "py") {
            continue;
        }
        let filepath = entry.path();
        let source = match fs::read_to_string(filepath) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Erro ao ler {}: {}", filepath.display(), e);
                continue;
            }
        };
        let tree = match parse_python_source(&mut parser, &source) {
            Some(t) => t,
            None => continue,
        };
        let ctx = Context {
            source: &source,
            filepath: filepath.to_str().unwrap_or("<unknown>"),
            config: &config,
        };

        let mut diags: Vec<Diagnostic> = Vec::new();
        for rule in registry.all() {
            if !config.lint.is_enabled(rule.code()) {
                continue;
            }
            let mut found = rule.check(tree.root_node(), &ctx);
            for diag in &mut found {
                if let Some(sev) = config.lint.severity_for(&diag.code) {
                    diag.severity = sev;
                }
            }
            diags.extend(found);
        }

        let diags = noqa::filter_suppressed(diags, &source);
        for d in diags {
            collected.push((ctx.filepath.to_string(), d));
        }
    }

    let has_error = collected
        .iter()
        .any(|(_, d)| matches!(d.severity, Severity::Error));
    let has_warning = collected
        .iter()
        .any(|(_, d)| matches!(d.severity, Severity::Warning));

    match format {
        OutputFormat::Human => {
            let use_color = std::io::stdout().is_terminal();
            if collected.is_empty() {
                println!("Nenhum problema encontrado!");
            } else {
                for (file, d) in &collected {
                    println!("{}", d.format(file, use_color));
                }
            }
        }
        OutputFormat::Json => {
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
    }

    if has_error || (strict && has_warning) {
        process::exit(1);
    }
}

fn run_fix(path: PathBuf, dry_run: bool, check: bool) {
    let config = load_config(&path);
    let registry = default_registry();
    let mut parser = get_parser();

    let mut total_edits = 0usize;
    let mut files_changed = 0usize;

    for entry in WalkDir::new(&path).into_iter().filter_map(|e| e.ok()) {
        if !entry.path().extension().map_or(false, |ext| ext == "py") {
            continue;
        }
        let filepath = entry.path();
        let source = match fs::read_to_string(filepath) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Erro ao ler {}: {}", filepath.display(), e);
                continue;
            }
        };
        let tree = match parse_python_source(&mut parser, &source) {
            Some(t) => t,
            None => continue,
        };
        let ctx = Context {
            source: &source,
            filepath: filepath.to_str().unwrap_or("<unknown>"),
            config: &config,
        };

        // Reúne todos os edits por regra, agrupando os diagnósticos que
        // cada regra produziu.
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
            continue;
        }

        let n_edits = edits.len();
        total_edits += n_edits;
        files_changed += 1;

        if check {
            println!(
                "{}: {} correção(ões) disponível(is)",
                ctx.filepath, n_edits
            );
            continue;
        }

        let novo = apply_edits(&source, edits);
        if dry_run {
            println!("--- {} (dry-run) ---", ctx.filepath);
            println!("{}", novo);
            continue;
        }

        if let Err(e) = fs::write(filepath, &novo) {
            eprintln!("Erro ao escrever {}: {}", filepath.display(), e);
            continue;
        }
        println!("{}: {} correção(ões) aplicada(s)", ctx.filepath, n_edits);
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