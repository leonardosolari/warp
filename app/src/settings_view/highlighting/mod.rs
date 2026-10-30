use std::cell::RefCell;
use std::time::Duration;

use settings::Setting;
use uuid::Uuid;
use warp_terminal::model::highlight::{LineHighlights, LogicalLine, MAX_RULES};
use warpui::elements::{
    ChildView, Container, CrossAxisAlignment, Expanded, Flex, MainAxisSize, MouseStateHandle,
    ParentElement, SavePosition, Text,
};
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::UiComponent;
use warpui::ui_components::switch::SwitchStateHandle;
use warpui::{
    AppContext, Element, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle,
};

use super::appearance_page::{AppearancePageAction, AppearanceSettingsPageView};
use super::settings_page::{LocalOnlyIconState, SettingsWidget, ToggleState, render_body_item};
use crate::appearance::Appearance;
use crate::editor::{EditorView, Event as EditorEvent, SingleLineEditorOptions, TextOptions};
use crate::terminal::color;
use crate::terminal::highlight::model::resolve_style;
use crate::terminal::highlight::settings::{
    HighlightSettings, PromptHeaderConfig, TextHighlightRule, compile_configuration,
};

const NAME: usize = 0;
const PATTERN: usize = 1;
const FOREGROUND: usize = 2;
const BACKGROUND: usize = 3;
const SEPARATOR: usize = 4;
const SAMPLE: usize = 5;
const BUTTONS: usize = MAX_RULES * 7;

#[derive(Clone, Debug)]
pub enum HighlightEditorAction {
    Add,
    Select(usize),
    Duplicate(usize),
    Remove(usize),
    Move(usize, bool),
    ToggleRule(usize),
    ToggleCase,
    ToggleTarget,
    ToggleSeparator,
    Save,
    Cancel,
}

struct PreviewLine {
    line: LogicalLine,
    highlights: LineHighlights,
}

pub struct HighlightEditor {
    prompt: bool,
    fields: Vec<ViewHandle<EditorView>>,
    rules: Vec<TextHighlightRule>,
    draft: Option<TextHighlightRule>,
    show_separator: bool,
    preview: Vec<PreviewLine>,
    error: Option<String>,
    save_error: Option<String>,
    generation: u64,
    buttons: Vec<MouseStateHandle>,
    preview_task: Option<warpui::r#async::SpawnedFutureHandle>,
    dirty: bool,
}

impl Entity for HighlightEditor {
    type Event = ();
}

impl HighlightEditor {
    pub fn new(prompt: bool, ctx: &mut ViewContext<Self>) -> Self {
        let fields: Vec<_> = (0..6)
            .map(|_| {
                ctx.add_typed_action_view(|ctx| {
                    EditorView::single_line(
                        SingleLineEditorOptions {
                            text: TextOptions::ui_font_size(Appearance::as_ref(ctx)),
                            ..Default::default()
                        },
                        ctx,
                    )
                })
            })
            .collect();
        for field in &fields {
            ctx.subscribe_to_view(field, |me, _, event, ctx| {
                if let EditorEvent::Edited(origin) = event
                    && origin.is_user()
                {
                    me.dirty = true;
                    me.save_error = None;
                    me.refresh_preview(ctx);
                }
            });
        }
        ctx.subscribe_to_model(&HighlightSettings::handle(ctx), |me, _, _, ctx| {
            me.rules = HighlightSettings::as_ref(ctx).rules.value().clone();
            if me.prompt && !me.dirty {
                me.load_prompt(ctx);
            }
            ctx.notify();
        });
        let mut editor = Self {
            prompt,
            fields,
            rules: HighlightSettings::as_ref(ctx).rules.value().clone(),
            draft: None,
            show_separator: true,
            preview: Vec::new(),
            error: None,
            save_error: None,
            generation: 0,
            buttons: (0..BUTTONS + 12)
                .map(|_| MouseStateHandle::default())
                .collect(),
            preview_task: None,
            dirty: false,
        };
        if prompt {
            editor.load_prompt(ctx);
        }
        editor
    }

    fn text(&self, field: usize, app: &AppContext) -> String {
        self.fields[field].as_ref(app).buffer_text(app)
    }

    fn set_text(&self, field: usize, text: &str, ctx: &mut ViewContext<Self>) {
        self.fields[field].update(ctx, |editor, ctx| editor.set_buffer_text(text, ctx));
    }

    fn load_prompt(&mut self, ctx: &mut ViewContext<Self>) {
        let header = HighlightSettings::as_ref(ctx).prompt_header.value().clone();
        self.set_text(PATTERN, header.fallback_regex.as_deref().unwrap_or(""), ctx);
        self.set_text(FOREGROUND, header.foreground.as_deref().unwrap_or(""), ctx);
        self.set_text(BACKGROUND, &header.background, ctx);
        self.set_text(SEPARATOR, &header.separator, ctx);
        self.set_text(SAMPLE, "leo@server:~/app$ ", ctx);
        self.show_separator = header.show_separator;
        self.dirty = false;
        self.refresh_preview(ctx);
    }

    fn select_rule(&mut self, rule: TextHighlightRule, ctx: &mut ViewContext<Self>) {
        self.set_text(NAME, &rule.name, ctx);
        self.set_text(PATTERN, &rule.pattern, ctx);
        self.set_text(FOREGROUND, rule.foreground.as_deref().unwrap_or(""), ctx);
        self.set_text(BACKGROUND, rule.background.as_deref().unwrap_or(""), ctx);
        self.set_text(SAMPLE, "ERROR: cannot connect to server", ctx);
        self.draft = Some(rule);
        self.dirty = false;
        self.refresh_preview(ctx);
    }

    fn rule(&self, app: &AppContext) -> Option<TextHighlightRule> {
        let mut rule = self.draft.clone()?;
        rule.name = self.text(NAME, app);
        rule.pattern = self.text(PATTERN, app);
        rule.foreground = optional(self.text(FOREGROUND, app));
        rule.background = optional(self.text(BACKGROUND, app));
        Some(rule)
    }

    fn header(&self, app: &AppContext) -> PromptHeaderConfig {
        PromptHeaderConfig {
            enabled: HighlightSettings::as_ref(app).prompt_header.enabled,
            foreground: optional(self.text(FOREGROUND, app)),
            background: self.text(BACKGROUND, app),
            separator: self.text(SEPARATOR, app),
            show_separator: self.show_separator,
            fallback_regex: optional(self.text(PATTERN, app)),
        }
    }

    fn refresh_preview(&mut self, ctx: &mut ViewContext<Self>) {
        let mut rules = self.rules.clone();
        if let Some(rule) = self.rule(ctx) {
            if let Some(existing) = rules.iter_mut().find(|existing| existing.id == rule.id) {
                *existing = rule;
            } else {
                rules.push(rule);
            }
        }
        let mut header = self.header(ctx);
        header.enabled = self.prompt;
        let sample = self.text(SAMPLE, ctx);
        let prompt = self.prompt;
        self.generation += 1;
        let generation = self.generation;
        self.preview_task = Some(ctx.spawn(
            async move {
                warpui::r#async::Timer::after(Duration::from_millis(100)).await;
                let mut compiled = compile_configuration(!prompt, &rules, &header);
                if prompt
                    && header.fallback_regex.is_none()
                    && !sample.is_empty()
                    && let Some(style) = compiled.header_style
                    && let Ok(regex) = warp_terminal::model::highlight::compile_pattern(
                        &format!("^{}", regex::escape(&sample)),
                        true,
                    )
                {
                    compiled.matcher.prompt_header =
                        Some(warp_terminal::model::highlight::PromptHeaderRule {
                            regex,
                            style,
                            separator: compiled.header_separator,
                        });
                }
                let texts = if prompt {
                    vec![
                        format!("{sample}echo first"),
                        "first output".into(),
                        format!("{sample}echo second"),
                        "second output".into(),
                    ]
                } else {
                    vec![sample]
                };
                let preview = texts
                    .into_iter()
                    .map(|text| {
                        let line = LogicalLine::from_text(&text);
                        let highlights = compiled.matcher.find(&line);
                        PreviewLine { line, highlights }
                    })
                    .collect();
                (preview, compiled.errors.into_iter().next())
            },
            move |me, (preview, error), ctx| {
                if me.generation == generation {
                    me.preview = preview;
                    me.error = error;
                    ctx.notify();
                }
            },
        ));
        ctx.notify();
    }

    fn save_rules(&mut self, ctx: &mut ViewContext<Self>) {
        let rules = self.rules.clone();
        let result = HighlightSettings::handle(ctx)
            .update(ctx, |settings, ctx| settings.rules.set_value(rules, ctx));
        self.save_error = result.err().map(|error| error.to_string());
        if self.save_error.is_some() {
            self.rules = HighlightSettings::as_ref(ctx).rules.value().clone();
        }
        self.refresh_preview(ctx);
        ctx.notify();
    }

    fn button(
        &self,
        label: &str,
        action: HighlightEditorAction,
        index: usize,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let position_id = format!("highlight_editor:{}:button:{action:?}", self.prompt);
        let button = Appearance::as_ref(app)
            .ui_builder()
            .button(ButtonVariant::Secondary, self.buttons[index].clone())
            .with_text_label(label.to_string())
            .build()
            .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
            .finish();
        SavePosition::new(button, &position_id).finish()
    }

    fn field(&self, label: &str, index: usize, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        Flex::column()
            .with_child(
                Text::new(label.to_owned(), appearance.ui_font_family(), 12.)
                    .with_color(appearance.theme().foreground().into_solid())
                    .finish(),
            )
            .with_child(
                Container::new(
                    SavePosition::new(
                        appearance
                            .ui_builder()
                            .text_input(self.fields[index].clone())
                            .build()
                            .finish(),
                        &format!("highlight_editor:{}:field:{index}", self.prompt),
                    )
                    .finish(),
                )
                .with_margin_top(4.)
                .with_margin_bottom(12.)
                .finish(),
            )
            .finish()
    }

    fn preview(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let colors = color::List::from(&color::Colors::from(theme.clone()));
        let mut column = Flex::column();
        for line in &self.preview {
            let cells = line.line.columns;
            let mut row = Flex::row().with_main_axis_size(MainAxisSize::Max);
            let mut position = 0;
            for span in &line.highlights.spans {
                let start = span.cells.start.min(cells);
                let end = span.cells.end.min(cells);
                if start > position {
                    row.add_child(
                        Text::new_inline(
                            line.line.text_for_cells(position..start).to_owned(),
                            appearance.monospace_font_family(),
                            12.,
                        )
                        .with_color(theme.foreground().into_solid())
                        .finish(),
                    );
                }
                let style = resolve_style(span.style, &colors, theme);
                let mut chunk = Container::new(
                    Text::new_inline(
                        line.line.text_for_cells(start..end).to_owned(),
                        appearance.monospace_font_family(),
                        12.,
                    )
                    .with_color(style.foreground.unwrap_or(theme.foreground().into_solid()))
                    .finish(),
                );
                if let Some(background) = style.background {
                    chunk = chunk.with_background_color(background);
                }
                row.add_child(chunk.finish());
                position = end;
            }
            if position < cells {
                row.add_child(
                    Text::new_inline(
                        line.line.text_for_cells(position..cells).to_owned(),
                        appearance.monospace_font_family(),
                        12.,
                    )
                    .with_color(theme.foreground().into_solid())
                    .finish(),
                );
            }
            let mut container = Container::new(row.finish());
            if let Some(background) = line
                .highlights
                .spans
                .iter()
                .find(|span| span.cells.start == 0 && span.cells.end >= cells)
                .and_then(|span| resolve_style(span.style, &colors, theme).background)
                .or_else(|| {
                    line.highlights
                        .prompt
                        .as_ref()
                        .and_then(|_| line.highlights.spans.first())
                        .and_then(|span| resolve_style(span.style, &colors, theme).background)
                })
            {
                container = container.with_background_color(background);
            }
            if let Some(separator) = line.highlights.separator {
                container =
                    container.with_border(warpui::elements::Border::top(1.).with_border_color(
                        crate::terminal::highlight::model::resolve_color(separator, &colors, theme),
                    ));
            }
            column.add_child(container.finish());
        }
        if self.prompt {
            let recognized = self.preview.iter().find_map(|line| {
                line.highlights
                    .prompt
                    .clone()
                    .map(|range| line.line.text_for_cells(range).replace(' ', "␠"))
            });
            column.add_child(
                Container::new(
                    Text::new(
                        recognized.map_or_else(
                            || "No prompt prefix matched the sample.".to_string(),
                            |prefix| format!("Recognized prompt: {prefix} (␠ marks a space)"),
                        ),
                        appearance.ui_font_family(),
                        12.,
                    )
                    .with_color(theme.foreground().into_solid())
                    .finish(),
                )
                .with_margin_top(8.)
                .finish(),
            );
        }
        Container::new(column.finish())
            .with_uniform_padding(12.)
            .with_background(theme.background())
            .with_margin_bottom(8.)
            .finish()
    }
}

fn optional(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

impl TypedActionView for HighlightEditor {
    type Action = HighlightEditorAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            HighlightEditorAction::Add => {
                if self.rules.len() < MAX_RULES {
                    self.select_rule(TextHighlightRule::default(), ctx);
                }
            }
            HighlightEditorAction::Select(index) => {
                if let Some(rule) = self.rules.get(*index) {
                    self.select_rule(rule.clone(), ctx);
                }
            }
            HighlightEditorAction::Duplicate(index) => {
                if self.rules.len() < MAX_RULES
                    && let Some(rule) = self.rules.get(*index)
                {
                    let mut rule = rule.clone();
                    rule.id = Uuid::new_v4().to_string();
                    rule.name.push_str(" (copy)");
                    self.select_rule(rule, ctx);
                }
            }
            HighlightEditorAction::Remove(index) => {
                if *index < self.rules.len() {
                    let removed = self.rules.remove(*index);
                    if self
                        .draft
                        .as_ref()
                        .is_some_and(|draft| draft.id == removed.id)
                    {
                        self.draft = None;
                    }
                    self.save_rules(ctx);
                }
            }
            HighlightEditorAction::Move(index, up) => {
                let next = if *up {
                    index.saturating_sub(1)
                } else {
                    index.saturating_add(1)
                };
                if next < self.rules.len() && *index < self.rules.len() {
                    self.rules.swap(*index, next);
                    self.save_rules(ctx);
                }
            }
            HighlightEditorAction::ToggleRule(index) => {
                if let Some(rule) = self.rules.get_mut(*index) {
                    rule.enabled = !rule.enabled;
                    self.save_rules(ctx);
                }
            }
            HighlightEditorAction::ToggleCase => {
                if let Some(rule) = &mut self.draft {
                    rule.case_sensitive = !rule.case_sensitive;
                }
                self.refresh_preview(ctx);
            }
            HighlightEditorAction::ToggleTarget => {
                if let Some(rule) = &mut self.draft {
                    rule.target = if rule.target == "text" {
                        "line"
                    } else {
                        "text"
                    }
                    .into();
                }
                self.refresh_preview(ctx);
            }
            HighlightEditorAction::ToggleSeparator => {
                self.show_separator = !self.show_separator;
                self.dirty = true;
                self.refresh_preview(ctx);
            }
            HighlightEditorAction::Save => {
                self.save_error = None;
                if self.prompt {
                    let header = self.header(ctx);
                    match header.validate() {
                        Ok(()) => {
                            let result = HighlightSettings::handle(ctx)
                                .update(ctx, |settings, ctx| {
                                    settings.prompt_header.set_value(header, ctx)
                                });
                            self.save_error = result.err().map(|error| error.to_string());
                            if self.save_error.is_none() {
                                self.dirty = false;
                            }
                        }
                        Err(error) => self.error = Some(error),
                    }
                } else if let Some(rule) = self.rule(ctx) {
                    match rule.compile() {
                        Ok(_) => {
                            if let Some(existing) =
                                self.rules.iter_mut().find(|entry| entry.id == rule.id)
                            {
                                *existing = rule;
                            } else if self.rules.len() < MAX_RULES {
                                self.rules.push(rule);
                            }
                            self.save_rules(ctx);
                            if self.save_error.is_none() {
                                self.draft = None;
                            }
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                ctx.notify();
            }
            HighlightEditorAction::Cancel => {
                self.error = None;
                self.save_error = None;
                self.dirty = false;
                if self.prompt {
                    self.load_prompt(ctx);
                } else {
                    self.draft = None;
                }
                ctx.notify();
            }
        }
    }
}

impl View for HighlightEditor {
    fn ui_name() -> &'static str {
        "TerminalHighlightEditor"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let mut column = Flex::column();
        if let Some(error) = &self.save_error {
            column.add_child(
                Text::new(error.clone(), appearance.ui_font_family(), 12.)
                    .with_color(appearance.theme().foreground().into_solid())
                    .finish(),
            );
        }
        if !self.prompt {
            let runtime = crate::terminal::highlight::HighlightRuntime::as_ref(app);
            for error in runtime.configuration_errors() {
                column.add_child(
                    Text::new(error.clone(), appearance.ui_font_family(), 12.)
                        .with_color(appearance.theme().foreground().into_solid())
                        .finish(),
                );
            }
            if runtime.skipped_lines() > 0 || runtime.has_limited_matches() {
                column.add_child(Text::new("Some terminal lines exceed the highlighting limits (16 KiB, 512 wrapped rows or 512 matches).", appearance.ui_font_family(), 12.)
                    .with_color(appearance.theme().foreground().into_solid()).finish());
            }
            for (index, rule) in self.rules.iter().enumerate().take(MAX_RULES) {
                column.add_child(
                    Flex::row()
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_child(
                            Expanded::new(
                                1.,
                                self.button(
                                    &rule.name,
                                    HighlightEditorAction::Select(index),
                                    index * 7,
                                    app,
                                ),
                            )
                            .finish(),
                        )
                        .with_child(self.button(
                            if rule.enabled { "On" } else { "Off" },
                            HighlightEditorAction::ToggleRule(index),
                            index * 7 + 1,
                            app,
                        ))
                        .with_child(self.button(
                            "↑",
                            HighlightEditorAction::Move(index, true),
                            index * 7 + 2,
                            app,
                        ))
                        .with_child(self.button(
                            "↓",
                            HighlightEditorAction::Move(index, false),
                            index * 7 + 3,
                            app,
                        ))
                        .with_child(self.button(
                            "Duplicate",
                            HighlightEditorAction::Duplicate(index),
                            index * 7 + 4,
                            app,
                        ))
                        .with_child(self.button(
                            "Delete",
                            HighlightEditorAction::Remove(index),
                            index * 7 + 5,
                            app,
                        ))
                        .finish(),
                );
            }
            column.add_child(self.button(
                "Add highlight rule",
                HighlightEditorAction::Add,
                BUTTONS,
                app,
            ));
        }
        if self.prompt || self.draft.is_some() {
            if !self.prompt {
                column.add_child(self.field("Name", NAME, app));
            }
            column.add_child(self.field(
                if self.prompt {
                    "Prompt regex (optional; match a prefix at the start of the line)"
                } else {
                    "Regex pattern"
                },
                PATTERN,
                app,
            ));
            column.add_child(self.field(
                "Text color (ANSI name or #RRGGBB; empty preserves existing colors)",
                FOREGROUND,
                app,
            ));
            column.add_child(self.field("Background color", BACKGROUND, app));
            if let Some(rule) = &self.draft {
                column.add_child(
                    Flex::row()
                        .with_child(self.button(
                            if rule.case_sensitive {
                                "Case sensitive"
                            } else {
                                "Ignore case"
                            },
                            HighlightEditorAction::ToggleCase,
                            BUTTONS + 1,
                            app,
                        ))
                        .with_child(self.button(
                            if rule.target == "text" {
                                "Target: Text"
                            } else {
                                "Target: Line"
                            },
                            HighlightEditorAction::ToggleTarget,
                            BUTTONS + 2,
                            app,
                        ))
                        .finish(),
                );
            }
            if self.prompt {
                column.add_child(self.field("Separator color", SEPARATOR, app));
                column.add_child(self.button(
                    if self.show_separator {
                        "Separator: On"
                    } else {
                        "Separator: Off"
                    },
                    HighlightEditorAction::ToggleSeparator,
                    BUTTONS + 3,
                    app,
                ));
                column.add_child(Text::new("Classic/PS1 uses known prompt boundaries. For sessions without integration, enter a prefix regex. Output that matches can also receive a header. Theme colors: theme:surface_2, theme:foreground, theme:outline, theme:accent.", appearance.ui_font_family(), 12.).with_color(appearance.theme().foreground().into_solid()).finish());
            }
            column.add_child(self.field(
                if self.prompt {
                    "Sample prompt (include its trailing space)"
                } else {
                    "Sample text"
                },
                SAMPLE,
                app,
            ));
            column.add_child(self.preview(app));
            if let Some(error) = &self.error {
                column.add_child(
                    Text::new(error.clone(), appearance.ui_font_family(), 12.)
                        .with_color(appearance.theme().foreground().into_solid())
                        .finish(),
                );
            }
            column.add_child(
                Flex::row()
                    .with_child(self.button("Save", HighlightEditorAction::Save, BUTTONS + 4, app))
                    .with_child(self.button(
                        "Cancel",
                        HighlightEditorAction::Cancel,
                        BUTTONS + 5,
                        app,
                    ))
                    .finish(),
            );
        }
        Container::new(column.finish())
            .with_margin_bottom(16.)
            .finish()
    }
}

#[derive(Default)]
pub struct HighlightToggleWidget {
    switch: SwitchStateHandle,
    local: RefCell<std::collections::HashMap<String, MouseStateHandle>>,
}
#[derive(Default)]
pub struct PromptHeaderToggleWidget {
    switch: SwitchStateHandle,
    local: RefCell<std::collections::HashMap<String, MouseStateHandle>>,
}
pub struct HighlightRulesWidget;
pub struct PromptHeaderWidget;

impl SettingsWidget for HighlightToggleWidget {
    type View = AppearanceSettingsPageView;
    fn search_terms(&self) -> &str {
        "terminal text highlighting enable trigger regex colors"
    }
    fn render(
        &self,
        _: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        render_body_item::<AppearancePageAction>(
            "Text highlighting".into(),
            None,
            LocalOnlyIconState::for_setting(
                "TextHighlightsEnabled",
                settings::SyncToCloud::Never,
                &mut self.local.borrow_mut(),
                app,
            ),
            ToggleState::Enabled,
            appearance,
            appearance
                .ui_builder()
                .switch(self.switch.clone())
                .check(*HighlightSettings::as_ref(app).enabled)
                .build()
                .on_click(|ctx, _, _| {
                    ctx.dispatch_typed_action(AppearancePageAction::ToggleTextHighlighting)
                })
                .finish(),
            None,
        )
    }
}

impl SettingsWidget for PromptHeaderToggleWidget {
    type View = AppearanceSettingsPageView;
    fn search_terms(&self) -> &str {
        "terminal prompt header colors enable classic ssh separator"
    }
    fn render(
        &self,
        _: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        render_body_item::<AppearancePageAction>(
            "Prompt header colors".into(),
            None,
            LocalOnlyIconState::for_setting(
                "PromptHeaderColors",
                settings::SyncToCloud::Never,
                &mut self.local.borrow_mut(),
                app,
            ),
            ToggleState::Enabled,
            appearance,
            appearance
                .ui_builder()
                .switch(self.switch.clone())
                .check(HighlightSettings::as_ref(app).prompt_header.enabled)
                .build()
                .on_click(|ctx, _, _| {
                    ctx.dispatch_typed_action(AppearancePageAction::TogglePromptHeaderColors)
                })
                .finish(),
            None,
        )
    }
}

impl SettingsWidget for HighlightRulesWidget {
    type View = AppearanceSettingsPageView;
    fn search_terms(&self) -> &str {
        "terminal text highlight rules regex priority trigger pattern"
    }
    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _: &AppContext,
    ) -> Box<dyn Element> {
        let mut column = Flex::column();
        if let Some(error) = &view.highlight_error {
            column.add_child(
                Text::new(error.clone(), appearance.ui_font_family(), 12.)
                    .with_color(appearance.theme().foreground().into_solid())
                    .finish(),
            );
        }
        column
            .with_child(
                ChildView::new(
                    view.highlight_rules_editor
                        .as_ref()
                        .expect("highlight editor is feature gated"),
                )
                .finish(),
            )
            .finish()
    }
}

impl SettingsWidget for PromptHeaderWidget {
    type View = AppearanceSettingsPageView;
    fn search_terms(&self) -> &str {
        "terminal prompt header colors background foreground separator regex classic ssh preview"
    }
    fn render(&self, view: &Self::View, _: &Appearance, _: &AppContext) -> Box<dyn Element> {
        ChildView::new(
            view.prompt_header_editor
                .as_ref()
                .expect("prompt editor is feature gated"),
        )
        .finish()
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
