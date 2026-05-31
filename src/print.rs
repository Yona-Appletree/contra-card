use std::{
    env, fs,
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, anyhow, bail};

const DEFAULT_MEDIA: &str = "3x5";

#[derive(Debug, Eq, PartialEq)]
pub struct PrintOptions {
    pub printer: Option<String>,
    pub media: String,
    pub copies: u32,
    pub landscape: bool,
    pub dry_run: bool,
}

impl PrintOptions {
    pub fn default_for_path(_path: PathBuf) -> Self {
        Self {
            printer: None,
            media: DEFAULT_MEDIA.to_owned(),
            copies: 1,
            landscape: true,
            dry_run: false,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct PrintJob {
    pub paths: Vec<PathBuf>,
    pub options: PrintOptions,
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

pub fn print_paths(job: &PrintJob) -> Result<()> {
    for path in &job.paths {
        print_path(&job.options, path)?;
    }
    Ok(())
}

pub fn print_path_with_options(options: &PrintOptions, path: &std::path::Path) -> Result<()> {
    print_path(options, path)
}

fn print_path(options: &PrintOptions, path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        bail!("{} does not exist", path.display());
    }

    let print_path = print_ready_path(path);
    let needs_conversion = print_path != path;
    let args = lp_args(options, &print_path);
    if options.dry_run {
        if needs_conversion {
            println!("convert {} -> {}", path.display(), print_path.display());
        }
        println!("{}", display_command("lp", &args));
        return Ok(());
    }

    if needs_conversion {
        convert_svg_to_pdf(path, &print_path)?;
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

pub fn parse_print_job(args: &[String]) -> Result<PrintJob> {
    let mut paths = Vec::new();
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
                paths.push(PathBuf::from(value));
            }
        }
        i += 1;
    }

    if paths.is_empty() {
        bail!(
            "usage: contra-card print <svg> [<svg> ...] [--printer NAME] [--media 3x5] [--copies N] [--dry-run]"
        );
    }

    Ok(PrintJob {
        paths,
        options: PrintOptions {
            printer,
            media,
            copies,
            landscape,
            dry_run,
        },
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
    let svg = fs::read_to_string(svg_path)
        .with_context(|| format!("could not read {}", svg_path.display()))?;
    let mut options = svg2pdf::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = svg2pdf::usvg::Tree::from_str(&svg, &options)
        .map_err(|err| anyhow!("could not parse SVG for PDF conversion: {err}"))?;
    let pdf = svg2pdf::to_pdf(
        &tree,
        svg2pdf::ConversionOptions::default(),
        svg2pdf::PageOptions::default(),
    )
    .map_err(|err| anyhow!("could not convert SVG to PDF: {err}"))?;

    fs::write(pdf_path, pdf).with_context(|| format!("could not write {}", pdf_path.display()))
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
            parse_print_job(&strings(&["dances/in-the-mood.svg"])).unwrap(),
            PrintJob {
                paths: vec![PathBuf::from("dances/in-the-mood.svg")],
                options: PrintOptions {
                    printer: None,
                    media: "3x5".to_owned(),
                    copies: 1,
                    landscape: true,
                    dry_run: false,
                },
            }
        );
    }

    #[test]
    fn parses_custom_print_options() {
        assert_eq!(
            parse_print_job(&strings(&[
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
            PrintJob {
                paths: vec![PathBuf::from("card.svg")],
                options: PrintOptions {
                    printer: Some("Office Printer".to_owned()),
                    media: "Custom.3x5in".to_owned(),
                    copies: 2,
                    landscape: false,
                    dry_run: true,
                },
            }
        );
    }

    #[test]
    fn parses_multiple_print_paths() {
        assert_eq!(
            parse_print_job(&strings(&["a.svg", "b.svg", "--dry-run"]))
                .unwrap()
                .paths,
            vec![PathBuf::from("a.svg"), PathBuf::from("b.svg")]
        );
    }

    #[test]
    fn builds_lp_args() {
        let options = PrintOptions {
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
