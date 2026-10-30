use super::*;
use crate::model::ansi::Handler;

const RED: HighlightColor = HighlightColor::Ansi(9);
const YELLOW: HighlightColor = HighlightColor::Ansi(3);

fn matcher(pattern: &str, target: HighlightTarget) -> HighlightMatcher {
    HighlightMatcher {
        rules: vec![
            HighlightRule::new(
                pattern,
                true,
                target,
                HighlightStyle {
                    foreground: Some(RED),
                    background: None,
                },
            )
            .unwrap(),
        ],
        prompt_header: None,
    }
}

fn grid_text(text: &str, columns: usize) -> GridHandler {
    let mut grid = GridHandler::new_for_test(4, columns);
    for character in text.chars() {
        grid.input(character);
    }
    grid
}

#[test]
fn highlights_every_nonempty_match() {
    let result =
        matcher("ERROR|$", HighlightTarget::Text).find(&LogicalLine::from_text("ERROR x ERROR"));
    assert_eq!(
        result.spans,
        vec![
            HighlightSpan {
                cells: 0..5,
                style: HighlightStyle {
                    foreground: Some(RED),
                    background: None
                }
            },
            HighlightSpan {
                cells: 8..13,
                style: HighlightStyle {
                    foreground: Some(RED),
                    background: None
                }
            },
        ]
    );
}

#[test]
fn first_rule_preserves_unspecified_ansi_properties() {
    let mut matcher = matcher("ERROR", HighlightTarget::Text);
    matcher.rules.push(
        HighlightRule::new(
            "ERROR",
            true,
            HighlightTarget::Line,
            HighlightStyle {
                foreground: None,
                background: Some(YELLOW),
            },
        )
        .unwrap(),
    );
    let result = matcher.find(&LogicalLine::from_text("x ERROR y"));
    assert_eq!(
        result.spans[1],
        HighlightSpan {
            cells: 2..7,
            style: HighlightStyle {
                foreground: Some(RED),
                background: None
            },
        }
    );
}

#[test]
fn matches_across_soft_wrap() {
    let grid = grid_text("ERROR", 3);
    let snapshot = snapshot_lines(&grid, 1..2);
    assert_eq!(snapshot.lines[0].line.text, "ERROR");
    assert_eq!(snapshot.lines[0].start_row, 0);
    let found = matcher("ERROR", HighlightTarget::Text).find(&snapshot.lines[0].line);
    assert_eq!(
        grid_range(&found.spans[0].cells, 0, 3),
        Point::new(0, 0)..=Point::new(1, 1)
    );
}

#[test]
fn line_target_includes_empty_columns_after_soft_wrap() {
    let grid = grid_text("ERROR", 3);
    let snapshot = snapshot_lines(&grid, 0..2);
    let found = matcher("ERROR", HighlightTarget::Line).find(&snapshot.lines[0].line);
    assert_eq!(found.spans[0].cells, 0..6);
}

#[test]
fn wide_char_match_includes_spacer_cells() {
    let grid = grid_text("a界b", 8);
    let snapshot = snapshot_lines(&grid, 0..1);
    let found = matcher("界", HighlightTarget::Text).find(&snapshot.lines[0].line);
    assert_eq!(snapshot.lines[0].line.text, "a界b");
    assert_eq!(found.spans[0].cells, 1..3);
}

#[test]
fn combining_mark_match_colors_entire_grapheme() {
    let grid = grid_text("e\u{301}x", 8);
    let snapshot = snapshot_lines(&grid, 0..1);
    let found = matcher("\u{301}", HighlightTarget::Text).find(&snapshot.lines[0].line);
    assert_eq!(found.spans[0].cells, 0..1);
}

#[test]
fn recognizes_prompt_only_at_start_of_line() {
    let matcher = HighlightMatcher {
        rules: Vec::new(),
        prompt_header: Some(PromptHeaderRule {
            regex: compile_pattern("user@host:~\\$ ", true).unwrap(),
            style: HighlightStyle {
                foreground: Some(RED),
                background: Some(YELLOW),
            },
            separator: Some(RED),
        }),
    };
    let prompt = matcher.find(&LogicalLine::from_text("user@host:~$ echo hello"));
    assert_eq!(prompt.prompt, Some(0..13));
    assert_eq!(prompt.spans[1].style.foreground, None);
    assert!(
        matcher
            .find(&LogicalLine::from_text("x user@host:~$ "))
            .prompt
            .is_none()
    );
}

#[test]
fn caps_matching_work_for_repeated_matches() {
    let found =
        matcher("a", HighlightTarget::Text).find(&LogicalLine::from_text(&"a ".repeat(600)));
    assert!(found.limited);
    assert_eq!(found.spans.len(), 512);
}

#[test]
fn rejects_unsupported_regex_syntax() {
    assert!(compile_pattern("(?<=x)y", true).is_err());
    assert!(compile_pattern(r"(x)\1", true).is_err());
}

#[test]
fn validates_color_names_and_rgb_boundaries() {
    assert_eq!(
        HighlightColor::parse("#aB00FF", false),
        Some(HighlightColor::Rgb([171, 0, 255]))
    );
    assert_eq!(HighlightColor::parse("ansi:bright_red", false), Some(RED));
    assert_eq!(HighlightColor::parse("#éFFFF", false), None);
    assert_eq!(HighlightColor::parse("theme:accent", false), None);
}

#[test]
fn preserves_written_trailing_spaces_without_grid_padding() {
    let grid = grid_text("prompt$ ", 80);
    let snapshot = snapshot_lines(&grid, 0..1);
    assert_eq!(snapshot.lines[0].line.text, "prompt$ ");
}

#[test]
fn skips_an_entire_oversized_logical_line() {
    let grid = grid_text(&"a".repeat(MAX_LINE_BYTES + 1), MAX_LINE_BYTES + 2);
    let snapshot = snapshot_lines(&grid, 0..1);
    assert!(snapshot.lines.is_empty());
    assert_eq!(snapshot.skipped_lines, 1);
}

#[test]
fn rewritten_text_does_not_keep_an_old_match() {
    let mut grid = grid_text("ERROR", 16);
    let matcher = matcher("ERROR", HighlightTarget::Text);
    let before = snapshot_lines(&grid, 0..1);
    assert_eq!(matcher.find(&before.lines[0].line).spans.len(), 1);

    grid.carriage_return();
    grid.clear_line(crate::model::ansi::LineClearMode::All);
    grid.input('O');
    grid.input('K');

    let after = snapshot_lines(&grid, 0..1);
    assert_eq!(after.lines[0].line.text, "OK");
    assert!(matcher.find(&after.lines[0].line).spans.is_empty());
}

#[test]
fn hard_newlines_do_not_join_matches() {
    let mut grid = grid_text("ER", 16);
    grid.carriage_return();
    grid.linefeed();
    grid.input('R');
    grid.input('O');
    grid.input('R');

    let snapshot = snapshot_lines(&grid, 0..2);
    assert_eq!(snapshot.lines[0].line.text, "ER");
    assert_eq!(snapshot.lines[1].line.text, "ROR");
    assert!(
        matcher("ERROR", HighlightTarget::Text)
            .find(&snapshot.lines[0].line)
            .spans
            .is_empty()
    );
}

#[test]
fn preview_graphemes_use_terminal_cell_widths() {
    let line = LogicalLine::from_text("e\u{301}界");
    let found = matcher("\u{301}界", HighlightTarget::Text).find(&line);
    assert_eq!(found.spans[0].cells, 0..3);
    assert_eq!(line.text_for_cells(0..3), "e\u{301}界");
}
