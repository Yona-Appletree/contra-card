#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpanKind {
    Plain,
    Role,
    Move,
    Amount,
}

impl SpanKind {
    pub fn class_name(self) -> &'static str {
        match self {
            SpanKind::Plain => "plain",
            SpanKind::Role => "role",
            SpanKind::Move => "move",
            SpanKind::Amount => "amount",
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct Span<'a> {
    pub kind: SpanKind,
    pub text: &'a str,
}

#[derive(Clone, Copy)]
struct Rule {
    kind: SpanKind,
    phrase: &'static str,
}

const ROLES: &[&str] = &[
    "Larks",
    "Robins",
    "Partner",
    "Partners",
    "Neighbor",
    "Neighbors",
    "Corner",
    "Corners",
    "Shadow",
    "Shadows",
];

const AMOUNTS: &[&str] = &[
    "counterclockwise",
    "clockwise",
    "once and a half",
    "one and a half",
    "three quarters",
    "halfway",
    "1 1/2",
    "1/2",
    "7/8",
    "3/4",
    "1/4",
];

const MOVES: &[&str] = &[
    "right and left through",
    "right hand balance",
    "right shoulder round",
    "left shoulder round",
    "partners balance & swing",
    "neighbors balance & swing",
    "balance and swing",
    "balance the ring",
    "balance the wave",
    "box the gnat",
    "box circulate",
    "right-hand chain",
    "pull by dancers",
    "right left through",
    "square through",
    "form an ocean wave",
    "form a long wave",
    "pass through",
    "California twirl",
    "Rory O'More",
    "facing star",
    "meltdown swing",
    "give & take",
    "half sashay",
    "pass by",
    "roll away",
    "turn as a couple",
    "turn as couples",
    "turn as couple",
    "bend into a ring",
    "go down the hall",
    "go up the hall",
    "down the hall",
    "up the hall",
    "bend the line",
    "long lines",
    "mad robin",
    "petronella",
    "RSR",
    "half poussette",
    "poussette",
    "circle",
    "circle left",
    "circle right",
    "allemande",
    "allemande left",
    "allemande right",
    "do si do",
    "do-si-do",
    "dosido",
    "promenade across",
    "promenade",
    "courtesy turn",
    "ocean wave",
    "long wave",
    "slice left",
    "slide left",
    "slide along set",
    "zig left zag right",
    "zig zag",
    "ricochet",
    "loop right",
    "loop wide",
    "J-hook",
    "pull by",
    "pass left",
    "pass right",
    "arch",
    "dive",
    "chain",
    "star",
    "star left",
    "star right",
    "balance",
    "swing",
    "gyre",
    "hey",
];

pub fn highlight(input: &str) -> Vec<Span<'_>> {
    let rules = rules();
    let mut spans = Vec::new();
    let mut index = 0;

    while index < input.len() {
        if let Some(rule) = best_rule_at(input, index, &rules) {
            let end = index + rule.phrase.len();
            push_span(&mut spans, rule.kind, &input[index..end]);
            index = end;
            continue;
        }

        let plain_start = index;
        while index < input.len() && best_rule_at(input, index, &rules).is_none() {
            index = input[index..]
                .char_indices()
                .nth(1)
                .map(|(offset, _)| index + offset)
                .unwrap_or(input.len());
        }
        push_span(&mut spans, SpanKind::Plain, &input[plain_start..index]);
    }

    spans
}

fn rules() -> Vec<Rule> {
    let mut rules = Vec::new();
    rules.extend(ROLES.iter().map(|phrase| Rule {
        kind: SpanKind::Role,
        phrase,
    }));
    rules.extend(AMOUNTS.iter().map(|phrase| Rule {
        kind: SpanKind::Amount,
        phrase,
    }));
    rules.extend(MOVES.iter().map(|phrase| Rule {
        kind: SpanKind::Move,
        phrase,
    }));
    rules.sort_by(|a, b| b.phrase.len().cmp(&a.phrase.len()));
    rules
}

fn best_rule_at<'a>(input: &str, index: usize, rules: &'a [Rule]) -> Option<&'a Rule> {
    rules
        .iter()
        .find(|rule| matches_phrase_at(input, index, rule.phrase))
}

fn matches_phrase_at(input: &str, index: usize, phrase: &str) -> bool {
    let Some(candidate) = input.get(index..index + phrase.len()) else {
        return false;
    };
    candidate.eq_ignore_ascii_case(phrase)
        && is_boundary(input, index)
        && is_boundary(input, index + phrase.len())
}

fn is_boundary(input: &str, index: usize) -> bool {
    let before = input[..index].chars().next_back();
    let after = input[index..].chars().next();

    match (before, after) {
        (Some(left), Some(right)) => !is_word_char(left) || !is_word_char(right),
        _ => true,
    }
}

fn is_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
}

fn push_span<'a>(spans: &mut Vec<Span<'a>>, kind: SpanKind, text: &'a str) {
    if text.is_empty() {
        return;
    }
    spans.push(Span { kind, text });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(input: &str) -> Vec<(SpanKind, &str)> {
        highlight(input)
            .into_iter()
            .map(|span| (span.kind, span.text))
            .collect()
    }

    #[test]
    fn highlights_role_move_and_amount() {
        assert_eq!(
            kinds("Larks pass left 1/2"),
            vec![
                (SpanKind::Role, "Larks"),
                (SpanKind::Plain, " "),
                (SpanKind::Move, "pass left"),
                (SpanKind::Plain, " "),
                (SpanKind::Amount, "1/2"),
            ]
        );
    }

    #[test]
    fn prefers_long_move_names() {
        assert_eq!(
            kinds("Right and left through with neighbor"),
            vec![
                (SpanKind::Move, "Right and left through"),
                (SpanKind::Plain, " with "),
                (SpanKind::Role, "neighbor"),
            ]
        );
    }

    #[test]
    fn does_not_treat_mad_robin_as_a_role() {
        assert_eq!(
            kinds("Mad robin counterclockwise around neighbor"),
            vec![
                (SpanKind::Move, "Mad robin"),
                (SpanKind::Plain, " "),
                (SpanKind::Amount, "counterclockwise"),
                (SpanKind::Plain, " around "),
                (SpanKind::Role, "neighbor"),
            ]
        );
    }

    #[test]
    fn highlights_common_travel_and_setup_moves() {
        assert_eq!(
            kinds("Neighbor turn as couples; go up the hall; bend the line; balance"),
            vec![
                (SpanKind::Role, "Neighbor"),
                (SpanKind::Plain, " "),
                (SpanKind::Move, "turn as couples"),
                (SpanKind::Plain, "; "),
                (SpanKind::Move, "go up the hall"),
                (SpanKind::Plain, "; "),
                (SpanKind::Move, "bend the line"),
                (SpanKind::Plain, "; "),
                (SpanKind::Move, "balance"),
            ]
        );
    }

    #[test]
    fn highlights_program_move_keywords() {
        assert_eq!(
            kinds(
                "Box the gnat; box circulate; do si do; give & take; square through; \
                 form an ocean wave; Rory O'More; roll away; pass through; meltdown swing; \
                 arch & dive; zig left zag right"
            ),
            vec![
                (SpanKind::Move, "Box the gnat"),
                (SpanKind::Plain, "; "),
                (SpanKind::Move, "box circulate"),
                (SpanKind::Plain, "; "),
                (SpanKind::Move, "do si do"),
                (SpanKind::Plain, "; "),
                (SpanKind::Move, "give & take"),
                (SpanKind::Plain, "; "),
                (SpanKind::Move, "square through"),
                (SpanKind::Plain, "; "),
                (SpanKind::Move, "form an ocean wave"),
                (SpanKind::Plain, "; "),
                (SpanKind::Move, "Rory O'More"),
                (SpanKind::Plain, "; "),
                (SpanKind::Move, "roll away"),
                (SpanKind::Plain, "; "),
                (SpanKind::Move, "pass through"),
                (SpanKind::Plain, "; "),
                (SpanKind::Move, "meltdown swing"),
                (SpanKind::Plain, "; "),
                (SpanKind::Move, "arch"),
                (SpanKind::Plain, " & "),
                (SpanKind::Move, "dive"),
                (SpanKind::Plain, "; "),
                (SpanKind::Move, "zig left zag right"),
            ]
        );
    }
}
