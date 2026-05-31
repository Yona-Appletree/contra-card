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
mod moves;
mod print;
mod wrap;

const BASE_URL: &str = "https://www.ibiblio.org/contradance/thecallersbox";
const CONTRADB_BASE_URL: &str = "https://contradb.com";
const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) contra-card/0.1";

#[derive(Debug)]
struct CardDance {
    id: String,
    title: String,
    authors: Vec<String>,
    formation: String,
    source: DanceSource,
    source_url: String,
    source_json_url: Option<String>,
    phrases: Vec<CardPhrase>,
    notes: Vec<String>,
}

#[derive(Debug)]
struct CardPhrase {
    name: String,
    figures: Vec<CardFigure>,
}

#[derive(Debug)]
struct CardFigure {
    beats: Option<String>,
    text: String,
}

#[derive(Clone, Copy, Debug)]
enum DanceSource {
    CallersBox,
    ContraDb,
}

impl std::str::FromStr for DanceSource {
    type Err = ();

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "the-callers-box" => Ok(DanceSource::CallersBox),
            "contradb" => Ok(DanceSource::ContraDb),
            _ => Err(()),
        }
    }
}

impl DanceSource {
    fn label(self) -> &'static str {
        match self {
            DanceSource::CallersBox => "Caller’s Box",
            DanceSource::ContraDb => "ContraDB",
        }
    }

    fn metadata_value(self) -> &'static str {
        match self {
            DanceSource::CallersBox => "the-callers-box",
            DanceSource::ContraDb => "contradb",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CallersBoxDance {
    #[serde(rename = "ID")]
    id: String,
    name: String,
    authors: Vec<String>,
    formation_base: String,
    formation_detail: String,
    progression: String,
    direction: String,
    #[serde(rename = "phrases")]
    phrases: Vec<CallersBoxPhrase>,
    calling_notes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CallersBoxPhrase {
    name: String,
    figures: Vec<String>,
}

#[derive(Debug)]
struct DanceCandidate {
    source: DanceSource,
    id: String,
    name: String,
    author: String,
    formation: String,
}

#[derive(Debug, Deserialize)]
struct ContraDbSearchResponse {
    dances: Vec<ContraDbSearchDance>,
}

#[derive(Debug, Deserialize)]
struct ContraDbSearchDance {
    id: u64,
    title: String,
    choreographer_name: String,
    formation: String,
}

impl From<CallersBoxDance> for CardDance {
    fn from(dance: CallersBoxDance) -> Self {
        let id = dance.id.clone();
        let source_url = format!("{BASE_URL}/dance.php?id={id}");
        let formation = callers_box_formation(&dance);
        CardDance {
            id: dance.id,
            title: dance.name,
            authors: dance.authors,
            formation,
            source: DanceSource::CallersBox,
            source_json_url: Some(format!("{source_url}&format=JSON")),
            source_url,
            phrases: dance
                .phrases
                .into_iter()
                .map(|phrase| CardPhrase {
                    name: phrase.name,
                    figures: phrase
                        .figures
                        .into_iter()
                        .map(|figure| {
                            let (beats, text) = split_beats(&figure);
                            CardFigure {
                                beats,
                                text: text.to_owned(),
                            }
                        })
                        .collect(),
                })
                .collect(),
            notes: dance.calling_notes,
        }
    }
}

fn callers_box_formation(dance: &CallersBoxDance) -> String {
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
    title_font_size: f32,
    title_y: f32,
    max_figure_chars: usize,
    max_note_chars: usize,
}

#[derive(Debug, Default)]
struct SvgCardInfo {
    title: Option<String>,
    authors: Option<String>,
    formation: Option<String>,
    source: Option<DanceSource>,
    source_id: Option<String>,
    source_url: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectedDanceOutcome {
    Done,
    BackToSelection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CreateChoice {
    Create,
    Skip,
    Back,
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

        if let Err(err) = handle_dance_input(&input) {
            eprintln!("Could not load dance: {err:#}\n");
        }
        println!();
    }
}

fn add_command(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return interactive_loop();
    }

    let input = args.join(" ");
    handle_dance_input(&input)
}

fn handle_dance_input(input: &str) -> Result<()> {
    if let Some(id) = contradb_id_from_input(input) {
        let dance = fetch_contradb_dance(&id)?;
        let _ = handle_selected_dance(&dance)?;
        return Ok(());
    }
    if let Some(id) = id_from_input(input) {
        let dance = fetch_callers_box_dance(&id)?;
        let _ = handle_selected_dance(&dance)?;
        return Ok(());
    }

    let candidates = search_by_title(input)?;
    if candidates.is_empty() {
        bail!("No matches for {input:?}");
    }

    handle_search_candidates(&candidates)
}

fn handle_search_candidates(candidates: &[DanceCandidate]) -> Result<()> {
    let shown = candidates.len().min(25);

    loop {
        print_candidates(candidates, shown);
        let Some(candidate) = choose_candidate(candidates, shown)? else {
            bail!("selection cancelled");
        };

        let dance = match candidate.source {
            DanceSource::CallersBox => fetch_callers_box_dance(&candidate.id)?,
            DanceSource::ContraDb => fetch_contradb_dance(&candidate.id)?,
        };

        match handle_selected_dance(&dance)? {
            SelectedDanceOutcome::Done => return Ok(()),
            SelectedDanceOutcome::BackToSelection => continue,
        }
    }
}

fn handle_selected_dance(dance: &CardDance) -> Result<SelectedDanceOutcome> {
    fs::create_dir_all("dances").context("could not create dances directory")?;
    let path = dance_svg_path(dance);
    println!("\n{}", preview_text(dance));

    if path.exists() {
        let info = svg_card_info(&path).ok();
        println!("Already have this card: {}", path.display());
        if let Some(info) = &info {
            if let Some(source) = info.source {
                println!("Existing source: {}", source.label());
            }
            if let Some(source_id) = &info.source_id {
                println!("Existing source ID: {source_id}");
            }
        }
        match confirm_update_existing()? {
            CreateChoice::Create => {}
            CreateChoice::Skip => {
                println!("Skipped existing card.");
                return Ok(SelectedDanceOutcome::Done);
            }
            CreateChoice::Back => return Ok(SelectedDanceOutcome::BackToSelection),
        }

        write_svg_to_path(dance, &path)?;
        println!("Updated {}", path.display());
        if confirm_no_default("Print updated card? [y/N]: ")? {
            let options = print::PrintOptions::default_for_path(path.clone());
            print::print_path_with_options(&options, &path)?;
        }
        return Ok(SelectedDanceOutcome::Done);
    }

    match confirm_create_dance()? {
        CreateChoice::Create => {
            write_svg_to_path(dance, &path)?;
            after_writing_card(path)?;
            Ok(SelectedDanceOutcome::Done)
        }
        CreateChoice::Skip => {
            println!("No worries. Back to search.");
            Ok(SelectedDanceOutcome::Done)
        }
        CreateChoice::Back => Ok(SelectedDanceOutcome::BackToSelection),
    }
}

fn handle_existing_card(path: PathBuf) -> Result<()> {
    let info = svg_card_info(&path)?;
    println!("\n{}", existing_card_summary(&path, &info));
    if confirm_yes_default("Print this card? [Y/n]: ")? {
        let options = print::PrintOptions::default_for_path(path.clone());
        print::print_path_with_options(&options, &path)?;
    }
    Ok(())
}

fn regen_command(args: &[String]) -> Result<()> {
    if args.iter().any(|arg| arg == "--all") {
        return regen_all_command(args);
    }

    let Some(path_arg) = args.first() else {
        bail!(
            "usage: contra-card regen <path-to-svg> [--yes]\n       contra-card regen --all [--yes]"
        );
    };
    let yes = args.iter().any(|arg| arg == "--yes" || arg == "-y");
    let path = PathBuf::from(path_arg);
    let dance = fetch_dance_from_svg_metadata(&path)?;

    println!("\n{}", preview_text(&dance));
    if !yes && !confirm_overwrite(&path)? {
        println!("Skipped {}", path.display());
        return Ok(());
    }

    write_svg_to_path(&dance, &path)?;
    println!("Regenerated {}", path.display());
    Ok(())
}

fn regen_all_command(args: &[String]) -> Result<()> {
    let yes = args.iter().any(|arg| arg == "--yes" || arg == "-y");
    if !yes && !confirm_yes_default("Regenerate all SVGs in dances/? [Y/n]: ")? {
        println!("Skipped regen --all");
        return Ok(());
    }

    let paths = dance_svg_paths()?;
    if paths.is_empty() {
        println!("No SVG cards found in dances/.");
        return Ok(());
    }

    let mut regenerated = 0;
    let mut failures = Vec::new();
    for path in paths {
        print!("Regenerating {} ... ", path.display());
        io::stdout().flush()?;
        match regen_svg_path(&path) {
            Ok(()) => {
                regenerated += 1;
                println!("ok");
            }
            Err(err) => {
                println!("failed");
                failures.push((path, err.to_string()));
            }
        }
    }

    println!(
        "\nRegenerated {regenerated} card{}.",
        if regenerated == 1 { "" } else { "s" }
    );
    if failures.is_empty() {
        return Ok(());
    }

    eprintln!("{} card(s) failed:", failures.len());
    for (path, err) in failures {
        eprintln!("- {}: {err}", path.display());
    }
    bail!("regen --all completed with failures")
}

fn regen_svg_path(path: &Path) -> Result<()> {
    let dance = fetch_dance_from_svg_metadata(path)?;
    write_svg_to_path(&dance, path)
}

fn fetch_dance_from_svg_metadata(path: &Path) -> Result<CardDance> {
    let source_id = source_id_from_svg(path)?;
    let source = svg_card_info(path)?.source;
    match source.unwrap_or(DanceSource::CallersBox) {
        DanceSource::CallersBox => fetch_callers_box_dance(&source_id),
        DanceSource::ContraDb => fetch_contradb_dance(&source_id),
    }
}

fn print_command(args: &[String]) -> Result<()> {
    let job = print::parse_print_job(args)?;
    print::print_paths(&job)
}

fn after_writing_card(path: PathBuf) -> Result<()> {
    println!("Wrote {}", path.display());
    if confirm_yes_default("Print now? [Y/n]: ")? {
        let options = print::PrintOptions::default_for_path(path.clone());
        print::print_path_with_options(&options, &path)?;
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
  contra-card regen --all     Re-fetch all SVGs in dances/
  contra-card printers        List configured CUPS printers
  contra-card print <SVG>...  Print one or more SVG cards using media=3x5

Notes:
  QUERY can be a dance title, Caller’s Box URL, ContraDB URL, or raw Caller’s Box ID.
  The interactive prompt also accepts existing SVG paths or filenames from dances/.
  Interactive yes/no prompts default to yes, so Enter proceeds.
  Print defaults to your custom paper size named 3x5 and landscape orientation.
  Use `contra-card print <SVG> [<SVG> ...] --dry-run` to inspect the lp command.
  `cargo run --` can be used in front of these commands during development.
"#
    );
}

fn print_candidates(candidates: &[DanceCandidate], shown: usize) {
    println!("\nMatches:");
    for (i, candidate) in candidates.iter().take(shown).enumerate() {
        println!(
            "{:>2}. [{}] {} — {} ({})",
            i + 1,
            candidate.source.label(),
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
}

fn choose_candidate(
    candidates: &[DanceCandidate],
    shown: usize,
) -> Result<Option<&DanceCandidate>> {
    loop {
        let choice = prompt("Choose a dance: ")?;
        if choice.eq_ignore_ascii_case("b") {
            return Ok(None);
        }

        let Ok(index) = choice.parse::<usize>() else {
            eprintln!("Enter a number or b.");
            continue;
        };
        let Some(candidate) = candidates.iter().take(shown).nth(index.saturating_sub(1)) else {
            eprintln!("Choose 1-{shown}.");
            continue;
        };

        return Ok(Some(candidate));
    }
}

fn id_from_input(input: &str) -> Option<String> {
    let id_re = Regex::new(r"(?:\bid=|^)(\d+)\b").ok()?;
    id_re
        .captures(input)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_owned())
}

fn contradb_id_from_input(input: &str) -> Option<String> {
    if !input.contains("contradb.com") {
        return None;
    }
    let id_re = Regex::new(r"/dances/(\d+)").ok()?;
    id_re
        .captures(input)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_owned())
}

fn search_by_title(title: &str) -> Result<Vec<DanceCandidate>> {
    let mut candidates = search_callers_box_by_title(title)?;
    match search_contradb_by_title(title) {
        Ok(mut contradb) => candidates.append(&mut contradb),
        Err(err) => eprintln!("ContraDB search failed: {err:#}"),
    }
    Ok(candidates)
}

fn search_callers_box_by_title(title: &str) -> Result<Vec<DanceCandidate>> {
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
        .map(|caps| DanceCandidate {
            source: DanceSource::CallersBox,
            id: caps[1].to_owned(),
            name: clean_html(&caps[2]),
            author: clean_html(&caps[3]),
            formation: clean_html(&caps[4]),
        })
        .collect())
}

fn search_contradb_by_title(title: &str) -> Result<Vec<DanceCandidate>> {
    let body = serde_json::json!({
        "filter": ["title", title],
        "count": 25,
        "offset": 0,
    });
    let response = http_client()?
        .post(format!("{CONTRADB_BASE_URL}/api/v1/dances"))
        .json(&body)
        .send()
        .context("ContraDB search request failed")?
        .error_for_status()
        .context("ContraDB search returned an error")?
        .json::<ContraDbSearchResponse>()
        .context("could not parse ContraDB search response")?;

    Ok(response
        .dances
        .into_iter()
        .map(|dance| DanceCandidate {
            source: DanceSource::ContraDb,
            id: dance.id.to_string(),
            name: dance.title,
            author: dance.choreographer_name,
            formation: dance.formation,
        })
        .collect())
}

fn fetch_contradb_dance(id: &str) -> Result<CardDance> {
    let source_url = format!("{CONTRADB_BASE_URL}/dances/{id}");
    let html = http_client()?
        .get(&source_url)
        .send()
        .context("ContraDB dance request failed")?
        .error_for_status()
        .context("ContraDB dance returned an error")?
        .text()
        .context("could not read ContraDB dance response")?;
    parse_contradb_dance(id, &source_url, &html)
}

fn parse_contradb_dance(id: &str, source_url: &str, html: &str) -> Result<CardDance> {
    let title = capture_clean(html, r#"(?s)<h1 class="dance-show-title">(.*?)</h1>"#)
        .context("ContraDB dance page did not include a title")?;
    let author = capture_clean(
        html,
        r#"(?s)<p class="dance-show-choreographer">.*?<strong>.*?>(.*?)</a>"#,
    )
    .unwrap_or_else(|| "Unknown".to_owned());
    let formation = capture_clean(
        html,
        r#"(?s)<p class="dance-show-formation">formation:\s*(.*?)</p>"#,
    )
    .unwrap_or_default();

    let row_re = Regex::new(
        r#"(?s)<tr class="[^"]*">\s*<td>(.*?)</td>\s*<td class=dance-show-beats>(.*?)</td>\s*<td><div class="show-figure">(.*?)</div>"#,
    )?;
    let mut phrases = Vec::<CardPhrase>::new();
    for caps in row_re.captures_iter(html) {
        let phrase_name = clean_html(&caps[1]);
        if !phrase_name.is_empty() || phrases.is_empty() {
            phrases.push(CardPhrase {
                name: phrase_name,
                figures: Vec::new(),
            });
        }
        let Some(phrase) = phrases.last_mut() else {
            continue;
        };
        phrase.figures.push(CardFigure {
            beats: Some(clean_html(&caps[2])).filter(|s| !s.is_empty()),
            text: clean_html(&caps[3]),
        });
    }

    let notes = capture_clean(
        html,
        r#"(?s)<div class="dance-show-notes"><div class='contra-markdown-block'>(.*?)</div></div>"#,
    )
    .into_iter()
    .filter(|note| !note.trim().is_empty())
    .collect();

    Ok(CardDance {
        id: id.to_owned(),
        title,
        authors: vec![author],
        formation,
        source: DanceSource::ContraDb,
        source_url: source_url.to_owned(),
        source_json_url: None,
        phrases,
        notes,
    })
}

fn fetch_callers_box_dance(id: &str) -> Result<CardDance> {
    let url = format!("{BASE_URL}/dance.php?id={id}&format=JSON");
    let dance = http_client()?
        .get(url)
        .send()
        .context("dance request failed")?
        .error_for_status()
        .context("dance request returned an error")?
        .json::<CallersBoxDance>()
        .context("could not parse dance JSON")?;
    Ok(dance.into())
}

fn http_client() -> Result<Client> {
    Client::builder()
        .user_agent(BROWSER_USER_AGENT)
        .build()
        .context("could not build HTTP client")
}

fn confirm_create_dance() -> Result<CreateChoice> {
    loop {
        let answer = prompt("Create SVG for this dance? [Y/n/b/s]: ")?;
        match answer.to_lowercase().as_str() {
            "" | "y" | "yes" => return Ok(CreateChoice::Create),
            "n" | "no" | "s" | "search" => return Ok(CreateChoice::Skip),
            "b" | "back" => return Ok(CreateChoice::Back),
            _ => eprintln!("Enter y, n, b, or s."),
        }
    }
}

fn confirm_update_existing() -> Result<CreateChoice> {
    loop {
        let answer = prompt("Update existing SVG? [y/N/b/s]: ")?;
        match answer.to_lowercase().as_str() {
            "y" | "yes" => return Ok(CreateChoice::Create),
            "" | "n" | "no" | "s" | "search" => return Ok(CreateChoice::Skip),
            "b" | "back" => return Ok(CreateChoice::Back),
            _ => eprintln!("Enter y, n, b, or s."),
        }
    }
}

fn preview_text(dance: &CardDance) -> String {
    let mut out = String::new();
    out.push_str(&format!("{} — {}\n", dance.title, dance.authors.join(", ")));
    out.push_str(&format!("Source: {}\n", dance.source.label()));
    out.push_str(&format!("{}\n", dance_meta(dance)));
    out.push_str("----------------------------------------\n");

    for phrase in &dance.phrases {
        for (i, figure) in phrase.figures.iter().enumerate() {
            let role = if i == 0 { phrase.name.as_str() } else { "" };
            out.push_str(&format!(
                "{:<3} {:>2}  {}\n",
                role,
                figure.beats.clone().unwrap_or_default(),
                normalize_card_text(&figure.text)
            ));
        }
    }

    if !dance.notes.is_empty() {
        out.push_str("\nNotes:\n");
        for note in &dance.notes {
            out.push_str(&format!("- {}\n", normalize_card_text(note)));
        }
    }

    out
}

fn dance_svg_path(dance: &CardDance) -> PathBuf {
    let filename = format!("{}.svg", slugify(&dance.title));
    PathBuf::from("dances").join(filename)
}

fn write_svg_to_path(dance: &CardDance, path: &Path) -> Result<()> {
    let svg = render_svg(dance);
    fs::write(path, svg).with_context(|| format!("could not write {}", path.display()))
}

fn render_svg(dance: &CardDance) -> String {
    let title = encode_text(&dance.title);
    let authors_text = format!("By {}", dance.authors.join(", "));
    let authors = encode_text(&authors_text);
    let meta_text = dance_meta(dance);
    let meta = render_header_meta(&meta_text);
    let source = encode_double_quoted_attribute(&dance.source_url);
    let source_id = encode_double_quoted_attribute(&dance.id);
    let source_json = encode_double_quoted_attribute(
        dance
            .source_json_url
            .as_deref()
            .unwrap_or(dance.source_url.as_str()),
    );
    let metadata_source = dance.source.metadata_value();
    let metadata_name = encode_double_quoted_attribute(&dance.title);
    let metadata_authors_text = dance.authors.join(", ");
    let metadata_authors = encode_double_quoted_attribute(&metadata_authors_text);
    let metadata_formation = encode_double_quoted_attribute(&meta_text);
    let layout = card_layout(dance);
    let move_badges = render_move_badges(dance);

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
            let wrapped_lines =
                wrap::wrap_text(&normalize_card_text(&figure.text), layout.max_figure_chars);
            for (line_index, line) in wrapped_lines.iter().enumerate() {
                let beat_text = if line_index == 0 {
                    encode_text(&figure.beats.clone().unwrap_or_default()).to_string()
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
            let rule_y = y - (layout.phrase_gap / 2.0) - 8.0;
            phrase_rules.push_str(&format!(
                r#"<path class="phrase-rule" d="M{left_rule_x:.1} {rule_y:.1} H484"/>
"#,
                left_rule_x = layout.left_rule_x,
            ));
            y += layout.phrase_gap;
        }
    }

    let mut notes = String::new();
    if !dance.notes.is_empty() {
        let mut first_note_line = true;
        for note in &dance.notes {
            if note.trim().is_empty() {
                continue;
            }
            for line in wrap::wrap_text(&normalize_card_text(note), layout.max_note_chars) {
                let note_x = if line.indent || !first_note_line {
                    layout.notes_x + 12.0
                } else {
                    layout.notes_x
                };
                let text = encode_text(&line.text);
                if first_note_line {
                    notes.push_str(&format!(
                        r#"<text x="{note_x:.1}" y="{y:.1}" class="notes"><tspan class="notes-label">Notes:</tspan><tspan> {text}</tspan></text>"#,
                    ));
                } else {
                    notes.push_str(&format!(
                        r#"<text x="{note_x:.1}" y="{y:.1}" class="notes">{text}</text>"#,
                    ));
                }
                first_note_line = false;
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
      source="{metadata_source}"
      source-id="{source_id}"
      source-url="{source}"
      source-json-url="{source_json}" />
  </metadata>
  <style>
    .paper {{ fill: #fffdf5; }}
    .phrase-rule {{ stroke: #7aa0b8; stroke-width: 1.2; opacity: 0.8; }}
    .top-rule {{ stroke: #b64545; stroke-width: 2; }}
    text {{ fill: #1d2528; font-family: "Avenir Next", Arial, sans-serif; }}
    .title {{ font-size: {title_font_size:.1}px; font-weight: 600; }}
    .badge-text {{ font-size: 9.0px; font-weight: 700; dominant-baseline: middle; text-anchor: middle; }}
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
    {move_badges}
    <text x="16" y="{title_y:.1}" class="title">{title}</text>
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
        title_font_size = layout.title_font_size,
        title_y = layout.title_y,
    )
}

fn render_move_badges(dance: &CardDance) -> String {
    let figure_texts = dance
        .phrases
        .iter()
        .flat_map(|phrase| phrase.figures.iter())
        .map(|figure| normalize_card_text(&figure.text))
        .collect::<Vec<_>>();
    let tags = moves::tags_for_texts(figure_texts.iter().map(String::as_str));

    let mut out = String::new();
    let mut x = 16.0;
    for tag in tags {
        let width = badge_width(tag.label);
        let y = 12.0;
        let text_y = y + 7.1;
        let label = encode_text(tag.label);
        out.push_str(&format!(
            r##"<rect x="{x:.1}" y="{y:.1}" width="{width:.1}" height="14.0" rx="7.0" fill="{bg}"/>
<text x="{text_x:.1}" y="{text_y:.1}" class="badge-text" style="fill: {fg};">{label}</text>
"##,
            bg = tag.bg,
            fg = tag.fg,
            text_x = x + width / 2.0,
        ));
        x += width + 4.0;
    }
    out
}

fn badge_width(label: &str) -> f32 {
    (text_width(label, 9.0, 0.54) + 12.0).max(25.0)
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

fn card_layout(dance: &CardDance) -> CardLayout {
    let base_figure_font_size = 17.0;
    let figure_x = 104.0;
    let right_edge = 484.0;
    let notes_x = 24.0;
    let max_figure_chars = max_chars_for_width(right_edge - figure_x, base_figure_font_size);
    let max_note_chars = max_chars_for_width(right_edge - notes_x, 13.0);
    let figure_rows = figure_visual_rows(dance, max_figure_chars);
    let phrase_gaps = dance.phrases.len().saturating_sub(1);
    let note_rows = if dance.notes.is_empty() {
        0
    } else {
        dance
            .notes
            .iter()
            .filter(|note| !note.trim().is_empty())
            .map(|note| wrap::wrap_text(&normalize_card_text(note), max_note_chars).len())
            .sum::<usize>()
    };

    let body_start = 66.0;
    let body_max_baseline = 268.0;
    let default_row_step = 21.0;
    let default_phrase_gap = 7.0;
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
        title_font_size: title_font_size(&dance.title),
        title_y: 43.0,
        max_figure_chars,
        max_note_chars,
    }
}

fn title_font_size(title: &str) -> f32 {
    let max_width = 330.0;
    let base_size = 23.0;
    let min_size = 16.0;
    let estimated_width = text_width(title, base_size, 0.56);
    if estimated_width <= max_width {
        base_size
    } else {
        (base_size * max_width / estimated_width).max(min_size)
    }
}

fn figure_visual_rows(dance: &CardDance, max_chars: usize) -> usize {
    dance
        .phrases
        .iter()
        .map(|phrase| {
            phrase
                .figures
                .iter()
                .map(|figure| wrap::wrap_text(&normalize_card_text(&figure.text), max_chars).len())
                .sum::<usize>()
                .max(1)
        })
        .sum()
}

fn phrase_visual_rows(phrase: &CardPhrase, layout: &CardLayout) -> usize {
    phrase
        .figures
        .iter()
        .map(|figure| {
            wrap::wrap_text(&normalize_card_text(&figure.text), layout.max_figure_chars).len()
        })
        .sum::<usize>()
        .max(1)
}

fn max_chars_for_width(width: f32, font_size: f32) -> usize {
    (width / (font_size * 0.45)).floor().max(16.0) as usize
}

fn text_width(text: &str, font_size: f32, average_em: f32) -> f32 {
    text.chars()
        .map(|ch| match ch {
            'i' | 'l' | 'I' | '|' | ';' | ':' | '.' | ',' | '\'' => 0.32,
            'm' | 'w' | 'M' | 'W' => 0.9,
            ' ' => 0.28,
            _ => average_em,
        })
        .sum::<f32>()
        * font_size
}

fn content_height(content_rows: usize, phrase_gaps: usize, row_step: f32, phrase_gap: f32) -> f32 {
    if content_rows == 0 {
        0.0
    } else {
        (content_rows - 1) as f32 * row_step + phrase_gaps as f32 * phrase_gap
    }
}

fn dance_meta(dance: &CardDance) -> String {
    compact_formation(&dance.formation)
}

fn compact_formation(formation: &str) -> String {
    formation
        .split('|')
        .map(str::trim)
        .filter(|part| !part.eq_ignore_ascii_case("Single progression"))
        .map(|part| {
            let part = part.replace("Duple Minor", "Hands 4");
            if part.eq_ignore_ascii_case("Becket ccw") {
                "Hands 4 - Becket | CCW".to_owned()
            } else if part.eq_ignore_ascii_case("Becket") {
                "Hands 4 - Becket".to_owned()
            } else if part.eq_ignore_ascii_case("improper") {
                "Hands 4 - Improper".to_owned()
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" | ")
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
        (r"\bgentlespoons\b", "Larks"),
        (r"\bgentlespoon\b", "Lark"),
        (r"\bladies\b", "Robins"),
        (r"\blady\b", "Robin"),
        (r"\bladles\b", "Robins"),
        (r"\bladle\b", "Robin"),
        (r"\bwomen\b", "Robins"),
        (r"\bwoman\b", "Robin"),
        (r"\bgyre\b", "RSR"),
        (r"\bhay\b", "hey"),
        (r"\bshift left\b", "slide left"),
        (r"\bshift right\b", "slide right"),
        (r"\b1/2 hey\b", "half hey"),
    ];

    let mut out = text.to_owned();
    for (pattern, replacement) in replacements {
        let re = Regex::new(&format!("(?i){pattern}")).expect("valid replacement regex");
        out = re.replace_all(&out, replacement).into_owned();
    }

    neutralize_shorthand(&out)
}

fn normalize_card_text(text: &str) -> String {
    sentence_case_preserving_tokens(&neutralize_terms(text))
}

fn sentence_case_preserving_tokens(text: &str) -> String {
    let whitespace_re = Regex::new(r"\s+").expect("valid whitespace regex");
    let token_re = Regex::new(r"[A-Za-z][A-Za-z0-9]*|[A-Za-z0-9]*[A-Za-z][A-Za-z0-9]*")
        .expect("valid token regex");
    let collapsed = whitespace_re.replace_all(text.trim(), " ");
    let mut saw_capitalizable_token = false;

    token_re
        .replace_all(&collapsed, |caps: &regex::Captures| {
            let token = &caps[0];
            if is_preserved_short_token(token) {
                return token.to_ascii_uppercase();
            }

            let normalized =
                canonical_card_word(token).unwrap_or_else(|| token.to_ascii_lowercase());
            if saw_capitalizable_token {
                normalized
            } else {
                saw_capitalizable_token = true;
                capitalize_first_ascii(&normalized)
            }
        })
        .into_owned()
}

fn is_preserved_short_token(token: &str) -> bool {
    let shorthand_re = Regex::new(r"(?i)^(?:RSR|BTR|[NPLR][LR]|[LR][12]|N[12][LR])$")
        .expect("valid shorthand regex");
    shorthand_re.is_match(token)
}

fn canonical_card_word(token: &str) -> Option<String> {
    let canonical = match token.to_ascii_lowercase().as_str() {
        "lark" => "Lark",
        "larks" => "Larks",
        "robin" => "Robin",
        "robins" => "Robins",
        "partner" => "Partner",
        "partners" => "Partners",
        "neighbor" => "Neighbor",
        "neighbors" => "Neighbors",
        "california" => "California",
        _ => return None,
    };
    Some(canonical.to_owned())
}

fn capitalize_first_ascii(text: &str) -> String {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out = first.to_ascii_uppercase().to_string();
    out.push_str(chars.as_str());
    out
}

fn neutralize_shorthand(text: &str) -> String {
    let dancer_number_re = Regex::new(r"\b([MW])([12])\b").expect("valid shorthand regex");
    let pass_shorthand_re = Regex::new(r"\b([MW])([LR])\b").expect("valid shorthand regex");

    let out = dancer_number_re.replace_all(text, |caps: &regex::Captures| {
        let role = match &caps[1] {
            "M" => "L",
            "W" => "R",
            other => other,
        };
        format!("{role}{}", &caps[2])
    });

    pass_shorthand_re
        .replace_all(&out, |caps: &regex::Captures| {
            let role = match &caps[1] {
                "M" => "L",
                "W" => "R",
                other => other,
            };
            format!("{role}{}", &caps[2])
        })
        .into_owned()
}

fn clean_html(input: &str) -> String {
    let tag_re = Regex::new(r"<[^>]*>").expect("valid tag regex");
    let without_tags = tag_re.replace_all(input, "");
    decode_html_entities(without_tags.trim()).into_owned()
}

fn capture_clean(input: &str, pattern: &str) -> Option<String> {
    let re = Regex::new(pattern).ok()?;
    re.captures(input)
        .and_then(|caps| caps.get(1))
        .map(|m| clean_html(m.as_str()))
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

fn dance_svg_paths() -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let entries = fs::read_dir("dances").context("could not read dances directory")?;
    for entry in entries {
        let entry = entry.context("could not read dances directory entry")?;
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "svg") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
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
        source: svg_attr(&svg, "source").and_then(|value| value.parse().ok()),
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
    if let Some(source) = info.source {
        lines.push(format!("Source DB: {}", source.label()));
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

fn confirm_no_default(label: &str) -> Result<bool> {
    loop {
        let answer = prompt(label)?;
        match answer.to_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "" | "n" | "no" => return Ok(false),
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
            neutralize_terms(
                "Men pass left; ladies chain; gentlespoons and ladles; new neighbor ladle"
            ),
            "Larks pass left; Robins chain; Larks and Robins; new neighbor Robin"
        );
    }

    #[test]
    fn neutralizes_gendered_shorthand() {
        assert_eq!(
            neutralize_terms("(W2-M1-W1-M2) ML;WR;WL;MR"),
            "(R2-L1-R1-L2) LL;RR;RL;LR"
        );
    }

    #[test]
    fn preserves_progress_symbol() {
        assert_eq!(
            neutralize_terms("partners California twirl ⁋"),
            "partners California twirl ⁋"
        );
    }

    #[test]
    fn converts_gyre_to_local_shorthand() {
        assert_eq!(neutralize_terms("Robins gyre once"), "Robins RSR once");
    }

    #[test]
    fn normalizes_card_text_casing() {
        assert_eq!(
            normalize_card_text("balance &  petronella"),
            "Balance & petronella"
        );
        assert_eq!(
            normalize_card_text("ladles start a hay"),
            "Robins start a hey"
        );
        assert_eq!(
            normalize_card_text("ladles allemande right ¾ to new neighbor ladle ⁋"),
            "Robins allemande right ¾ to new Neighbor Robin ⁋"
        );
        assert_eq!(
            normalize_card_text("gentlespoons shift left"),
            "Larks slide left"
        );
        assert_eq!(
            normalize_card_text("gentlespoons shift right"),
            "Larks slide right"
        );
        assert_eq!(
            normalize_card_text("ladles start a 1/2 hey"),
            "Robins start a half hey"
        );
        assert_eq!(normalize_card_text("neighbors swing"), "Neighbors swing");
        assert_eq!(
            normalize_card_text("partners California twirl ⁋"),
            "Partners California twirl ⁋"
        );
        assert_eq!(
            normalize_card_text("(W2-M1-W1-M2) ML;WR;WL;MR hey"),
            "(R2-L1-R1-L2) LL;RR;RL;LR Hey"
        );
        assert_eq!(normalize_card_text("Robins gyre 1½"), "Robins RSR 1½");
    }

    #[test]
    fn slugs_names() {
        assert_eq!(slugify("In the Mood!"), "in-the-mood");
    }

    #[test]
    fn scales_long_titles() {
        assert_eq!(title_font_size("In the Mood"), 23.0);
        assert!(title_font_size("Maliza's Magical Mystery Motion") < 22.0);
    }

    #[test]
    fn compacts_common_formation_metadata() {
        assert_eq!(
            compact_formation("Duple Minor - Improper | Single progression"),
            "Hands 4 - Improper"
        );
        assert_eq!(
            compact_formation("Duple Minor - Becket | Single progression | CCW"),
            "Hands 4 - Becket | CCW"
        );
        assert_eq!(compact_formation("Becket ccw"), "Hands 4 - Becket | CCW");
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

    #[test]
    fn parses_contradb_dance_page() {
        let html = r#"
          <h1 class="dance-show-title">Test Dance</h1>
          <p class="dance-show-choreographer">by: <strong><a href="/choreographers/1">Jane Caller</a></strong></p>
          <p class="dance-show-formation">formation: Becket ccw </p>
          <table>
            <tr class="a1b1 "><td>A1</td><td class=dance-show-beats>8</td><td><div class="show-figure">neighbors swing</div></td></tr>
            <tr class="a1b1 "><td></td><td class=dance-show-beats>8</td><td><div class="show-figure">ladles chain</div></td></tr>
          </table>
          <div class="dance-show-notes"><div class='contra-markdown-block'>A note</div></div>
        "#;
        let dance = parse_contradb_dance("42", "https://contradb.com/dances/42", html).unwrap();
        assert_eq!(dance.title, "Test Dance");
        assert_eq!(dance.authors, vec!["Jane Caller"]);
        assert_eq!(dance.formation, "Becket ccw");
        assert_eq!(dance.source.metadata_value(), "contradb");
        assert_eq!(dance.phrases[0].name, "A1");
        assert_eq!(dance.phrases[0].figures[1].text, "ladles chain");
        assert_eq!(dance.notes, vec!["A note"]);
    }
}
