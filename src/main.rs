use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use asm_check::render::{self, Format};
use asm_check::{Config, Diagnostic, Severity, lints};
use clap::{Parser, Subcommand, ValueEnum};

/// A semantic linter for x86-64 assembly (NASM / Intel syntax).
#[derive(Parser)]
#[command(version, about, args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Files or directories to check; `-` reads standard input.
    paths: Vec<PathBuf>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = FormatArg::Human)]
    format: FormatArg,

    /// Also run the stricter `pedantic` lints.
    #[arg(long)]
    pedantic: bool,

    /// Silence a lint. May be repeated.
    #[arg(short = 'A', long = "allow", value_name = "LINT")]
    allow: Vec<String>,

    /// Exit with an error when there are warnings.
    #[arg(short = 'D', long)]
    deny_warnings: bool,

    /// Lowest severity to report.
    #[arg(long, value_enum, default_value_t = LevelArg::Hint)]
    level: LevelArg,

    /// When to use colours.
    #[arg(long, value_enum, default_value_t = ColorArg::Auto)]
    color: ColorArg,
}

#[derive(Subcommand)]
enum Command {
    /// List every lint.
    Lints,
}

#[derive(Clone, Copy, ValueEnum)]
enum FormatArg {
    Human,
    Short,
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
enum LevelArg {
    Hint,
    Warning,
    Error,
}

#[derive(Clone, Copy, ValueEnum)]
enum ColorArg {
    Auto,
    Always,
    Never,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Lints) => {
            list_lints();
            ExitCode::SUCCESS
        }
        None => check(&cli),
    }
}

fn list_lints() {
    let width = lints::ALL.iter().map(|lint| lint.name.len()).max().unwrap_or(0);
    for lint in lints::ALL {
        println!("{:width$}  {:11}  {}", lint.name, lint.group.name(), lint.summary);
    }
}

fn check(cli: &Cli) -> ExitCode {
    if cli.paths.is_empty() {
        eprintln!("asm-check: no input files (try `asm-check --help`)");
        return ExitCode::from(2);
    }
    for name in &cli.allow {
        if lints::find(name).is_none() {
            eprintln!("asm-check: unknown lint `{name}` (see `asm-check lints`)");
            return ExitCode::from(2);
        }
    }

    let config = Config { pedantic: cli.pedantic, allow: cli.allow.iter().cloned().collect() };
    let format = match cli.format {
        FormatArg::Human => Format::Human,
        FormatArg::Short => Format::Short,
        FormatArg::Json => Format::Json,
    };
    let level = match cli.level {
        LevelArg::Hint => Severity::Hint,
        LevelArg::Warning => Severity::Warning,
        LevelArg::Error => Severity::Error,
    };
    let color = match cli.color {
        ColorArg::Always => true,
        ColorArg::Never => false,
        ColorArg::Auto => std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none(),
    };

    let mut files = Vec::new();
    for path in &cli.paths {
        if let Err(error) = collect_files(path, &mut files) {
            eprintln!("asm-check: {}: {error}", path.display());
            return ExitCode::from(2);
        }
    }

    let mut all: Vec<Diagnostic> = Vec::new();
    let mut json = Vec::new();
    for file in &files {
        let source = match read(file) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("asm-check: {}: {error}", file.display());
                return ExitCode::from(2);
            }
        };
        let name = file.display().to_string();
        let mut diagnostics = asm_check::check(&source, &config);
        diagnostics.retain(|d| d.severity >= level);
        match format {
            Format::Human => print!("{}", render::human(&name, &source, &diagnostics, color)),
            Format::Short => print!("{}", render::short(&name, &source, &diagnostics)),
            Format::Json => {
                if let serde_json::Value::Array(items) = render::json(&name, &source, &diagnostics)
                {
                    json.extend(items);
                }
            }
        }
        all.extend(diagnostics);
    }

    match format {
        Format::Json => println!("{}", serde_json::Value::Array(json)),
        Format::Human => match render::summary(&all) {
            Some(summary) => eprintln!("asm-check: {summary} in {}", plural(files.len(), "file")),
            None => eprintln!("asm-check: no issues in {}", plural(files.len(), "file")),
        },
        Format::Short => {}
    }

    let failing = if cli.deny_warnings { Severity::Warning } else { Severity::Error };
    if all.iter().any(|d| d.severity >= failing) { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

fn plural(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

fn read(path: &Path) -> std::io::Result<String> {
    if path == Path::new("-") {
        let mut source = String::new();
        std::io::stdin().read_to_string(&mut source)?;
        Ok(source)
    } else {
        // Assembly sources are not always valid UTF-8 (Latin-1 comments).
        Ok(String::from_utf8_lossy(&std::fs::read(path)?).into_owned())
    }
}

fn is_assembly(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "s" | "S" | "asm" | "nasm"))
}

/// Expands directories into the assembly files they contain.
fn collect_files(path: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !path.is_dir() {
        files.push(path.to_path_buf());
        return Ok(());
    }
    let mut entries: Vec<PathBuf> =
        std::fs::read_dir(path)?.filter_map(|entry| entry.ok().map(|entry| entry.path())).collect();
    entries.sort();
    for entry in entries {
        let hidden = entry.file_name().is_some_and(|name| name.to_string_lossy().starts_with('.'));
        if entry.is_dir() && !hidden && entry.file_name() != Some("target".as_ref()) {
            collect_files(&entry, files)?;
        } else if entry.is_file() && is_assembly(&entry) {
            files.push(entry);
        }
    }
    Ok(())
}
