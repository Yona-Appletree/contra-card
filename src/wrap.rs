#[derive(Debug, Eq, PartialEq)]
pub struct WrappedLine {
    pub text: String,
    pub indent: bool,
}

pub fn wrap_text(text: &str, max_chars: usize) -> Vec<WrappedLine> {
    let text = text.trim();
    if text.is_empty() {
        return vec![WrappedLine {
            text: String::new(),
            indent: false,
        }];
    }

    let max_chars = max_chars.max(16);
    let mut lines = Vec::new();
    let mut current = String::new();

    let words = attach_progress_markers(text);
    for word in &words {
        let pending_len = if current.is_empty() {
            visual_len(word)
        } else {
            visual_len(&current) + 1 + visual_len(word)
        };

        if !current.is_empty() && pending_len > max_chars {
            lines.push(WrappedLine {
                text: current,
                indent: !lines.is_empty(),
            });
            current = String::new();
        }

        if current.is_empty() {
            current.push_str(word);
        } else {
            current.push(' ');
            current.push_str(word);
        }
    }

    if !current.is_empty() {
        lines.push(WrappedLine {
            text: current,
            indent: !lines.is_empty(),
        });
    }

    lines
}

fn visual_len(text: &str) -> usize {
    text.chars()
        .map(|ch| match ch {
            'i' | 'l' | 'I' | '|' | ';' | ':' | '.' | ',' | '\'' => 1,
            'm' | 'w' | 'M' | 'W' => 2,
            _ => 1,
        })
        .sum()
}

fn attach_progress_markers(text: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        if word == "⁋" {
            if let Some(previous) = words.last_mut() {
                previous.push(' ');
                previous.push_str(word);
            } else {
                words.push(word.to_owned());
            }
        } else {
            words.push(word.to_owned());
        }
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_short_text_alone() {
        assert_eq!(
            wrap_text("Partner swing", 40),
            vec![WrappedLine {
                text: "Partner swing".to_owned(),
                indent: false,
            }]
        );
    }

    #[test]
    fn wraps_on_words_and_marks_continuations() {
        assert_eq!(
            wrap_text("On right diagonal, hey (MR;N2L;WR;PL;MR;N2L;WR;PL)", 38),
            vec![
                WrappedLine {
                    text: "On right diagonal, hey".to_owned(),
                    indent: false,
                },
                WrappedLine {
                    text: "(MR;N2L;WR;PL;MR;N2L;WR;PL)".to_owned(),
                    indent: true,
                },
            ]
        );
    }

    #[test]
    fn keeps_progress_marker_with_previous_words() {
        assert_eq!(
            wrap_text("Balance & petronella and turn to face the next ⁋", 48),
            vec![WrappedLine {
                text: "Balance & petronella and turn to face the next ⁋".to_owned(),
                indent: false,
            }]
        );
    }
}
