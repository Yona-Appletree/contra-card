use regex::Regex;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MoveTag {
    pub label: &'static str,
    pub fg: &'static str,
    pub bg: &'static str,
}

struct MoveTaxon {
    tag: MoveTag,
    aliases: &'static [&'static str],
}

const MOVE_TAXONOMY: &[MoveTaxon] = &[
    MoveTaxon {
        tag: MoveTag {
            label: "robins chain",
            fg: "#442600",
            bg: "#f4c56b",
        },
        aliases: &["robins chain", "robins right-hand chain"],
    },
    MoveTaxon {
        tag: MoveTag {
            label: "hey",
            fg: "#14351f",
            bg: "#8fd19e",
        },
        aliases: &["hey", "hay"],
    },
    MoveTaxon {
        tag: MoveTag {
            label: "down the hall",
            fg: "#12324a",
            bg: "#86c5e7",
        },
        aliases: &["down the hall", "go down the hall", "dth"],
    },
    MoveTaxon {
        tag: MoveTag {
            label: "promenade",
            fg: "#3b2755",
            bg: "#c4a4e9",
        },
        aliases: &["promenade", "promenade across"],
    },
    MoveTaxon {
        tag: MoveTag {
            label: "petronella",
            fg: "#5b1e28",
            bg: "#efa0a8",
        },
        aliases: &["petronella"],
    },
    MoveTaxon {
        tag: MoveTag {
            label: "star",
            fg: "#17392f",
            bg: "#88d0c0",
        },
        aliases: &["star", "star left", "star right", "facing star"],
    },
    MoveTaxon {
        tag: MoveTag {
            label: "poussette",
            fg: "#4a2b16",
            bg: "#d8b08a",
        },
        aliases: &["poussette", "half poussette"],
    },
    MoveTaxon {
        tag: MoveTag {
            label: "mad robin",
            fg: "#5b254c",
            bg: "#e0a0cd",
        },
        aliases: &["mad robin"],
    },
    MoveTaxon {
        tag: MoveTag {
            label: "slice",
            fg: "#263447",
            bg: "#a9bedc",
        },
        aliases: &["slice", "slice left"],
    },
    MoveTaxon {
        tag: MoveTag {
            label: "do-si-do",
            fg: "#372e14",
            bg: "#d9ca75",
        },
        aliases: &["do-si-do", "do si do", "dosido"],
    },
    MoveTaxon {
        tag: MoveTag {
            label: "box circulate",
            fg: "#1f3440",
            bg: "#9bc2d1",
        },
        aliases: &["box circulate"],
    },
    MoveTaxon {
        tag: MoveTag {
            label: "shadow",
            fg: "#382742",
            bg: "#c2a6d6",
        },
        aliases: &["shadow", "shadows"],
    },
    MoveTaxon {
        tag: MoveTag {
            label: "diagonal",
            fg: "#4a2c3a",
            bg: "#d8a7bd",
        },
        aliases: &["diagonal", "left diagonal", "right diagonal"],
    },
    MoveTaxon {
        tag: MoveTag {
            label: "wave",
            fg: "#183845",
            bg: "#8cc8d8",
        },
        aliases: &["wave", "waves", "wave of four", "ocean wave", "long wave"],
    },
    MoveTaxon {
        tag: MoveTag {
            label: "give & take",
            fg: "#4b2230",
            bg: "#d994a8",
        },
        aliases: &["give & take", "give and take"],
    },
    MoveTaxon {
        tag: MoveTag {
            label: "right left through",
            fg: "#213654",
            bg: "#9bb5dd",
        },
        aliases: &[
            "right left through",
            "right and left through",
            "right & left through",
        ],
    },
    MoveTaxon {
        tag: MoveTag {
            label: "square through",
            fg: "#2f3920",
            bg: "#b4ca86",
        },
        aliases: &["square through"],
    },
];

pub fn tags_for_texts<'a>(texts: impl IntoIterator<Item = &'a str>) -> Vec<MoveTag> {
    let searchable = texts
        .into_iter()
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join("\n");

    MOVE_TAXONOMY
        .iter()
        .filter(|taxon| {
            taxon
                .aliases
                .iter()
                .any(|alias| contains_phrase(&searchable, alias))
        })
        .map(|taxon| taxon.tag)
        .collect()
}

fn contains_phrase(text: &str, phrase: &str) -> bool {
    let pattern = format!(
        r"(?i)(^|[^[:alnum:]]){}($|[^[:alnum:]])",
        regex::escape(phrase)
    );
    Regex::new(&pattern)
        .expect("valid move taxonomy regex")
        .is_match(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_tags_in_taxonomy_order() {
        let tags = tags_for_texts([
            "Balance & hay; down the hall",
            "Partners promenade",
            "Petronella into star left",
        ]);
        let labels = tags.iter().map(|tag| tag.label).collect::<Vec<_>>();
        assert_eq!(
            labels,
            vec!["hey", "down the hall", "promenade", "petronella", "star"]
        );
    }

    #[test]
    fn detects_do_si_do_aliases() {
        let tags = tags_for_texts(["Partners do si do once"]);
        let labels = tags.iter().map(|tag| tag.label).collect::<Vec<_>>();
        assert_eq!(labels, vec!["do-si-do"]);
    }

    #[test]
    fn detects_box_circulate() {
        let tags = tags_for_texts(["Balance & box circulate"]);
        let labels = tags.iter().map(|tag| tag.label).collect::<Vec<_>>();
        assert_eq!(labels, vec!["box circulate"]);
    }

    #[test]
    fn detects_shadow() {
        let tags = tags_for_texts(["Left diagonal chain to shadow"]);
        let labels = tags.iter().map(|tag| tag.label).collect::<Vec<_>>();
        assert_eq!(labels, vec!["shadow", "diagonal"]);
    }

    #[test]
    fn detects_wave() {
        let tags = tags_for_texts(["Balance in a wave of four"]);
        let labels = tags.iter().map(|tag| tag.label).collect::<Vec<_>>();
        assert_eq!(labels, vec!["wave"]);
    }

    #[test]
    fn detects_becket_program_tags() {
        let tags = tags_for_texts([
            "Larks give & take neighbors",
            "Right left through",
            "Right & left through",
            "Square through two",
        ]);
        let labels = tags.iter().map(|tag| tag.label).collect::<Vec<_>>();
        assert_eq!(
            labels,
            vec!["give & take", "right left through", "square through"]
        );
    }

    #[test]
    fn does_not_match_inside_words() {
        let tags = tags_for_texts(["Chainmail is not a dance move"]);
        assert!(tags.is_empty());
    }
}
