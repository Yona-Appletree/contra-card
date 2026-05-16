use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use html_escape::{decode_html_entities, encode_double_quoted_attribute, encode_text};
use regex::Regex;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, PRAGMA};
use serde::Deserialize;

mod highlight;
mod print;
mod wrap;

const BASE_URL: &str = "https://www.ibiblio.org/contradance/thecallersbox";
const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) contra-card/0.1";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Dance {
    #[serde(rename = "ID")]
    id: String,
    name: String,
    authors: Vec<String>,
    formation_base: String,
    formation_detail: String,
    progression: String,
    direction: String,
    #[serde(rename = "phrases")]
    phrases: Vec<Phrase>,
    calling_notes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Phrase {
    name: String,
    figures: Vec<String>,
}

#[derive(Debug)]
struct SearchResult {
    id: String,
    name: String,
    author: String,
    formation: String,
}

#[derive(Debug)]
struct CardLayout {
    phrase_x: f32,
    beats_x: f32,
    figure_x: f32,
    left_rule_x: f32,
    notes_x: f32,
    continuation_x: f32,
    body_start: f32,
    row_step: f32,
    phrase_gap: f32,
    phrase_font_size: f32,
    beats_font_size: f32,
    figure_font_size: f32,
    notes_font_size: f32,
    max_figure_chars: usize,
    max_note_chars: usize,
}

#[derive(Debug, Default)]
struct SvgCardInfo {
    title: Option<String>,
    authors: Option<String>,
    formation: Option<String>,
    source_id: Option<String>,
    source_url: Option<String>,
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        None => interactive_loop(),
        Some("add") => add_command(&args[1..]),
        Some("regen") => regen_command(&args[1..]),
        Some("print") => print_command(&args[1..]),
        Some("printers") => print::list_printers(),
        Some("help" | "-h" | "--help") => {
            print_help();
            Ok(())
        }
        Some(command) => bail!("unknown command {command:?}; try `cargo run -- help`"),
    }
}

fn interactive_loop() -> Result<()> {
    println!("Contra card maker");
    println!(
        "Search The Caller's Box by dance name, paste a dance URL/SVG path, or type q to quit.\n"
    );

    loop {
        let input = prompt("Dance name or URL: ")?;
        if input.eq_ignore_ascii_case("q") || input.eq_ignore_ascii_case("quit") {
            return Ok(());
        }
        if input.trim().is_empty() {
            continue;
        }

        if let Some(path) = resolve_existing_svg(&input) {
            handle_existing_card(path)?;
            println!();
            continue;
        }

        let dance = match dance_from_input(&input) {
            Ok(dance) => dance,
            Err(err) => {
                eprintln!("Could not load dance: {err:#}\n");
                continue;
            }
        };

        if confirm_dance(&dance)? {
            let path = write_svg(&dance)?;
            after_writing_card(path)?;
            println!();
            continue;
        }

        println!("No worries. Back to search.\n");
    }
}

fn add_command(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return interactive_loop();
    }

    let input = args.join(" ");
    let dance = dance_from_input(&input)?;
    if confirm_dance(&dance)? {
        let path = write_svg(&dance)?;
        after_writing_card(path)?;
    }
    Ok(())
}

fn handle_existing_card(path: PathBuf) -> Result<()> {
    let info = svg_card_info(&path)?;
    println!("\n{}", existing_card_summary(&path, &info));
    if confirm_yes_default("Print this card? [Y/n]: ")? {
        let options = print::PrintOptions::default_for_path(path);
        print::print_svg(&options)?;
    }
    Ok(())
}

fn regen_command(args: &[String]) -> Result<()> {
    let Some(path_arg) = args.first() else {
        bail!("usage: contra-card regen <path-to-svg> [--yes]");
    };
    let yes = args.iter().any(|arg| arg == "--yes" || arg == "-y");
    let path = PathBuf::from(path_arg);
    let source_id = source_id_from_svg(&path)?;
    let dance = fetch_dance(&source_id)?;

    println!("\n{}", preview_text(&dance));
    if !yes && !confirm_overwrite(&path)? {
        println!("Skipped {}", path.display());
        return Ok(());
    }

    write_svg_to_path(&dance, &path)?;
    println!("Regenerated {}", path.display());
    Ok(())
}

fn print_command(args: &[String]) -> Result<()> {
    let options = print::parse_print_options(args)?;
    print::print_svg(&options)
}

fn after_writing_card(path: PathBuf) -> Result<()> {
    println!("Wrote {}", path.display());
    if confirm_yes_default("Print now? [Y/n]: ")? {
        let options = print::PrintOptions::default_for_path(path);
        print::print_svg(&options)?;
    }
    Ok(())
}

fn print_help() {
    println!(
        r#"Contra card maker

Usage:
  contra-card                 Interactive add flow
  contra-card add [QUERY]     Search/fetch and write dances/<dance-name>.svg
  contra-card regen <SVG>     Re-fetch using embedded SVG source metadata
  contra-card printers        List configured CUPS printers
  contra-card print <SVG>     Print an SVG card using media=3x5

Notes:
  QUERY can be a dance title, Caller’s Box URL, or raw Caller’s Box ID.
  The interactive prompt also accepts existing SVG paths or filenames from dances/.
  Interactive yes/no prompts default to yes, so Enter proceeds.
  Print defaults to your custom paper size named 3x5 and landscape orientation.
  Use `contra-card print <SVG> --dry-run` to inspect the lp command.
  `cargo run --` can be used in front of these commands during development.
"#
    );
}

fn dance_from_input(input: &str) -> Result<Dance> {
    if let Some(id) = id_from_input(input) {
        return fetch_dance(&id);
    }

    let candidates = search_by_title(input)?;
    if candidates.is_empty() {
        bail!("No matches for {input:?}");
    }

    let shown = candidates.len().min(25);
    println!("\nMatches:");
    for (i, candidate) in candidates.iter().take(shown).enumerate() {
        println!(
            "{:>2}. {} — {} ({})",
            i + 1,
            candidate.name,
            candidate.author,
            candidate.formation
        );
    }
    if candidates.len() > shown {
        println!(
            "    ... {} more matches; try a more specific title if yours is not shown.",
            candidates.len() - shown
        );
    }
    println!(" b. back");

    loop {
        let choice = prompt("Choose a dance: ")?;
        if choice.eq_ignore_ascii_case("b") {
            bail!("selection cancelled");
        }

        let Ok(index) = choice.parse::<usize>() else {
            eprintln!("Enter a number or b.");
            continue;
        };
        let Some(candidate) = candidates.iter().take(shown).nth(index.saturating_sub(1)) else {
            eprintln!("Choose 1-{shown}.");
            continue;
        };

        return fetch_dance(&candidate.id);
    }
}

fn id_from_input(input: &str) -> Option<String> {
    let id_re = Regex::new(r"(?:\bid=|^)(\d+)\b").ok()?;
    id_re
        .captures(input)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_owned())
}

fn search_by_title(title: &str) -> Result<Vec<SearchResult>> {
    let url = format!(
        "{BASE_URL}/index.php?title={}",
        urlencoding::encode(title.trim())
    );

    // The Caller's Box appears to expose search as HTML from index.php, not JSON.
    // Once we scrape a dance ID from this page, dance.php?id=...&format=JSON
    // gives us the structured dance details.
    let html = http_client()?
        .get(url)
        .header(
            ACCEPT,
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .header(ACCEPT_LANGUAGE, "en-US,en;q=0.6")
        .header(CACHE_CONTROL, "no-cache")
        .header(PRAGMA, "no-cache")
        .send()
        .context("search request failed")?
        .error_for_status()
        .context("search returned an error")?
        .text()
        .context("could not read search response")?;

    let row_re = Regex::new(
        r#"(?s)<tr>.*?dance\.php\?id=(\d+).*?>(.*?)</a></td>\s*<td>(.*?)</td>\s*<td>(.*?)</td>.*?</tr>"#,
    )?;

    Ok(row_re
        .captures_iter(&html)
        .map(|caps| SearchResult {
            id: caps[1].to_owned(),
            name: clean_html(&caps[2]),
            author: clean_html(&caps[3]),
            formation: clean_html(&caps[4]),
        })
        .collect())
}

fn fetch_dance(id: &str) -> Result<Dance> {
    let url = format!("{BASE_URL}/dance.php?id={id}&format=JSON");
    http_client()?
        .get(url)
        .send()
        .context("dance request failed")?
        .error_for_status()
        .context("dance request returned an error")?
        .json::<Dance>()
        .context("could not parse dance JSON")
}

fn http_client() -> Result<Client> {
    Client::builder()
        .user_agent(BROWSER_USER_AGENT)
        .build()
        .context("could not build HTTP client")
}

fn confirm_dance(dance: &Dance) -> Result<bool> {
    println!("\n{}", preview_text(dance));

    loop {
        let answer = prompt("Create SVG for this dance? [Y/n/b]: ")?;
        match answer.to_lowercase().as_str() {
            "" | "y" | "yes" => return Ok(true),
            "n" | "no" | "b" | "back" => return Ok(false),
            _ => eprintln!("Enter y or b."),
        }
    }
}

fn preview_text(dance: &Dance) -> String {
    let mut out = String::new();
    out.push_str(&format!("{} — {}\n", dance.name, dance.authors.join(", ")));
    out.push_str(&format!("{}\n", dance_meta(dance)));
    out.push_str("----------------------------------------\n");

    for phrase in &dance.phrases {
        for (i, figure) in phrase.figures.iter().enumerate() {
            let (beats, text) = split_beats(figure);
            let role = if i == 0 { phrase.name.as_str() } else { "" };
            out.push_str(&format!(
                "{:<3} {:>2}  {}\n",
                role,
                beats.unwrap_or_default(),
                neutralize_terms(text)
            ));
        }
    }

    if !dance.calling_notes.is_empty() {
        out.push_str("\nNotes:\n");
        for note in &dance.calling_notes {
            out.push_str(&format!("- {}\n", neutralize_terms(note)));
        }
    }

    out
}

fn write_svg(dance: &Dance) -> Result<PathBuf> {
    fs::create_dir_all("dances").context("could not create dances directory")?;

    let filename = format!("{}.svg", slugify(&dance.name));
    let path = PathBuf::from("dances").join(filename);
    write_svg_to_path(dance, &path)?;

    Ok(path)
}

fn write_svg_to_path(dance: &Dance, path: &Path) -> Result<()> {
    let svg = render_svg(dance);
    fs::write(path, svg).with_context(|| format!("could not write {}", path.display()))
}

fn render_svg(dance: &Dance) -> String {
    let title = encode_text(&dance.name);
    let authors_text = format!("By {}", dance.authors.join(", "));
    let authors = encode_text(&authors_text);
    let meta_text = dance_meta(dance);
    let meta = render_header_meta(&meta_text);
    let source_url = format!("{BASE_URL}/dance.php?id={}", dance.id);
    let source = encode_double_quoted_attribute(&source_url);
    let source_id = encode_double_quoted_attribute(&dance.id);
    let source_json_url = format!("{source_url}&format=JSON");
    let source_json = encode_double_quoted_attribute(&source_json_url);
    let metadata_name = encode_double_quoted_attribute(&dance.name);
    let metadata_authors_text = dance.authors.join(", ");
    let metadata_authors = encode_double_quoted_attribute(&metadata_authors_text);
    let metadata_formation = encode_double_quoted_attribute(&meta_text);
    let layout = card_layout(dance);

    let mut rows = String::new();
    let mut phrase_rules = String::new();
    let mut y = layout.body_start;
    for (phrase_index, phrase) in dance.phrases.iter().enumerate() {
        let phrase_start = y;
        let phrase_rows = phrase_visual_rows(phrase, &layout);
        let phrase_label_y = phrase_start + ((phrase_rows - 1) as f32 * layout.row_step / 2.0);
        rows.push_str(&format!(
            r#"<text x="{phrase_x:.1}" y="{phrase_label_y:.1}" class="phrase">{}</text>
"#,
            encode_text(&phrase.name),
            phrase_x = layout.phrase_x,
        ));

        for figure in &phrase.figures {
            let (beats, text) = split_beats(figure);
            let wrapped_lines = wrap::wrap_text(&neutralize_terms(text), layout.max_figure_chars);
            for (line_index, line) in wrapped_lines.iter().enumerate() {
                let beat_text = if line_index == 0 {
                    encode_text(&beats.clone().unwrap_or_default()).to_string()
                } else {
                    String::new()
                };
                let figure_x = if line.indent {
                    layout.continuation_x
                } else {
                    layout.figure_x
                };
                let figure_spans = render_figure_spans(&line.text);
                rows.push_str(&format!(
                    r#"<text x="{beats_x:.1}" y="{y:.1}" class="beats">{beat_text}</text>
<text x="{figure_x:.1}" y="{y:.1}" class="figure" xml:space="preserve">{figure_spans}</text>
"#,
                    beats_x = layout.beats_x,
                ));
                y += layout.row_step;
            }
        }
        if phrase_index + 1 < dance.phrases.len() {
            let rule_y = y - (layout.phrase_gap / 2.0) - 4.0;
            phrase_rules.push_str(&format!(
                r#"<path class="phrase-rule" d="M{left_rule_x:.1} {rule_y:.1} H484"/>
"#,
                left_rule_x = layout.left_rule_x,
            ));
            y += layout.phrase_gap;
        }
    }

    let mut notes = String::new();
    if !dance.calling_notes.is_empty() {
        notes.push_str(&format!(
            r#"<text x="{notes_x:.1}" y="{y:.1}" class="notes-label">Notes</text>"#,
            notes_x = layout.notes_x,
        ));
        y += layout.row_step;
        for note in &dance.calling_notes {
            if note.trim().is_empty() {
                continue;
            }
            for line in wrap::wrap_text(&neutralize_terms(note), layout.max_note_chars) {
                let note_x = if line.indent {
                    layout.notes_x + 12.0
                } else {
                    layout.notes_x
                };
                notes.push_str(&format!(
                    r#"<text x="{note_x:.1}" y="{y:.1}" class="notes">{}</text>"#,
                    encode_text(&line.text),
                ));
                y += layout.row_step;
            }
        }
    }

    format!(
        r##"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="5in" height="3in" viewBox="0 0 500 300">
  <title>{title}</title>
  <desc>Contra dance calling card generated from {source}</desc>
  <metadata>
    <contra-card xmlns="https://github.com/yona/contra-card"
      version="0.1"
      dance-name="{metadata_name}"
      authors="{metadata_authors}"
      formation="{metadata_formation}"
      source="the-callers-box"
      source-id="{source_id}"
      source-url="{source}"
      source-json-url="{source_json}" />
  </metadata>
  <style>
    .paper {{ fill: #fffdf5; }}
    .phrase-rule {{ stroke: #7aa0b8; stroke-width: 1.2; opacity: 0.8; }}
    .top-rule {{ stroke: #b64545; stroke-width: 2; }}
    text {{ fill: #1d2528; font-family: "Avenir Next", Arial, sans-serif; }}
    .title {{ font-size: 26px; font-weight: 700; }}
    .authors {{ font-size: 14px; }}
    .meta {{ font-size: 12px; text-anchor: end; }}
    .phrase {{ font-size: {phrase_font_size:.1}px; font-weight: 700; dominant-baseline: middle; }}
    .beats {{ font-size: {beats_font_size:.1}px; font-weight: 700; text-anchor: end; }}
    .figure {{ font-size: {figure_font_size:.1}px; }}
    .meta-strong {{ font-weight: 700; }}
    .move {{ fill: #7a2e2e; font-weight: 600; }}
    .role {{ font-weight: 600; }}
    .amount {{ fill: #4d6470; font-weight: 600; }}
    .notes-label {{ font-size: {notes_font_size:.1}px; font-weight: 700; }}
    .notes {{ font-size: {notes_font_size:.1}px; }}
  </style>
  <rect class="paper" x="0" y="0" width="500" height="300"/>
  <g transform="translate(4 8)">
    <path class="top-rule" d="M0 48 H500"/>
    {phrase_rules}
    <text x="16" y="36" class="title">{title}</text>
    <text x="472" y="22" class="meta">{meta}</text>
    <text x="472" y="40" class="meta">{authors}</text>
    {rows}
    {notes}
  </g>
</svg>
"##,
        phrase_font_size = layout.phrase_font_size,
        beats_font_size = layout.beats_font_size,
        figure_font_size = layout.figure_font_size,
        notes_font_size = layout.notes_font_size,
    )
}

fn render_figure_spans(text: &str) -> String {
    highlight::highlight(text)
        .into_iter()
        .map(|span| {
            format!(
                r#"<tspan class="{}">{}</tspan>"#,
                span.kind.class_name(),
                encode_text(span.text)
            )
        })
        .collect()
}

fn render_header_meta(text: &str) -> String {
    text.split("Becket")
        .enumerate()
        .map(|(i, part)| {
            let mut out = String::new();
            if i > 0 {
                out.push_str(r#"<tspan class="meta-strong">Becket</tspan>"#);
            }
            out.push_str(&encode_text(part));
            out
        })
        .collect()
}

fn card_layout(dance: &Dance) -> CardLayout {
    let base_figure_font_size = 17.0;
    let figure_x = 104.0;
    let right_edge = 484.0;
    let notes_x = 24.0;
    let max_figure_chars = max_chars_for_width(right_edge - figure_x, base_figure_font_size);
    let max_note_chars = max_chars_for_width(right_edge - notes_x, 13.0);
    let figure_rows = figure_visual_rows(dance, max_figure_chars);
    let phrase_gaps = dance.phrases.len().saturating_sub(1);
    let note_rows = if dance.calling_notes.is_empty() {
        0
    } else {
        1 + dance
            .calling_notes
            .iter()
            .filter(|note| !note.trim().is_empty())
            .map(|note| wrap::wrap_text(&neutralize_terms(note), max_note_chars).len())
            .sum::<usize>()
    };

    let body_start = 66.0;
    let body_max_baseline = 268.0;
    let default_row_step = 21.0;
    let default_phrase_gap = 10.0;
    let content_rows = figure_rows + note_rows;
    let default_height = content_height(
        content_rows,
        phrase_gaps,
        default_row_step,
        default_phrase_gap,
    );
    let available_height = body_max_baseline - body_start;
    let scale = if default_height <= available_height {
        1.0
    } else {
        available_height / default_height
    };

    CardLayout {
        phrase_x: 16.0,
        beats_x: 68.0,
        figure_x,
        left_rule_x: 16.0,
        notes_x,
        continuation_x: figure_x + 16.0,
        body_start,
        row_step: default_row_step * scale,
        phrase_gap: default_phrase_gap * scale,
        phrase_font_size: 18.0 * scale,
        beats_font_size: 13.0 * scale,
        figure_font_size: base_figure_font_size * scale,
        notes_font_size: 13.0 * scale,
        max_figure_chars,
        max_note_chars,
    }
}

fn figure_visual_rows(dance: &Dance, max_chars: usize) -> usize {
    dance
        .phrases
        .iter()
        .map(|phrase| {
            phrase
                .figures
                .iter()
                .map(|figure| {
                    let (_, text) = split_beats(figure);
                    wrap::wrap_text(&neutralize_terms(text), max_chars).len()
                })
                .sum::<usize>()
                .max(1)
        })
        .sum()
}

fn phrase_visual_rows(phrase: &Phrase, layout: &CardLayout) -> usize {
    phrase
        .figures
        .iter()
        .map(|figure| {
            let (_, text) = split_beats(figure);
            wrap::wrap_text(&neutralize_terms(text), layout.max_figure_chars).len()
        })
        .sum::<usize>()
        .max(1)
}

fn max_chars_for_width(width: f32, font_size: f32) -> usize {
    (width / (font_size * 0.47)).floor().max(16.0) as usize
}

fn content_height(content_rows: usize, phrase_gaps: usize, row_step: f32, phrase_gap: f32) -> f32 {
    if content_rows == 0 {
        0.0
    } else {
        (content_rows - 1) as f32 * row_step + phrase_gaps as f32 * phrase_gap
    }
}

fn dance_meta(dance: &Dance) -> String {
    let mut parts = vec![dance.formation_base.clone()];
    if !dance.formation_detail.trim().is_empty() {
        parts.push(dance.formation_detail.clone());
    }
    if !dance.progression.trim().is_empty() {
        parts.push(format!("{} progression", dance.progression));
    }
    if !dance.direction.trim().is_empty() {
        parts.push(dance.direction.clone());
    }
    parts.join(" | ")
}

fn split_beats(figure: &str) -> (Option<String>, &str) {
    let Some(rest) = figure.strip_prefix('(') else {
        return (None, figure);
    };
    let Some((beats, text)) = rest.split_once(')') else {
        return (None, figure);
    };
    (Some(beats.trim().to_owned()), text.trim())
}

fn neutralize_terms(text: &str) -> String {
    let replacements = [
        (r"\bmen\b", "Larks"),
        (r"\bman\b", "Lark"),
        (r"\bgents\b", "Larks"),
        (r"\bgentlemen\b", "Larks"),
        (r"\bladies\b", "Robins"),
        (r"\blady\b", "Robin"),
        (r"\bwomen\b", "Robins"),
        (r"\bwoman\b", "Robin"),
    ];

    let mut out = text.to_owned();
    for (pattern, replacement) in replacements {
        let re = Regex::new(&format!("(?i){pattern}")).expect("valid replacement regex");
        out = re.replace_all(&out, replacement).into_owned();
    }
    out
}

fn clean_html(input: &str) -> String {
    let tag_re = Regex::new(r"<[^>]*>").expect("valid tag regex");
    let without_tags = tag_re.replace_all(input, "");
    decode_html_entities(without_tags.trim()).into_owned()
}

fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;

    for ch in name.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }

    slug.trim_matches('-').to_owned()
}

fn source_id_from_svg(path: &Path) -> Result<String> {
    let info = svg_card_info(path)?;
    if let Some(source_id) = info.source_id {
        return Ok(source_id);
    }

    let svg =
        fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))?;
    id_from_input(&svg).with_context(|| {
        format!(
            "could not find Caller’s Box source metadata in {}",
            path.display()
        )
    })
}

fn resolve_existing_svg(input: &str) -> Option<PathBuf> {
    let cleaned = input.trim().trim_matches('"').trim_matches('\'');
    if cleaned.is_empty() {
        return None;
    }

    let candidate = PathBuf::from(cleaned);
    let candidates = if candidate.extension().is_some() {
        vec![candidate.clone(), PathBuf::from("dances").join(&candidate)]
    } else {
        vec![
            candidate.clone(),
            candidate.with_extension("svg"),
            PathBuf::from("dances").join(&candidate),
            PathBuf::from("dances")
                .join(&candidate)
                .with_extension("svg"),
        ]
    };

    candidates
        .into_iter()
        .find(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "svg"))
}

fn svg_card_info(path: &Path) -> Result<SvgCardInfo> {
    let svg =
        fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))?;
    Ok(SvgCardInfo {
        title: svg_title(&svg),
        authors: svg_attr(&svg, "authors"),
        formation: svg_attr(&svg, "formation"),
        source_id: svg_attr(&svg, "source-id"),
        source_url: svg_attr(&svg, "source-url"),
    })
}

fn existing_card_summary(path: &Path, info: &SvgCardInfo) -> String {
    let mut lines = vec![format!("Existing card: {}", path.display())];
    if let Some(title) = &info.title {
        lines.push(format!("Dance: {title}"));
    }
    if let Some(authors) = &info.authors {
        lines.push(format!("By: {authors}"));
    }
    if let Some(formation) = &info.formation {
        lines.push(format!("Formation: {formation}"));
    }
    if let Some(source_id) = &info.source_id {
        lines.push(format!("Caller’s Box ID: {source_id}"));
    }
    if let Some(source_url) = &info.source_url {
        lines.push(format!("Source: {source_url}"));
    }
    lines.join("\n")
}

fn svg_title(svg: &str) -> Option<String> {
    let title_re = Regex::new(r#"(?s)<title>(.*?)</title>"#).ok()?;
    title_re
        .captures(svg)
        .and_then(|caps| caps.get(1))
        .map(|m| decode_html_entities(m.as_str().trim()).into_owned())
}

fn svg_attr(svg: &str, attr: &str) -> Option<String> {
    let attr_re = Regex::new(&format!(r#"{attr}="([^"]*)""#)).ok()?;
    attr_re
        .captures(svg)
        .and_then(|caps| caps.get(1))
        .map(|m| decode_html_entities(m.as_str()).into_owned())
}

fn confirm_overwrite(path: &Path) -> Result<bool> {
    confirm_yes_default(&format!("Overwrite {}? [Y/n]: ", path.display()))
}

fn confirm_yes_default(label: &str) -> Result<bool> {
    loop {
        let answer = prompt(label)?;
        match answer.to_lowercase().as_str() {
            "" | "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => eprintln!("Enter y or n."),
        }
    }
}

fn prompt(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush().context("could not flush stdout")?;

    let mut input = String::new();
    let bytes = io::stdin()
        .read_line(&mut input)
        .context("could not read input")?;
    if bytes == 0 {
        bail!("input closed");
    }
    Ok(input.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_id_from_url() {
        assert_eq!(
            id_from_input("https://example.test/dance.php?id=14486&format=JSON"),
            Some("14486".to_owned())
        );
    }

    #[test]
    fn splits_beats() {
        assert_eq!(
            split_beats("(8) Right and left through"),
            (Some("8".to_owned()), "Right and left through")
        );
    }

    #[test]
    fn neutralizes_role_terms() {
        assert_eq!(
            neutralize_terms("Men pass left; ladies chain; mad robin"),
            "Larks pass left; Robins chain; mad robin"
        );
    }

    #[test]
    fn slugs_names() {
        assert_eq!(slugify("In the Mood!"), "in-the-mood");
    }

    #[test]
    fn reads_svg_title_and_attrs() {
        let svg = r#"<svg><title>Air &amp; Pants</title><metadata><contra-card authors="Lisa Greenleaf" formation="Duple Minor - Becket" source-id="123" /></metadata></svg>"#;
        assert_eq!(svg_title(svg), Some("Air & Pants".to_owned()));
        assert_eq!(svg_attr(svg, "authors"), Some("Lisa Greenleaf".to_owned()));
        assert_eq!(
            svg_attr(svg, "formation"),
            Some("Duple Minor - Becket".to_owned())
        );
        assert_eq!(svg_attr(svg, "source-id"), Some("123".to_owned()));
    }
}
