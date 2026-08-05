//! Just enough Markdown to render what the coordinator actually writes.
//!
//! Answers arrive as Markdown and were being shown as source: `Love MI et al. 2014.
//! **Moderated estimation…**` with the asterisks visible. For this product that is not
//! cosmetic — citations and reports *are* the deliverable, and a citation the reader has to
//! mentally de-escape is a worse artifact than the web app's (plan §16).
//!
//! **Hand-written rather than a parser crate.** GPUI has no Markdown element, so the block
//! layer had to be built either way; the inline layer is then a few hundred lines against a
//! measured subset — emphasis, inline code, links, headings, lists, fenced code, tables,
//! blockquotes, nested lists — which is smaller than the API surface of a full CommonMark AST
//! and has no dependency to track. §16 recorded "if tables or nested structures start
//! mattering, reconsider"; both now do, and both fit. What would genuinely justify
//! `gpui-component` is a **nested block tree** — a list inside a quote, a table inside a list.
//! Nesting here is a `depth` number on a flat block, which renders those cases as their text
//! rather than their structure, and is the deliberate edge of this subset.
//!
//! Ranges produced here are **byte offsets into the block's text**, and GPUI asserts they
//! land on `char` boundaries — hence the care with multi-byte input throughout.

use std::ops::Range;

/// How a run of text should be drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Emphasis {
    Strong,
    Italic,
    Code,
    /// The visible text of a link.
    Link,
    /// A link's URL, kept beside the text because nothing is clickable yet — dropping it
    /// would lose the DOI in a citation, which is the part a researcher needs.
    Url,
}

/// A run of text plus the styled ranges inside it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inlines {
    pub text: String,
    pub styles: Vec<(Range<usize>, Emphasis)>,
}

/// Only the tests build `Inlines` by hand — the parser always produces them.
#[cfg(test)]
impl Inlines {
    fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            styles: Vec::new(),
        }
    }
}

/// One rendered block.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading { level: u8, inlines: Inlines },
    Paragraph(Inlines),
    /// A list item. `marker` is what to draw in the gutter — a bullet, or `3.`.
    ///
    /// `depth` is how deeply nested it is, counted from the *indents actually seen* in this
    /// list rather than by dividing spaces by two: agents write both two- and four-space
    /// nesting, sometimes in the same answer, and a fixed divisor renders one of them wrong.
    ListItem {
        marker: String,
        inlines: Inlines,
        depth: usize,
    },
    /// A blockquote. `depth` counts the `>` markers, so a quoted quote still reads as one.
    ///
    /// Consecutive quoted lines fold into a single block, the same way paragraph lines do —
    /// a quote containing several paragraphs collapses to one, which is a limit worth having
    /// over a nested block tree nothing in this app produces.
    Quote { depth: usize, inlines: Inlines },
    /// An image reference, alone on its line.
    ///
    /// The **path is not loaded**. It would have to be translated out of the distro's
    /// filesystem into one Windows can open, and no such translation exists in this direction
    /// (§46 records the three spellings). Figures the agent actually produced already appear
    /// beneath the answer, found by diffing the thread's output directory on the host (§42) —
    /// so this block exists to stop `![](…)` rendering as punctuation, not to duplicate them.
    Image { alt: String, url: String },
    /// A fenced code block, kept verbatim: no inline parsing inside code.
    Code { language: String, text: String },
    /// A pipe table. `header` may be empty when the source had no header row.
    ///
    /// Rows are ragged on purpose — a malformed table should render as the cells it does
    /// have, not vanish. Column *count* is settled by the widest row so nothing is
    /// silently dropped.
    Table {
        header: Vec<Inlines>,
        rows: Vec<Vec<Inlines>>,
    },
    Rule,
}

impl Block {
    /// Columns in a table, from its widest row. Zero for anything else.
    pub fn columns(&self) -> usize {
        match self {
            Block::Table { header, rows } => rows
                .iter()
                .map(Vec::len)
                .chain(std::iter::once(header.len()))
                .max()
                .unwrap_or(0),
            _ => 0,
        }
    }
}

/// Split Markdown into blocks.
pub fn parse(source: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut paragraph: Vec<&str> = Vec::new();
    let mut lines = source.lines().peekable();
    // The indents seen so far in the list being built, shallowest first. Its length is the
    // current depth. Emptied by anything that ends the list.
    let mut list_indents: Vec<usize> = Vec::new();

    while let Some(line) = lines.next() {
        let trimmed = line.trim_end();
        let start = trimmed.trim_start();
        let indent = indent_of(trimmed);

        // A fence swallows lines verbatim until it closes, so nothing inside a code block
        // is mistaken for a heading or a list.
        if let Some(language) = start.strip_prefix("```") {
            flush(&mut paragraph, &mut blocks);
            list_indents.clear();
            let mut body = Vec::new();
            while let Some(next) = lines.next() {
                if next.trim_start().starts_with("```") {
                    break;
                }
                body.push(next);
            }
            blocks.push(Block::Code {
                language: language.trim().to_string(),
                text: body.join("\n"),
            });
            continue;
        }

        if start.is_empty() {
            flush(&mut paragraph, &mut blocks);
            continue;
        }

        if is_rule(start) {
            flush(&mut paragraph, &mut blocks);
            list_indents.clear();
            blocks.push(Block::Rule);
            continue;
        }

        // A blockquote runs until a line that is not quoted. Folded like a paragraph, because
        // the coordinator quotes a sentence or two — never a document.
        if let Some((depth, first)) = quote_line(start) {
            flush(&mut paragraph, &mut blocks);
            list_indents.clear();
            let mut body = vec![first.to_string()];
            while let Some(next) = lines.peek() {
                match quote_line(next.trim()) {
                    // A `>` on its own ends the paragraph inside the quote; treat it as a
                    // space rather than starting a second block.
                    Some((_, rest)) => {
                        body.push(rest.to_string());
                        lines.next();
                    }
                    None => break,
                }
            }
            blocks.push(Block::Quote {
                depth,
                inlines: parse_inlines(body.join(" ").trim()),
            });
            continue;
        }

        // An image on a line of its own. Inside a sentence it is handled inline instead.
        if let Some((alt, url)) = lone_image(start) {
            flush(&mut paragraph, &mut blocks);
            list_indents.clear();
            blocks.push(Block::Image { alt, url });
            continue;
        }

        if let Some((level, rest)) = heading(start) {
            flush(&mut paragraph, &mut blocks);
            list_indents.clear();
            blocks.push(Block::Heading {
                level,
                inlines: parse_inlines(rest),
            });
            continue;
        }

        // A table is recognised by its **separator** row (`|---|---|`), not by pipes
        // alone: prose about `a | b` is far commoner than a one-line table, and treating
        // every pipe as a cell boundary would shred ordinary sentences. That means
        // looking one line ahead, which is why `lines` is peekable.
        if looks_like_row(start)
            && lines
                .peek()
                .is_some_and(|next| is_table_separator(next.trim()))
        {
            flush(&mut paragraph, &mut blocks);
            list_indents.clear();
            let header = table_row(start);
            lines.next(); // the separator itself carries only alignment, which we ignore
            let mut rows = Vec::new();
            while let Some(next) = lines.peek() {
                let candidate = next.trim();
                if !looks_like_row(candidate) {
                    break;
                }
                rows.push(table_row(candidate));
                lines.next();
            }
            blocks.push(Block::Table { header, rows });
            continue;
        }

        if let Some((marker, rest)) = list_item(start) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::ListItem {
                marker,
                inlines: parse_inlines(rest),
                depth: nest(&mut list_indents, indent),
            });
            continue;
        }

        // Prose ends a list. An indented paragraph is a continuation in CommonMark, but
        // treating it as one here would mean carrying a depth across a blank line and
        // guessing which item it belonged to — predictable is worth more.
        list_indents.clear();
        paragraph.push(trimmed);
    }
    flush(&mut paragraph, &mut blocks);
    blocks
}

/// How far a line is indented, counting a tab as four columns.
///
/// Tabs because agents emit them and a tab measured as one column would sort a tab-indented
/// child *above* a two-space one.
fn indent_of(line: &str) -> usize {
    line.chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

/// Depth for a list item at `indent`, updating the stack of indents seen.
///
/// Deeper than the current level opens one; shallower closes as many as it has to. This is
/// what makes two-space and four-space nesting render the same, and it cannot invent a level
/// the source did not indent for.
fn nest(indents: &mut Vec<usize>, indent: usize) -> usize {
    while indents.last().is_some_and(|&open| indent < open) {
        indents.pop();
    }
    if indents.last().is_none_or(|&open| indent > open) {
        indents.push(indent);
    }
    indents.len().saturating_sub(1)
}

/// A quoted line: how many `>` deep, and what follows them.
fn quote_line(line: &str) -> Option<(usize, &str)> {
    let mut rest = line.strip_prefix('>')?;
    let mut depth = 1;
    loop {
        let trimmed = rest.trim_start();
        match trimmed.strip_prefix('>') {
            Some(next) => {
                depth += 1;
                rest = next;
            }
            None => return Some((depth, trimmed)),
        }
    }
}

/// `![alt](url)` occupying a whole line, and nothing else.
///
/// Whole-line only: mid-sentence it is inline text, and promoting it to a block there would
/// tear the sentence in half.
fn lone_image(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("![")?;
    let close = rest.find("](")?;
    let url_end = rest[close + 2..].find(')')?;
    // Anything after the closing paren means it is part of a sentence.
    if rest[close + 2 + url_end + 1..].trim().is_empty() {
        return Some((
            rest[..close].to_string(),
            rest[close + 2..close + 2 + url_end].to_string(),
        ));
    }
    None
}

/// Whether a line could be a table row: it has a pipe that is not escaped.
fn looks_like_row(line: &str) -> bool {
    let mut previous = ' ';
    for c in line.chars() {
        if c == '|' && previous != '\\' {
            return true;
        }
        previous = c;
    }
    false
}

/// Whether a line is the `|---|:---:|` rule under a header.
///
/// This is the whole basis for calling something a table, so it is strict: every cell must
/// be dashes with optional alignment colons, and there must be at least one.
fn is_table_separator(line: &str) -> bool {
    let cells = split_row(line);
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let cell = cell.trim();
            let body = cell.trim_start_matches(':').trim_end_matches(':');
            !body.is_empty() && body.chars().all(|c| c == '-')
        })
}

/// Split one row into its cells, then parse each for emphasis.
fn table_row(line: &str) -> Vec<Inlines> {
    split_row(line)
        .into_iter()
        .map(|cell| parse_inlines(cell.trim()))
        .collect()
}

/// Cut a row on unescaped pipes, dropping the optional leading and trailing ones.
///
/// `\|` is how a literal pipe is written inside a cell — which matters here, because the
/// agent writes regular expressions and shell commands into tables.
fn split_row(line: &str) -> Vec<String> {
    let line = line.trim();
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for c in line.chars() {
        match c {
            '\\' if !escaped => escaped = true,
            '|' if !escaped => cells.push(std::mem::take(&mut current)),
            other => {
                if escaped && other != '|' {
                    // Not an escape we know: keep the backslash as typed.
                    current.push('\\');
                }
                escaped = false;
                current.push(other);
            }
        }
    }
    if escaped {
        current.push('\\');
    }
    cells.push(current);

    // `| a | b |` yields an empty cell at each end; `a | b` yields neither.
    if line.starts_with('|') && !cells.is_empty() {
        cells.remove(0);
    }
    if line.ends_with('|') && !line.ends_with("\\|") {
        cells.pop();
    }
    cells
}

/// Join the pending lines into a paragraph.
///
/// Markdown folds a single newline into a space, and the coordinator relies on that: it
/// wraps citations across lines and ends lines with two spaces for a hard break, which we
/// treat the same way — one paragraph, wrapped by the layout rather than by the source.
fn flush(paragraph: &mut Vec<&str>, blocks: &mut Vec<Block>) {
    if paragraph.is_empty() {
        return;
    }
    let joined = paragraph
        .iter()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join(" ");
    paragraph.clear();
    if !joined.is_empty() {
        blocks.push(Block::Paragraph(parse_inlines(&joined)));
    }
}

fn is_rule(line: &str) -> bool {
    let stripped: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    stripped.len() >= 3
        && (stripped.chars().all(|c| c == '-')
            || stripped.chars().all(|c| c == '*')
            || stripped.chars().all(|c| c == '_'))
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = line[hashes..].strip_prefix(' ')?;
    Some((hashes as u8, rest))
}

fn list_item(line: &str) -> Option<(String, &str)> {
    for bullet in ["- ", "* ", "• "] {
        if let Some(rest) = line.strip_prefix(bullet) {
            return Some(("·".to_string(), rest));
        }
    }
    // `1.` / `12)` — keep the author's own number rather than renumbering.
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits <= 3 {
        let rest = &line[digits..];
        for separator in [". ", ") "] {
            if let Some(rest) = rest.strip_prefix(separator) {
                return Some((format!("{}.", &line[..digits]), rest));
            }
        }
    }
    None
}

/// Strip inline markers, recording where the emphasis applied.
pub fn parse_inlines(source: &str) -> Inlines {
    let mut out = Inlines::default();
    let bytes = source.as_bytes();
    let mut at = 0;

    while at < source.len() {
        // Code first: nothing inside a span of backticks is emphasis.
        if bytes[at] == b'`' {
            if let Some(end) = source[at + 1..].find('`') {
                let inner = &source[at + 1..at + 1 + end];
                push(&mut out, inner, Some(Emphasis::Code));
                at += end + 2;
                continue;
            }
        }
        // `**strong**` before `*italic*`, or the first two asterisks read as empty italic.
        if source[at..].starts_with("**") {
            if let Some(end) = source[at + 2..].find("**") {
                let inner = &source[at + 2..at + 2 + end];
                if !inner.is_empty() {
                    push_nested(&mut out, inner, Emphasis::Strong);
                    at += end + 4;
                    continue;
                }
            }
        }
        if bytes[at] == b'*' || bytes[at] == b'_' {
            let marker = bytes[at] as char;
            if let Some(end) = source[at + 1..].find(marker) {
                let inner = &source[at + 1..at + 1 + end];
                // `snake_case` must not become italics, so `_` only opens when the run
                // does not butt against a word character.
                let boundary_ok = marker == '*'
                    || (!preceded_by_word(source, at) && !inner.starts_with(char::is_alphanumeric)
                        || !inner.is_empty() && inner.ends_with(' '));
                if !inner.is_empty() && (marker == '*' || boundary_ok) {
                    push_nested(&mut out, inner, Emphasis::Italic);
                    at += end + 2;
                    continue;
                }
            }
        }
        // `![alt](url)` inside a sentence. Handled before `[` so the `!` is dropped rather
        // than left stranded in front of the text — which is what it used to do, because the
        // prose scan stopped at the bracket and pushed the bang as ordinary text.
        if bytes[at] == b'!' && source[at + 1..].starts_with('[') {
            if let Some((alt, url)) = inline_image(&source[at..]) {
                let consumed = alt.len() + url.len() + 5; // `!`, `[`, `](`, `)`
                push(&mut out, &alt, Some(Emphasis::Link));
                if !url.is_empty() {
                    push(&mut out, " ", None);
                    push(&mut out, &url, Some(Emphasis::Url));
                }
                at += consumed;
                continue;
            }
        }
        // `[text](url)` — the text is styled, the URL kept beside it.
        if bytes[at] == b'[' {
            if let Some(close) = source[at..].find("](") {
                let after = at + close + 2;
                if let Some(end) = source[after..].find(')') {
                    let text = &source[at + 1..at + close];
                    let url = &source[after..after + end];
                    push(&mut out, text, Some(Emphasis::Link));
                    if !url.is_empty() && url != text {
                        push(&mut out, " ", None);
                        push(&mut out, url, Some(Emphasis::Url));
                    }
                    at = after + end + 1;
                    continue;
                }
            }
        }

        // Ordinary text: take everything up to the next possible marker.
        //
        // Step over one whole *character*, not one byte. `at + 1` lands inside `á` when the
        // run starts with a multi-byte character, and slicing there panics — which is what
        // the boundary test caught. Every earlier branch is safe because it only advances
        // past an ASCII marker.
        let after = at + source[at..].chars().next().map_or(1, char::len_utf8);
        let next = source[after..]
            .find(['`', '*', '_', '[', '!'])
            .map(|offset| after + offset)
            .unwrap_or(source.len());
        push(&mut out, &source[at..next], None);
        at = next;
    }
    out
}

/// `![alt](url)` at the start of `source`, if it is there.
fn inline_image(source: &str) -> Option<(String, String)> {
    let rest = source.strip_prefix("![")?;
    let close = rest.find("](")?;
    let url_end = rest[close + 2..].find(')')?;
    Some((
        rest[..close].to_string(),
        rest[close + 2..close + 2 + url_end].to_string(),
    ))
}

fn preceded_by_word(source: &str, at: usize) -> bool {
    source[..at]
        .chars()
        .next_back()
        .is_some_and(char::is_alphanumeric)
}

fn push(out: &mut Inlines, text: &str, emphasis: Option<Emphasis>) {
    if text.is_empty() {
        return;
    }
    let start = out.text.len();
    out.text.push_str(text);
    if let Some(emphasis) = emphasis {
        out.styles.push((start..out.text.len(), emphasis));
    }
}

/// Add `inner` under `emphasis`, keeping any emphasis found *inside* it.
///
/// So `**bold with *italic* inside**` keeps both, rather than the inner markers showing up
/// as literal asterisks.
fn push_nested(out: &mut Inlines, inner: &str, emphasis: Emphasis) {
    let start = out.text.len();
    let nested = parse_inlines(inner);
    out.text.push_str(&nested.text);
    out.styles.push((start..out.text.len(), emphasis));
    for (range, nested_emphasis) in nested.styles {
        out.styles
            .push((start + range.start..start + range.end, nested_emphasis));
    }
}

#[cfg(test)]
mod tables {
    use super::*;

    fn table(source: &str) -> (Vec<Inlines>, Vec<Vec<Inlines>>) {
        match parse(source).into_iter().find(|b| matches!(b, Block::Table { .. })) {
            Some(Block::Table { header, rows }) => (header, rows),
            other => panic!("expected a table, got {other:?}"),
        }
    }

    /// Just the list items, as `(marker, text, depth)`.
    fn items(source: &str) -> Vec<(String, String, usize)> {
        parse(source)
            .into_iter()
            .filter_map(|block| match block {
                Block::ListItem {
                    marker,
                    inlines,
                    depth,
                } => Some((marker, inlines.text, depth)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn two_space_and_four_space_nesting_produce_the_same_shape() {
        // The reason depth is a stack of the indents actually seen rather than spaces
        // divided by two: agents emit both, and a fixed divisor renders one of them flat or
        // twice as deep as written (docs §65).
        let two = items("- top\n  - child\n    - grandchild\n- second");
        let four = items("- top\n    - child\n        - grandchild\n- second");
        let depths: Vec<usize> = two.iter().map(|(_, _, depth)| *depth).collect();
        assert_eq!(depths, vec![0, 1, 2, 0]);
        assert_eq!(depths, four.iter().map(|(_, _, d)| *d).collect::<Vec<_>>());
        assert_eq!(two[1].1, "child");
    }

    #[test]
    fn a_tab_indents_as_deeply_as_the_spaces_beside_it() {
        // A tab counted as one column would sort a tab-indented child *above* a two-space
        // one, putting it at the wrong level in a list that mixes them.
        let mixed = items("- top\n\t- tabbed\n- back");
        assert_eq!(
            mixed.iter().map(|(_, _, d)| *d).collect::<Vec<_>>(),
            vec![0, 1, 0]
        );
    }

    #[test]
    fn closing_a_nested_level_returns_to_the_one_it_came_from() {
        let list = items("- a\n  - b\n    - c\n  - d\n- e");
        assert_eq!(
            list.iter().map(|(_, _, d)| *d).collect::<Vec<_>>(),
            vec![0, 1, 2, 1, 0]
        );
    }

    #[test]
    fn a_numbered_item_keeps_its_own_number_when_nested() {
        // Renumbering, or swapping in a bullet because it happens to be indented, would
        // change what the answer says — steps are often referred to by number.
        let list = items("1. first\n   2) second\n");
        assert_eq!(list[0].0, "1.");
        assert_eq!(list[1].0, "2.");
        assert_eq!(list[1].2, 1);
    }

    #[test]
    fn prose_between_two_lists_starts_the_second_one_over() {
        let blocks = parse("- a\n  - b\n\nSome prose.\n\n- c");
        let depths: Vec<usize> = blocks
            .iter()
            .filter_map(|block| match block {
                Block::ListItem { depth, .. } => Some(*depth),
                _ => None,
            })
            .collect();
        assert_eq!(depths, vec![0, 1, 0], "{blocks:?}");
    }

    #[test]
    fn a_blockquote_folds_its_lines_and_counts_its_markers() {
        let blocks = parse("> Moderated estimation of fold change\n> and dispersion.\n\nAfter.");
        assert_eq!(
            blocks[0],
            Block::Quote {
                depth: 1,
                inlines: Inlines::plain("Moderated estimation of fold change and dispersion."),
            }
        );
        // The quote ends where the quoting stops.
        assert_eq!(blocks[1], Block::Paragraph(Inlines::plain("After.")));
    }

    #[test]
    fn a_quoted_quote_is_deeper_but_still_one_block() {
        let blocks = parse(">> twice removed");
        assert_eq!(
            blocks[0],
            Block::Quote {
                depth: 2,
                inlines: Inlines::plain("twice removed"),
            }
        );
    }

    #[test]
    fn emphasis_inside_a_quote_is_still_parsed() {
        match &parse("> see **table 2**")[0] {
            Block::Quote { inlines, .. } => {
                assert_eq!(inlines.text, "see table 2");
                assert_eq!(inlines.styles, vec![(4..11, Emphasis::Strong)]);
            }
            other => panic!("expected a quote, got {other:?}"),
        }
    }

    #[test]
    fn an_image_on_its_own_line_becomes_a_block() {
        assert_eq!(
            parse("![Yield by cultivar](outputs/yield.png)")[0],
            Block::Image {
                alt: "Yield by cultivar".into(),
                url: "outputs/yield.png".into(),
            }
        );
    }

    #[test]
    fn an_image_inside_a_sentence_stays_in_the_sentence() {
        // Promoting it would tear the sentence in half. What it must not do is leave the
        // `!` stranded in front of the text, which is what happened before: the prose scan
        // stopped at the bracket and pushed the bang as ordinary text (docs §65).
        let blocks = parse("See ![the plot](p.png) for detail.");
        match &blocks[0] {
            Block::Paragraph(inlines) => {
                assert!(!inlines.text.contains('!'), "{:?}", inlines.text);
                assert!(inlines.text.starts_with("See the plot"), "{:?}", inlines.text);
                assert!(inlines.text.ends_with("for detail."), "{:?}", inlines.text);
                assert!(inlines.text.contains("p.png"), "the path is kept");
            }
            other => panic!("expected a paragraph, got {other:?}"),
        }
        assert_eq!(blocks.len(), 1, "{blocks:?}");
    }

    #[test]
    fn a_bang_that_is_not_an_image_is_left_alone() {
        // `!` is ordinary punctuation far more often than it is a marker, and adding it to
        // the scan set means every one of them now reaches that branch.
        for source in ["Done! Next?", "Careful! [see here](x)", "a ! b", "!"] {
            let blocks = parse(source);
            match &blocks[0] {
                Block::Paragraph(inlines) => {
                    assert!(inlines.text.contains('!'), "{source}: {:?}", inlines.text)
                }
                other => panic!("{source}: expected a paragraph, got {other:?}"),
            }
        }
    }

    #[test]
    fn renders_the_shape_a_report_subagent_emits() {
        let (header, rows) = table(
            "| Cultivar | Yield | Notes |\n\
             |---|---:|:--|\n\
             | Amarilis | 32 t/ha | **resistant** |\n\
             | Yungay | 28 t/ha | susceptible |",
        );
        assert_eq!(
            header.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(),
            ["Cultivar", "Yield", "Notes"]
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0].text, "Amarilis");
        // Cells are still Markdown — a bold verdict in a results table is the norm.
        assert_eq!(rows[0][2].text, "resistant");
        assert_eq!(rows[0][2].styles.len(), 1);
    }

    #[test]
    fn ordinary_prose_with_a_pipe_is_not_a_table() {
        // The reason a table needs its separator row to be recognised: the coordinator
        // writes about shell pipelines and alternatives constantly, and treating every
        // pipe as a cell boundary would shred those sentences into columns.
        for source in [
            "Run `asta search | head -5` to see the first few.",
            "Either main | develop works here.",
            "| this line has pipes but no rule under it |",
        ] {
            assert!(
                !parse(source).iter().any(|b| matches!(b, Block::Table { .. })),
                "{source:?} must stay prose"
            );
            assert!(parse(source).iter().any(|b| matches!(
                b,
                Block::Paragraph(_) | Block::ListItem { .. }
            )));
        }
    }

    #[test]
    fn a_ragged_table_keeps_every_cell_it_has() {
        // Streaming means half-written tables are on screen constantly, and a truncated
        // row must not make the whole block disappear.
        let (header, rows) = table("| a | b | c |\n|---|---|---|\n| 1 | 2 |\n| 1 | 2 | 3 | 4 |");
        assert_eq!(header.len(), 3);
        assert_eq!(rows[0].len(), 2, "short row keeps what it has");
        assert_eq!(rows[1].len(), 4, "long row is not truncated");
        // Column count comes from the widest row, so nothing is silently dropped.
        let block = Block::Table { header, rows };
        assert_eq!(block.columns(), 4);
    }

    #[test]
    fn an_escaped_pipe_stays_inside_its_cell() {
        // The agent writes regular expressions and shell commands into tables.
        let (_, rows) = table("| what | how |\n|---|---|\n| filter | `grep -E 'a\\|b'` |");
        assert_eq!(rows[0].len(), 2, "{:?}", rows[0]);
        assert!(rows[0][1].text.contains("a|b"), "{:?}", rows[0][1].text);
    }

    #[test]
    fn a_table_without_surrounding_pipes_still_parses() {
        // GitHub-style tables often omit the outer pipes, and models emit both forms.
        let (header, rows) = table("a | b\n--- | ---\n1 | 2");
        assert_eq!(header.len(), 2);
        assert_eq!(rows[0][1].text, "2");
    }

    #[test]
    fn a_table_ends_where_the_prose_resumes() {
        let blocks = parse("| a |\n|---|\n| 1 |\n\nAnd then a sentence.");
        assert!(matches!(blocks[0], Block::Table { .. }));
        match &blocks[1] {
            Block::Paragraph(inlines) => assert_eq!(inlines.text, "And then a sentence."),
            other => panic!("expected the prose back, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inline(source: &str) -> (String, Vec<(&str, Emphasis)>) {
        let parsed = parse_inlines(source);
        let styles = parsed
            .styles
            .iter()
            .map(|(range, emphasis)| {
                (
                    // Leaked so the test can hold &str without cloning gymnastics.
                    Box::leak(parsed.text[range.clone()].to_string().into_boxed_str()) as &str,
                    *emphasis,
                )
            })
            .collect();
        (parsed.text, styles)
    }

    #[test]
    fn strips_the_markers_the_coordinator_actually_writes() {
        // Measured output from a real turn (plan §15c).
        let source = "Love MI, Huber W, Anders S. 2014. **Moderated estimation of fold change \
                      and dispersion for RNA-seq data with DESeq2.** *Genome Biology* 15:550. \
                      DOI: **10.1186/s13059-014-0550-8**";
        let (text, styles) = inline(source);
        assert!(!text.contains('*'), "{text}");
        assert!(text.starts_with("Love MI, Huber W, Anders S. 2014. Moderated"));
        assert_eq!(styles.len(), 3);
        assert_eq!(styles[1], ("Genome Biology", Emphasis::Italic));
        assert_eq!(styles[2], ("10.1186/s13059-014-0550-8", Emphasis::Strong));
    }

    #[test]
    fn keeps_inline_code_verbatim() {
        let (text, styles) = inline("run `python3 -m pytest` first");
        assert_eq!(text, "run python3 -m pytest first");
        assert_eq!(styles, vec![("python3 -m pytest", Emphasis::Code)]);
        // Emphasis markers *inside* code are content, not markup.
        let (text, styles) = inline("`a * b`");
        assert_eq!(text, "a * b");
        assert_eq!(styles, vec![("a * b", Emphasis::Code)]);
    }

    #[test]
    fn keeps_a_links_url_beside_its_text() {
        // Dropping the URL would lose the DOI, which is the part that matters in a citation.
        let (text, styles) = inline("see [the paper](https://doi.org/10.1186/x)");
        assert_eq!(text, "see the paper https://doi.org/10.1186/x");
        assert_eq!(styles[0], ("the paper", Emphasis::Link));
        assert_eq!(styles[1], ("https://doi.org/10.1186/x", Emphasis::Url));
    }

    #[test]
    fn does_not_italicise_snake_case() {
        // `read_file` and `write_file` in one sentence would otherwise become italics.
        let (text, styles) = inline("call read_file then write_file");
        assert_eq!(text, "call read_file then write_file");
        assert!(styles.is_empty(), "{styles:?}");
    }

    #[test]
    fn nests_emphasis_instead_of_leaking_markers() {
        let (text, styles) = inline("**bold with *italic* inside**");
        assert_eq!(text, "bold with italic inside");
        assert!(styles.contains(&("bold with italic inside", Emphasis::Strong)));
        assert!(styles.contains(&("italic", Emphasis::Italic)));
    }

    #[test]
    fn every_style_range_lands_on_a_char_boundary() {
        // GPUI asserts this, so multi-byte text must not be sliced mid-character.
        let source = "**á é í**, *ñ* and `ü` — plus [más](https://example.com/qué)";
        let parsed = parse_inlines(source);
        for (range, _) in &parsed.styles {
            assert!(parsed.text.is_char_boundary(range.start), "{range:?}");
            assert!(parsed.text.is_char_boundary(range.end), "{range:?}");
        }
        assert!(parsed.text.contains("á é í"));
    }

    #[test]
    fn splits_blocks_and_keeps_code_fences_whole() {
        let blocks = parse(
            "## Findings\n\nFirst line\nsecond line\n\n- one\n- two\n3. three\n\n\
             ```python\nx = 1  # not a *heading*\n```\n\n---\n",
        );
        assert_eq!(
            blocks[0],
            Block::Heading {
                level: 2,
                inlines: Inlines::plain("Findings")
            }
        );
        // A single newline folds into a space; the layout does the wrapping.
        assert_eq!(
            blocks[1],
            Block::Paragraph(Inlines::plain("First line second line"))
        );
        assert_eq!(
            blocks[2],
            Block::ListItem {
                marker: "·".into(),
                inlines: Inlines::plain("one"),
                depth: 0
            }
        );
        assert_eq!(
            blocks[4],
            Block::ListItem {
                marker: "3.".into(),
                inlines: Inlines::plain("three"),
                depth: 0
            }
        );
        assert_eq!(
            blocks[5],
            Block::Code {
                language: "python".into(),
                text: "x = 1  # not a *heading*".into()
            }
        );
        assert_eq!(blocks[6], Block::Rule);
    }

    #[test]
    fn plain_text_survives_untouched() {
        // The common case: an answer with no markup at all must not be reshaped.
        let blocks = parse("Just a sentence.");
        assert_eq!(blocks, vec![Block::Paragraph(Inlines::plain("Just a sentence."))]);
        assert!(parse("").is_empty());
    }

    #[test]
    fn an_unclosed_marker_is_shown_as_typed() {
        // Streaming means we render half-written markup constantly; it must not vanish or
        // swallow the rest of the line.
        let (text, styles) = inline("this is **half written");
        assert_eq!(text, "this is **half written");
        assert!(styles.is_empty());
    }
}
