mod graph;
mod layout;
mod parser;
mod render;

use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read};
use std::process::ExitCode;

use render::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorMode {
    Auto,
    Always,
    Never,
}

fn print_help() {
    println!("Usage: mascii [OPTIONS] [FILE]");
    println!();
    println!("Convert a Mermaid `flowchart TD` diagram to pretty Unicode ASCII.");
    println!("Reads from FILE if given, otherwise from stdin.");
    println!();
    println!("Options:");
    println!("  --width N       max output width (currently advisory, default: 80)");
    println!("  --padding N     horizontal padding inside boxes (default: 1)");
    println!("  --theme NAME    color theme: grey (default), mono, neon, dim, none");
    println!("  --color WHEN    when to color: auto (default), always, never");
    println!("  --no-color      equivalent to --color never");
    println!("  -h, --help      show this help");
    println!();
    println!("Themes:");
    println!("  grey   — grey borders, edges, arrows; default labels");
    println!("  mono   — grey borders/edges, bright arrows, yellow crossings");
    println!("  neon   — bright cyan/magenta/yellow palette");
    println!("  dim    — dim everything");
    println!("  none   — no ANSI colors");
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut padding: usize = 1;
    let mut _width: usize = 80;
    let mut input_path: Option<String> = None;
    let mut theme_name: String = "grey".to_string();
    let mut color_mode: ColorMode = ColorMode::Auto;

    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "-h" | "--help" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            "--width" => {
                i += 1;
                let v = args.get(i).and_then(|s| s.parse().ok());
                match v {
                    Some(n) => _width = n,
                    None => {
                        eprintln!("--width requires a positive integer");
                        return ExitCode::FAILURE;
                    }
                }
            }
            "--padding" => {
                i += 1;
                let v = args.get(i).and_then(|s| s.parse().ok());
                match v {
                    Some(n) => padding = n,
                    None => {
                        eprintln!("--padding requires a non-negative integer");
                        return ExitCode::FAILURE;
                    }
                }
            }
            "--theme" => {
                i += 1;
                match args.get(i) {
                    Some(s) => theme_name = s.clone(),
                    None => {
                        eprintln!("--theme requires a name");
                        return ExitCode::FAILURE;
                    }
                }
            }
            "--color" => {
                i += 1;
                match args.get(i).map(|s| s.as_str()) {
                    Some("auto") => color_mode = ColorMode::Auto,
                    Some("always") => color_mode = ColorMode::Always,
                    Some("never") => color_mode = ColorMode::Never,
                    _ => {
                        eprintln!("--color must be one of: auto, always, never");
                        return ExitCode::FAILURE;
                    }
                }
            }
            "--no-color" => {
                color_mode = ColorMode::Never;
            }
            s if !s.starts_with('-') && input_path.is_none() => {
                input_path = Some(s.to_string());
            }
            _ => {
                eprintln!("Unknown argument: {}", a);
                return ExitCode::FAILURE;
            }
        }
        i += 1;
    }

    let source = match input_path {
        Some(p) => match fs::read_to_string(&p) {
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

    let laid_out = layout::layout(g, padding);

    let use_color = match color_mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => io::stdout().is_terminal(),
    };
    let theme = if use_color {
        match Theme::by_name(&theme_name) {
            Some(t) => t,
            None => {
                eprintln!(
                    "Unknown theme '{}'. Known: grey, mono, neon, dim, none",
                    theme_name
                );
                return ExitCode::FAILURE;
            }
        }
    } else {
        Theme::plain()
    };

    let output = render::render(&laid_out, &theme);
    print!("{}", output);
    ExitCode::SUCCESS
}
