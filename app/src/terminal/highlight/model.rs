use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use parking_lot::Mutex;
use pathfinder_color::ColorU;
use warp_terminal::model::grid::Dimensions;
use warp_terminal::model::grid::grid_handler::GridHandler;
use warp_terminal::model::highlight::{
    HighlightColor, HighlightSpan, HighlightStyle, LineHighlights, LogicalLine, MAX_MATCHES,
    ThemeColor, visit_lines,
};
use warpui::{AppContext, Entity, ModelContext, SingletonEntity};

use super::settings::{CompiledHighlights, HighlightSettings, compile_configuration};
use crate::features::FeatureFlag;
use crate::terminal::color;
use crate::terminal::model::index::Point;
use crate::themes::theme::WarpTheme;

const CACHE_BUDGET: usize = 16 * 1024 * 1024;
const MAX_PENDING: usize = 64;
struct ResultSlot {
    value: Mutex<Option<Arc<LineHighlights>>>,
    budget: Arc<AtomicUsize>,
    bytes: usize,
    needed: AtomicUsize,
    queued: AtomicBool,
}

impl Drop for ResultSlot {
    fn drop(&mut self) {
        self.budget.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

type MatchResult = Arc<ResultSlot>;

struct Work {
    line: Arc<LogicalLine>,
    configuration: Arc<CompiledHighlights>,
    result: MatchResult,
}

struct CacheEntry {
    result: MatchResult,
    touched: u64,
    bytes: usize,
}

struct PreparedLine {
    start_row: usize,
    line: Arc<LogicalLine>,
    result: MatchResult,
}

impl Drop for PreparedLine {
    fn drop(&mut self) {
        self.result.needed.fetch_sub(1, Ordering::Relaxed);
    }
}

struct PreparedGrid {
    lines: Vec<PreparedLine>,
    ranges: Vec<Range<usize>>,
    retry: bool,
    revision: u64,
}

#[derive(Default)]
struct Cache {
    lines: HashMap<Arc<LogicalLine>, CacheEntry>,
    prepared: HashMap<u64, PreparedGrid>,
    bytes: usize,
    clock: u64,
    skipped: usize,
    headers: HashMap<u64, (Point, usize)>,
}

pub struct HighlightRuntime {
    configuration: Arc<CompiledHighlights>,
    cache: RefCell<Cache>,
    work: async_channel::Sender<Work>,
    budget: Arc<AtomicUsize>,
}

impl Entity for HighlightRuntime {
    type Event = ();
}
impl SingletonEntity for HighlightRuntime {}

impl HighlightRuntime {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let (work, receiver) = async_channel::bounded::<Work>(MAX_PENDING);
        for _ in 0..2 {
            let receiver = receiver.clone();
            ctx.background_executor()
                .spawn(async move {
                    while let Ok(work) = receiver.recv().await {
                        if work.result.needed.load(Ordering::Relaxed) > 0 {
                            let result = work.configuration.matcher.find(&work.line);
                            *work.result.value.lock() = Some(Arc::new(result));
                        }
                        work.result.queued.store(false, Ordering::Release);
                    }
                })
                .detach();
        }
        ctx.subscribe_to_model(&HighlightSettings::handle(ctx), |me, _, _, ctx| {
            me.configuration = Arc::new(Self::configuration(ctx));
            *me.cache.borrow_mut() = Cache::default();
            ctx.emit(());
            ctx.notify();
        });
        Self {
            configuration: Arc::new(Self::configuration(ctx)),
            cache: RefCell::new(Cache::default()),
            work,
            budget: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn configuration(app: &AppContext) -> CompiledHighlights {
        let settings = HighlightSettings::as_ref(app);
        compile_configuration(*settings.enabled, &settings.rules, &settings.prompt_header)
    }

    pub fn configuration_errors(&self) -> &[String] {
        &self.configuration.errors
    }

    pub fn skipped_lines(&self) -> usize {
        self.cache.borrow().skipped
    }

    pub fn has_limited_matches(&self) -> bool {
        self.cache.borrow().lines.values().any(|entry| {
            entry
                .result
                .value
                .lock()
                .as_ref()
                .is_some_and(|result| result.limited)
        })
    }

    pub fn is_enabled(&self) -> bool {
        FeatureFlag::TerminalHighlighting.is_enabled() && !self.configuration.matcher.is_empty()
    }

    pub fn header_style(&self) -> Option<HighlightStyle> {
        FeatureFlag::TerminalHighlighting
            .is_enabled()
            .then_some(self.configuration.header_style)
            .flatten()
    }

    pub fn header_separator(&self) -> Option<HighlightColor> {
        self.configuration.header_separator
    }

    pub fn prepare_prompt_grid(&self, grid: &GridHandler, prompt_end: Point, rows: usize) {
        if self.header_style().is_some() {
            let mut cache = self.cache.borrow_mut();
            if cache.headers.len() >= 256 {
                cache.headers.clear();
            }
            cache
                .headers
                .insert(grid.content_revision(), (prompt_end, rows));
        }
    }

    /// Prepares only visible original rows; unchanged lines reuse their immutable results.
    pub fn prepare_grid(&self, grid: &GridHandler, displayed_rows: Range<usize>) {
        if !self.is_enabled() {
            return;
        }
        let ranges: Vec<_> = if let Some(rows) = grid.displayed_output_rows() {
            let mut ranges: Vec<Range<usize>> = Vec::new();
            for row in rows.skip(displayed_rows.start).take(displayed_rows.len()) {
                if let Some(previous) = ranges.last_mut()
                    && previous.end == row
                {
                    previous.end += 1;
                } else {
                    ranges.push(row..row + 1);
                }
            }
            ranges
        } else {
            vec![displayed_rows]
        };
        let mut prepared = Vec::new();
        let mut retry = false;
        let mut cache = self.cache.borrow_mut();
        if cache
            .prepared
            .get(&grid.content_identity())
            .is_some_and(|prepared| {
                prepared.revision == grid.content_revision()
                    && prepared.ranges == ranges
                    && !prepared.retry
                    && prepared.lines.iter().all(|line| {
                        line.result.queued.load(Ordering::Acquire)
                            || line.result.value.lock().is_some()
                    })
            })
        {
            return;
        }
        cache.clock += 1;
        let clock = cache.clock;
        for range in &ranges {
            let skipped = visit_lines(grid, range.clone(), |snapshot| {
                if prepared
                    .last()
                    .is_some_and(|previous: &PreparedLine| previous.start_row == snapshot.start_row)
                {
                    return;
                }
                let snapshot_line = Arc::new(snapshot.line);
                let line = cache
                    .lines
                    .get_key_value(&snapshot_line)
                    .map_or(snapshot_line.clone(), |(shared, _)| shared.clone());
                let result = if let Some(entry) = cache.lines.get_mut(&line) {
                    entry.touched = clock;
                    let result = entry.result.clone();
                    result.needed.fetch_add(1, Ordering::Relaxed);
                    if result.value.lock().is_none() && !result.queued.swap(true, Ordering::AcqRel)
                    {
                        let work = Work {
                            line: line.clone(),
                            configuration: self.configuration.clone(),
                            result: result.clone(),
                        };
                        if self.work.try_send(work).is_err() {
                            result.queued.store(false, Ordering::Release);
                            retry = true;
                        }
                    }
                    result
                } else {
                    let max_spans = (line.rows * line.columns).min(MAX_MATCHES * 2 + 4);
                    let bytes = line.estimated_bytes()
                        + max_spans * std::mem::size_of::<HighlightSpan>()
                        + std::mem::size_of::<ResultSlot>()
                        + std::mem::size_of::<LineHighlights>();
                    if self.budget.load(Ordering::Relaxed) + bytes > CACHE_BUDGET {
                        cache.prepared.clear();
                        let mut oldest: Vec<_> = cache
                            .lines
                            .iter()
                            .map(|(key, value)| (value.touched, key.clone()))
                            .collect();
                        oldest.sort_unstable_by_key(|(touched, _)| *touched);
                        for (_, key) in oldest {
                            if self.budget.load(Ordering::Relaxed) + bytes <= CACHE_BUDGET {
                                break;
                            }
                            if let Some(entry) = cache.lines.remove(&key) {
                                cache.bytes -= entry.bytes;
                            }
                        }
                    }
                    if self
                        .budget
                        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                            (used + bytes <= CACHE_BUDGET).then_some(used + bytes)
                        })
                        .is_err()
                    {
                        retry = true;
                        return;
                    }
                    let result = Arc::new(ResultSlot {
                        value: Mutex::new(None),
                        budget: self.budget.clone(),
                        bytes,
                        needed: AtomicUsize::new(1),
                        queued: AtomicBool::new(true),
                    });
                    let work = Work {
                        line: line.clone(),
                        configuration: self.configuration.clone(),
                        result: result.clone(),
                    };
                    if self.work.try_send(work).is_err() {
                        retry = true;
                        return;
                    }
                    cache.bytes += bytes;
                    cache.lines.insert(
                        line.clone(),
                        CacheEntry {
                            result: result.clone(),
                            touched: clock,
                            bytes,
                        },
                    );
                    result
                };
                prepared.push(PreparedLine {
                    start_row: snapshot.start_row,
                    line,
                    result,
                });
            });
            cache.skipped = cache.skipped.saturating_add(skipped);
        }
        if cache.prepared.len() >= 128 {
            cache.prepared.clear();
        }
        cache.prepared.insert(
            grid.content_identity(),
            PreparedGrid {
                lines: prepared,
                ranges,
                retry,
                revision: grid.content_revision(),
            },
        );
    }

    pub fn for_grid(
        &self,
        grid: &GridHandler,
        colors: &color::List,
        theme: &WarpTheme,
    ) -> GridHighlights {
        let mut highlights = GridHighlights::default();
        if !FeatureFlag::TerminalHighlighting.is_enabled() {
            return highlights;
        }
        let cache = self.cache.borrow();
        if let Some((end, rows)) = cache.headers.get(&grid.content_revision())
            && let Some(style) = self.header_style()
        {
            highlights.metadata = Some((
                0..end.row * grid.columns() + end.col + 1,
                rows * grid.columns(),
                resolve_style(style, colors, theme),
            ));
            if let Some(color) = self.header_separator() {
                highlights
                    .separators
                    .push((0, resolve_color(color, colors, theme)));
            }
        }
        if let Some(prepared_grid) = cache.prepared.get(&grid.content_identity()) {
            if prepared_grid.revision != grid.content_revision() {
                highlights.pending = true;
                return highlights;
            }
            highlights.pending = prepared_grid.retry;
            for prepared in &prepared_grid.lines {
                let result = prepared.result.value.lock();
                let Some(result) = result.as_ref() else {
                    highlights.pending = true;
                    continue;
                };
                for span in &result.spans {
                    let start = prepared.start_row * prepared.line.columns + span.cells.start;
                    let end = prepared.start_row * prepared.line.columns + span.cells.end;
                    highlights
                        .spans
                        .push((start..end, resolve_style(span.style, colors, theme)));
                }
                if let Some(color) = result.separator {
                    highlights
                        .separators
                        .push((prepared.start_row, resolve_color(color, colors, theme)));
                }
            }
        }
        highlights
            .spans
            .sort_unstable_by_key(|(range, _)| range.start);
        highlights
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResolvedHighlightStyle {
    pub foreground: Option<ColorU>,
    pub background: Option<ColorU>,
}

#[derive(Default)]
pub struct GridHighlights {
    spans: Vec<(Range<usize>, ResolvedHighlightStyle)>,
    pub separators: Vec<(usize, ColorU)>,
    pub pending: bool,
    metadata: Option<(Range<usize>, usize, ResolvedHighlightStyle)>,
}

impl GridHighlights {
    pub fn style_at(&self, point: Point, columns: usize) -> Option<ResolvedHighlightStyle> {
        let position = point.row * columns + point.col;
        let index = self
            .spans
            .partition_point(|(range, _)| range.end <= position);
        let mut style = self
            .spans
            .get(index)
            .filter(|(range, _)| range.contains(&position))
            .map(|(_, style)| *style);
        if let Some((prompt, end, metadata)) = &self.metadata
            && position < *end
        {
            let value = style.get_or_insert_default();
            if metadata.background.is_some() {
                value.background = metadata.background;
            }
            if prompt.contains(&position) && metadata.foreground.is_some() {
                value.foreground = metadata.foreground;
            }
        }
        style
    }
}

pub fn resolve_color(color: HighlightColor, colors: &color::List, theme: &WarpTheme) -> ColorU {
    match color {
        HighlightColor::Rgb([red, green, blue]) => ColorU::new(red, green, blue, 255),
        HighlightColor::Ansi(index) => colors[index as usize],
        HighlightColor::Theme(token) => match token {
            ThemeColor::Surface => theme.surface_2().into_solid(),
            ThemeColor::Foreground => theme.foreground().into_solid(),
            ThemeColor::Outline => theme.outline().into_solid(),
            ThemeColor::Accent => theme.accent().into_solid(),
        },
    }
}

pub fn resolve_style(
    style: HighlightStyle,
    colors: &color::List,
    theme: &WarpTheme,
) -> ResolvedHighlightStyle {
    ResolvedHighlightStyle {
        foreground: style
            .foreground
            .map(|value| resolve_color(value, colors, theme)),
        background: style
            .background
            .map(|value| resolve_color(value, colors, theme)),
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
