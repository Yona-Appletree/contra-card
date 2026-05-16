use std::{
    env, fs,
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};

const DEFAULT_MEDIA: &str = "3x5";

#[derive(Debug, Eq, PartialEq)]
pub struct PrintOptions {
    pub path: PathBuf,
    pub printer: Option<String>,
    pub media: String,
    pub copies: u32,
    pub landscape: bool,
    pub dry_run: bool,
}

pub fn list_printers() -> Result<()> {
    let status = Command::new("lpstat")
        .args(["-p", "-d"])
        .status()
        .context("could not run lpstat")?;

    if !status.success() {
        bail!("lpstat exited with {status}");
    }

    Ok(())
}

pub fn print_svg(options: &PrintOptions) -> Result<()> {
    if !options.path.exists() {
        bail!("{} does not exist", options.path.display());
    }

    let print_path = print_ready_path(&options.path);
    let needs_conversion = print_path != options.path;
    let args = lp_args(options, &print_path);
    if options.dry_run {
        if needs_conversion {
            println!(
                "{}",
                display_command("sips", &sips_args(&options.path, &print_path))
            );
        }
        println!("{}", display_command("lp", &args));
        return Ok(());
    }

    if needs_conversion {
        convert_svg_to_pdf(&options.path, &print_path)?;
    }

    let status = Command::new("lp")
        .args(&args)
        .stdin(Stdio::null())
        .status()
        .context("could not run lp")?;

    if !status.success() {
        bail!("lp exited with {status}");
    }

    if needs_conversion {
        let _ = fs::remove_file(&print_path);
    }

    Ok(())
}

pub fn parse_print_options(args: &[String]) -> Result<PrintOptions> {
    let mut path = None;
    let mut printer = None;
    let mut media = DEFAULT_MEDIA.to_owned();
    let mut copies = 1;
    let mut landscape = true;
    let mut dry_run = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--printer" | "-d" => {
                i += 1;
                printer = Some(required_value(args, i, "--printer")?.to_owned());
            }
            "--media" | "-m" => {
                i += 1;
                media = required_value(args, i, "--media")?.to_owned();
            }
            "--copies" | "-n" => {
                i += 1;
                copies = required_value(args, i, "--copies")?
                    .parse::<u32>()
                    .context("--copies must be a positive integer")?;
                if copies == 0 {
                    bail!("--copies must be at least 1");
                }
            }
            "--portrait" => {
                landscape = false;
            }
            "--landscape" => {
                landscape = true;
            }
            "--dry-run" => {
                dry_run = true;
            }
            value if value.starts_with('-') => {
                bail!("unknown print option {value:?}");
            }
            value => {
                if path.is_some() {
                    bail!("print accepts one SVG path");
                }
                path = Some(PathBuf::from(value));
            }
        }
        i += 1;
    }

    let Some(path) = path else {
        bail!(
            "usage: contra-card print <svg> [--printer NAME] [--media 3x5] [--copies N] [--dry-run]"
        );
    };

    Ok(PrintOptions {
        path,
        printer,
        media,
        copies,
        landscape,
        dry_run,
    })
}

fn lp_args(options: &PrintOptions, print_path: &std::path::Path) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(printer) = &options.printer {
        args.push("-d".to_owned());
        args.push(printer.to_owned());
    }

    if options.copies != 1 {
        args.push("-n".to_owned());
        args.push(options.copies.to_string());
    }

    args.push("-o".to_owned());
    args.push(format!("media={}", options.media));
    if options.landscape {
        args.push("-o".to_owned());
        args.push("landscape".to_owned());
    }
    args.push("-o".to_owned());
    args.push("fit-to-page".to_owned());
    args.push(print_path.display().to_string());
    args
}

fn print_ready_path(path: &std::path::Path) -> PathBuf {
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
    {
        return path.to_path_buf();
    }

    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("contra-card");
    env::temp_dir().join(format!("{stem}-print-{}.pdf", std::process::id()))
}

fn convert_svg_to_pdf(svg_path: &std::path::Path, pdf_path: &std::path::Path) -> Result<()> {
    let status = Command::new("sips")
        .args(sips_args(svg_path, pdf_path))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .context("could not run sips to convert SVG to PDF")?;

    if !status.success() {
        bail!("sips exited with {status}");
    }

    Ok(())
}

fn sips_args(svg_path: &std::path::Path, pdf_path: &std::path::Path) -> Vec<String> {
    vec![
        "-s".to_owned(),
        "format".to_owned(),
        "pdf".to_owned(),
        svg_path.display().to_string(),
        "--out".to_owned(),
        pdf_path.display().to_string(),
    ]
}

fn required_value<'a>(args: &'a [String], index: usize, option: &str) -> Result<&'a str> {
    args.get(index)
        .map(String::as_str)
        .filter(|value| !value.starts_with('-'))
        .with_context(|| format!("{option} requires a value"))
}

fn display_command(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_owned())
        .chain(args.iter().map(|arg| shell_quote(arg)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "-_./=:@".contains(ch))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_minimal_print_options() {
        assert_eq!(
            parse_print_options(&strings(&["dances/in-the-mood.svg"])).unwrap(),
            PrintOptions {
                path: PathBuf::from("dances/in-the-mood.svg"),
                printer: None,
                media: "3x5".to_owned(),
                copies: 1,
                landscape: true,
                dry_run: false,
            }
        );
    }

    #[test]
    fn parses_custom_print_options() {
        assert_eq!(
            parse_print_options(&strings(&[
                "card.svg",
                "--printer",
                "Office Printer",
                "--media",
                "Custom.3x5in",
                "--copies",
                "2",
                "--portrait",
                "--dry-run",
            ]))
            .unwrap(),
            PrintOptions {
                path: PathBuf::from("card.svg"),
                printer: Some("Office Printer".to_owned()),
                media: "Custom.3x5in".to_owned(),
                copies: 2,
                landscape: false,
                dry_run: true,
            }
        );
    }

    #[test]
    fn builds_lp_args() {
        let options = PrintOptions {
            path: PathBuf::from("card.svg"),
            printer: Some("Office Printer".to_owned()),
            media: "3x5".to_owned(),
            copies: 2,
            landscape: true,
            dry_run: false,
        };

        assert_eq!(
            lp_args(&options, &PathBuf::from("card.pdf")),
            strings(&[
                "-d",
                "Office Printer",
                "-n",
                "2",
                "-o",
                "media=3x5",
                "-o",
                "landscape",
                "-o",
                "fit-to-page",
                "card.pdf",
            ])
        );
    }

    #[test]
    fn non_pdf_paths_print_through_temp_pdf() {
        assert!(
            print_ready_path(&PathBuf::from("card.svg"))
                .ends_with(format!("card-print-{}.pdf", std::process::id()))
        );
        assert_eq!(
            print_ready_path(&PathBuf::from("card.pdf")),
            PathBuf::from("card.pdf")
        );
    }
}
