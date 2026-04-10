mod graph;
mod layout;
mod parser;
mod render;
mod style;

use std::fs;
use std::io::{self, IsTerminal, Read};
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use render::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ThemeName {
    Grey,
    Mono,
    Neon,
    Dim,
    None,
}

impl ThemeName {
    fn theme(self) -> Theme {
        match self {
            ThemeName::Grey => Theme::grey(),
            ThemeName::Mono => Theme::mono(),
            ThemeName::Neon => Theme::neon(),
            ThemeName::Dim => Theme::dim(),
            ThemeName::None => Theme::plain(),
        }
    }
}

/// Convert a Mermaid `flowchart` into pretty Unicode ASCII.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Input file. Reads from stdin if omitted.
    file: Option<String>,

    /// Horizontal padding inside boxes.
    #[arg(long, default_value_t = 1)]
    padding: usize,

    /// Color theme.
    #[arg(long, value_enum, default_value_t = ThemeName::Grey)]
    theme: ThemeName,

    /// When to emit ANSI color.
    #[arg(long, value_enum, default_value_t = ColorMode::Auto)]
    color: ColorMode,

    /// Disable colors (alias for --color never).
    #[arg(long)]
    no_color: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let source = match cli.file.as_deref() {
        Some(p) => match fs::read_to_string(p) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading {}: {}", p, e);
                return ExitCode::FAILURE;
            }
        },
        None => {
            let mut s = String::new();
            if io::stdin().read_to_string(&mut s).is_err() {
                eprintln!("Error reading stdin");
                return ExitCode::FAILURE;
            }
            s
        }
    };

    let g = match parser::parse(&source) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Parse error: {}", e);
            return ExitCode::FAILURE;
        }
    };
    let laid_out = layout::layout(g, cli.padding);

    let use_color = if cli.no_color {
        false
    } else {
        match cli.color {
            ColorMode::Always => true,
            ColorMode::Never => false,
            ColorMode::Auto => io::stdout().is_terminal(),
        }
    };
    let theme = if use_color {
        cli.theme.theme()
    } else {
        Theme::plain()
    };

    print!("{}", render::render(&laid_out, &theme));
    ExitCode::SUCCESS
}
