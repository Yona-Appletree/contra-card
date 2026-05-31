# Contra Card

A tiny Rust CLI for making editable 3x5 SVG calling cards from dances in
The Caller's Box and ContraDB.

## Use

```sh
cargo run
```

Then enter either:

- a dance title, such as `In the Mood`
- a Caller’s Box dance URL
- a ContraDB dance URL
- a raw Caller’s Box dance ID, such as `14486`
- an existing SVG path, such as `dances/in-the-mood.svg`
- an existing SVG filename from `dances/`, such as `in-the-mood.svg`

The CLI shows candidates, previews the selected dance, applies a simple
gender-neutral terminology pass, and writes:

```text
dances/<dance-name>.svg
```

The SVG is intentionally plain text with editable `<text>` elements.

After writing the SVG, the interactive flow asks whether to print it. Yes/no
prompts default to yes, so pressing Enter proceeds.

If the input is an existing SVG card, the CLI shows embedded card metadata and
asks whether to print it.

## Commands

No command runs the interactive add flow:

```sh
cargo run
```

You can also start with a query:

```sh
cargo run -- add "In the Mood"
cargo run -- add 14486
```

Generated SVGs include Caller’s Box source metadata. To re-fetch and regenerate
an existing card:

```sh
cargo run -- regen dances/in-the-mood.svg
```

Use `--yes` to skip the overwrite prompt:

```sh
cargo run -- regen dances/in-the-mood.svg --yes
```

List configured printers:

```sh
cargo run -- printers
```

Print a card using your custom paper size named `3x5`:

```sh
cargo run -- print dances/in-the-mood.svg
```

Print several cards with the same settings:

```sh
cargo run -- print dances/in-the-mood.svg dances/heartbeat-contra.svg
```

Inspect the exact `lp` command before printing:

```sh
cargo run -- print dances/in-the-mood.svg --dry-run
```

Select a printer or override options:

```sh
cargo run -- print dances/in-the-mood.svg --printer HP_Color_LaserJet_M255dw__ADA71C_
cargo run -- print dances/in-the-mood.svg --media 3x5 --copies 2
```

## Caller’s Box Fetching

Search uses the normal HTML endpoint:

```text
https://www.ibiblio.org/contradance/thecallersbox/index.php?title=<query>
```

The CLI scrapes candidate dance IDs from that page. After selection, it fetches
the selected dance as JSON:

```text
https://www.ibiblio.org/contradance/thecallersbox/dance.php?id=<id>&format=JSON
```

## ContraDB Fetching

ContraDB search uses its public API:

```text
POST https://contradb.com/api/v1/dances
```

Full dance details are currently scraped from the public dance page:

```text
https://contradb.com/dances/<id>
```

## Current Terminology Pass

The first version does simple word-boundary replacements:

- `men`, `man`, `gents`, `gentlemen` -> `Larks`
- `ladies`, `lady`, `women`, `woman` -> `Robins`

This is deliberately conservative and easy to hand-edit after generation.
