use std::ops::{Range, RangeInclusive};

use rangemap::RangeMap;
use regex::{Regex, RegexBuilder};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::char_or_str::PushCharOrStr;
use super::grid::cell::{Flags, LineLength};
use super::grid::grid_handler::GridHandler;
use super::grid::{CellType, Dimensions};
use super::index::Point;

pub const MAX_RULES: usize = 64;
pub const MAX_PATTERN_BYTES: usize = 4096;
pub const MAX_LINE_BYTES: usize = 16 * 1024;
pub const MAX_LINE_ROWS: usize = 512;
pub const MAX_MATCHES: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeColor {
    Surface,
    Foreground,
    Outline,
    Accent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HighlightColor {
    Rgb([u8; 3]),
    Ansi(u8),
    Theme(ThemeColor),
}

impl HighlightColor {
    pub fn parse(value: &str, allow_theme: bool) -> Option<Self> {
        const ANSI_NAMES: [&str; 16] = [
            "black",
            "red",
            "green",
            "yellow",
            "blue",
            "magenta",
            "cyan",
            "white",
            "bright_black",
            "bright_red",
            "bright_green",
            "bright_yellow",
            "bright_blue",
            "bright_magenta",
            "bright_cyan",
            "bright_white",
        ];
        if let Some(hex) = value.strip_prefix('#') {
            if hex.len() != 6 || !hex.is_ascii() {
                return None;
            }
            return Some(Self::Rgb([
                u8::from_str_radix(&hex[..2], 16).ok()?,
                u8::from_str_radix(&hex[2..4], 16).ok()?,
                u8::from_str_radix(&hex[4..], 16).ok()?,
            ]));
        }
        if let Some(name) = value.strip_prefix("ansi:") {
            return ANSI_NAMES
                .iter()
                .position(|entry| *entry == name)
                .map(|index| Self::Ansi(index as u8));
        }
        if allow_theme {
            return Some(Self::Theme(match value {
                "theme:surface_2" => ThemeColor::Surface,
                "theme:foreground" => ThemeColor::Foreground,
                "theme:outline" => ThemeColor::Outline,
                "theme:accent" => ThemeColor::Accent,
                _ => return None,
            }));
        }
        None
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HighlightStyle {
    pub foreground: Option<HighlightColor>,
    pub background: Option<HighlightColor>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HighlightTarget {
    Text,
    Line,
}

pub struct HighlightRule {
    regex: Regex,
    target: HighlightTarget,
    style: HighlightStyle,
}

pub fn compile_pattern(pattern: &str, case_sensitive: bool) -> Result<Regex, String> {
    if pattern.is_empty() {
        return Err("Enter a regular expression.".into());
    }
    if pattern.len() > MAX_PATTERN_BYTES {
        return Err("Patterns must contain at most 4096 UTF-8 bytes.".into());
    }
    RegexBuilder::new(pattern)
        .case_insensitive(!case_sensitive)
        .size_limit(1024 * 1024)
        .build()
        .map_err(|error| error.to_string())
}

impl HighlightRule {
    pub fn new(
        pattern: &str,
        case_sensitive: bool,
        target: HighlightTarget,
        style: HighlightStyle,
    ) -> Result<Self, String> {
        Ok(Self {
            regex: compile_pattern(pattern, case_sensitive)?,
            target,
            style,
        })
    }
}

pub struct PromptHeaderRule {
    pub regex: Regex,
    pub style: HighlightStyle,
    pub separator: Option<HighlightColor>,
}

#[derive(Default)]
pub struct HighlightMatcher {
    pub rules: Vec<HighlightRule>,
    pub prompt_header: Option<PromptHeaderRule>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct GraphemeCells {
    bytes: Range<usize>,
    cells: Range<usize>,
}

/// Text and cell coordinates relative to the beginning of one logical line.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LogicalLine {
    pub text: String,
    pub columns: usize,
    pub rows: usize,
    graphemes: Vec<GraphemeCells>,
}

impl LogicalLine {
    pub fn from_text(text: &str) -> Self {
        let mut graphemes = Vec::new();
        let mut column = 0;
        for (start, grapheme) in text.grapheme_indices(true) {
            let width = grapheme.width().max(1);
            graphemes.push(GraphemeCells {
                bytes: start..start + grapheme.len(),
                cells: column..column + width,
            });
            column += width;
        }
        Self {
            text: text.into(),
            columns: column.max(1),
            rows: 1,
            graphemes,
        }
    }

    pub fn estimated_bytes(&self) -> usize {
        self.text.capacity() + self.graphemes.capacity() * std::mem::size_of::<GraphemeCells>()
    }

    pub fn text_for_cells(&self, cells: Range<usize>) -> &str {
        let first = self
            .graphemes
            .partition_point(|grapheme| grapheme.cells.end <= cells.start);
        let last = self
            .graphemes
            .partition_point(|grapheme| grapheme.cells.start < cells.end);
        if first >= last {
            return "";
        }
        &self.text[self.graphemes[first].bytes.start..self.graphemes[last - 1].bytes.end]
    }

    fn cell_range(&self, bytes: Range<usize>) -> Option<Range<usize>> {
        let first = self
            .graphemes
            .partition_point(|grapheme| grapheme.bytes.end <= bytes.start);
        let last = self
            .graphemes
            .partition_point(|grapheme| grapheme.bytes.start < bytes.end);
        (first < last)
            .then(|| self.graphemes[first].cells.start..self.graphemes[last - 1].cells.end)
    }
}

pub struct LineSnapshot {
    pub start_row: usize,
    pub line: LogicalLine,
}

#[derive(Default)]
pub struct GridSnapshot {
    pub lines: Vec<LineSnapshot>,
    pub skipped_lines: usize,
}

/// Extracts complete logical lines touching the requested original grid rows.
pub fn snapshot_lines(grid: &GridHandler, rows: Range<usize>) -> GridSnapshot {
    let mut result = GridSnapshot::default();
    result.skipped_lines = visit_lines(grid, rows, |line| result.lines.push(line));
    result
}

/// Visits complete logical lines without retaining the entire viewport snapshot.
pub fn visit_lines(
    grid: &GridHandler,
    rows: Range<usize>,
    mut visit: impl FnMut(LineSnapshot),
) -> usize {
    let mut skipped_lines = 0;
    let end = rows.end.min(grid.total_rows());
    let mut row_index = rows.start.min(end);
    if row_index >= end || grid.columns() == 0 {
        return 0;
    }

    let mut walked = 0;
    while row_index > 0 && grid.row_wraps(row_index - 1) && walked < MAX_LINE_ROWS {
        row_index -= 1;
        walked += 1;
    }
    if row_index > 0 && grid.row_wraps(row_index - 1) {
        skipped_lines += 1;
        row_index = rows.start;
        while row_index < end && grid.row_wraps(row_index) {
            row_index += 1;
        }
        row_index += 1;
    }

    while row_index < end {
        let start_row = row_index;
        let mut line = LogicalLine {
            text: String::new(),
            columns: grid.columns(),
            rows: 0,
            graphemes: Vec::new(),
        };
        let mut too_long = false;
        while let Some(row) = grid.row(row_index) {
            let content_end = row.line_length();
            if !too_long {
                for column in 0..content_end {
                    let cell = &row[column];
                    let width = match CellType::from(cell) {
                        CellType::RegularChar => 1,
                        CellType::WideChar => 2,
                        CellType::WideCharSpacer | CellType::LeadingWideCharSpacer => continue,
                    };
                    let start = line.text.len();
                    line.text.push_char_or_str(cell.content_for_display());
                    if line.text.len() > MAX_LINE_BYTES {
                        too_long = true;
                        break;
                    }
                    let position = line.rows * line.columns + column;
                    line.graphemes.push(GraphemeCells {
                        bytes: start..line.text.len(),
                        cells: position..position + width,
                    });
                }
            }
            let wraps = row
                .last()
                .is_some_and(|cell| cell.flags.contains(Flags::WRAPLINE));
            line.rows += 1;
            row_index += 1;
            if !wraps || row_index >= grid.total_rows() {
                break;
            }
            if line.rows >= MAX_LINE_ROWS {
                too_long = true;
                // Stop at the viewport boundary instead of walking an unbounded wrapped line.
                if row_index >= end {
                    break;
                }
            }
        }
        if too_long {
            skipped_lines += 1;
        } else {
            visit(LineSnapshot { start_row, line });
        }
    }
    skipped_lines
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HighlightSpan {
    pub cells: Range<usize>,
    pub style: HighlightStyle,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineHighlights {
    pub spans: Vec<HighlightSpan>,
    pub prompt: Option<Range<usize>>,
    pub separator: Option<HighlightColor>,
    pub limited: bool,
}

impl HighlightMatcher {
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty() && self.prompt_header.is_none()
    }

    pub fn find(&self, line: &LogicalLine) -> LineHighlights {
        if line.text.len() > MAX_LINE_BYTES {
            return LineHighlights {
                limited: true,
                ..Default::default()
            };
        }
        let mut matches = Vec::new();
        let mut limited = false;
        'rules: for rule in self.rules.iter().take(MAX_RULES) {
            for found in rule
                .regex
                .find_iter(&line.text)
                .filter(|found| !found.is_empty())
            {
                if matches.len() == MAX_MATCHES {
                    limited = true;
                    break 'rules;
                }
                let cells = match rule.target {
                    HighlightTarget::Text => line.cell_range(found.range()),
                    HighlightTarget::Line => Some(0..line.rows * line.columns),
                };
                if let Some(cells) = cells {
                    matches.push((cells, rule.style));
                }
                if rule.target == HighlightTarget::Line {
                    break;
                }
            }
        }
        let mut styles = RangeMap::new();
        for (range, style) in matches.into_iter().rev() {
            styles.insert(range, style);
        }
        let mut prompt = None;
        let mut separator = None;
        if let Some(rule) = &self.prompt_header
            && let Some(found) = rule.regex.find(&line.text)
            && found.start() == 0
            && !found.is_empty()
            && let Some(range) = line.cell_range(found.range())
        {
            prompt = Some(range.clone());
            separator = rule.separator;
            // A header background preserves trigger foreground outside the prompt prefix.
            if let Some(background) = rule.style.background {
                let existing: Vec<_> = styles
                    .iter()
                    .map(|(range, style)| (range.clone(), *style))
                    .collect();
                styles.insert(
                    0..line.rows * line.columns,
                    HighlightStyle {
                        background: Some(background),
                        ..Default::default()
                    },
                );
                for (range, mut style) in existing {
                    style.background = Some(background);
                    styles.insert(range, style);
                }
            }
            if let Some(foreground) = rule.style.foreground {
                let existing: Vec<_> = styles
                    .overlapping(&range)
                    .map(|(range, style)| (range.clone(), *style))
                    .collect();
                styles.insert(range.clone(), rule.style);
                for (existing_range, mut style) in existing {
                    style.foreground = Some(foreground);
                    if rule.style.background.is_some() {
                        style.background = rule.style.background;
                    }
                    styles.insert(
                        existing_range.start.max(range.start)..existing_range.end.min(range.end),
                        style,
                    );
                }
            }
        }
        LineHighlights {
            spans: styles
                .into_iter()
                .map(|(cells, style)| HighlightSpan { cells, style })
                .collect(),
            prompt,
            separator,
            limited,
        }
    }
}

/// Converts a relative linear cell interval to an inclusive original-grid range.
pub fn grid_range(cells: &Range<usize>, start_row: usize, columns: usize) -> RangeInclusive<Point> {
    Point::new(start_row + cells.start / columns, cells.start % columns)
        ..=Point::new(
            start_row + (cells.end - 1) / columns,
            (cells.end - 1) % columns,
        )
}

#[cfg(test)]
#[path = "highlight_tests.rs"]
mod tests;
