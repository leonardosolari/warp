use std::collections::HashMap;

use warp::appearance::Appearance;
use warp::features::FeatureFlag;
use warp::integration_testing::settings::{open_settings_page, type_settings_search};
use warp::integration_testing::step::new_step_with_default_assertions;
use warp::integration_testing::terminal::{
    clear_blocklist_to_remove_bootstrapped_blocks, execute_command_successfully,
    execute_long_running_command, wait_until_bootstrapped_single_pane_for_tab,
};
use warp::integration_testing::view_getters::single_terminal_view_for_tab;
use warp::settings_view::SettingsSection;
use warp::terminal::highlight::HighlightRuntime;
use warp::terminal::highlight::settings::{
    HighlightSettings, PromptHeaderConfig, TextHighlightRule,
};
use warp::terminal::model::blocks::BlockFilter;
use warp::terminal::model::grid::Dimensions;
use warp::terminal::model::index::Point;
use warpui_core::integration::{AssertionOutcome, TestStep};
use warpui_core::{SingletonEntity, async_assert};

use super::{Builder, new_builder};

fn highlighting_defaults() -> HashMap<String, String> {
    FeatureFlag::TerminalHighlighting.set_enabled(true);
    let rule = TextHighlightRule {
        name: "Errors".into(),
        pattern: "ERROR".into(),
        foreground: Some("#FF0000".into()),
        ..Default::default()
    };
    HashMap::from([
        ("TextHighlightsEnabled".into(), "true".into()),
        (
            "TextHighlightRules".into(),
            serde_json::to_string(&vec![rule]).unwrap(),
        ),
    ])
}

pub fn test_terminal_highlighting_in_integrated_session() -> Builder {
    new_builder()
        .with_user_defaults(highlighting_defaults())
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(clear_blocklist_to_remove_bootstrapped_blocks())
        .with_step(execute_command_successfully("echo ERROR"))
        .with_step(
            new_step_with_default_assertions("Highlight the completed command output")
                .add_named_assertion(
                    "Rendered output has the configured foreground without changing its text",
                    |app, window_id| {
                        let terminal = single_terminal_view_for_tab(app, window_id, 0);
                        terminal.read(app, |view, ctx| {
                            let model = view.model.lock();
                            let Some(index) = model
                                .block_list()
                                .last_matching_block_by_index(BlockFilter::commands())
                            else {
                                return AssertionOutcome::failure(
                                    "No completed command block".into(),
                                );
                            };
                            let block = model.block_list().block_at(index).unwrap();
                            let grid = block.output_grid().grid_handler();
                            let highlights = HighlightRuntime::as_ref(ctx).for_grid(
                                grid,
                                &model.colors(),
                                Appearance::as_ref(ctx).theme(),
                            );
                            let style = highlights.style_at(Point::new(0, 0), grid.columns());
                            async_assert!(
                                style.and_then(|style| style.foreground)
                                    == Some(warpui_core::color::ColorU::new(255, 0, 0, 255))
                                    && block
                                        .output_grid()
                                        .contents_to_string(false, None)
                                        .contains("ERROR"),
                                "Expected red highlight on unchanged ERROR output"
                            )
                        })
                    },
                ),
        )
        .with_step(screenshot("integrated-output.png"))
}

pub fn test_prompt_headers_without_shell_integration() -> Builder {
    let mut defaults = highlighting_defaults();
    let header = PromptHeaderConfig {
        enabled: true,
        background: "#102030".into(),
        foreground: Some("#FFFFFF".into()),
        fallback_regex: Some("^highlight> ".into()),
        ..Default::default()
    };
    defaults.insert(
        "PromptHeaderColors".into(),
        serde_json::to_string(&header).unwrap(),
    );
    new_builder().with_user_defaults(defaults)
        .set_should_run_test(|| !cfg!(windows))
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(clear_blocklist_to_remove_bootstrapped_blocks())
        .with_step(execute_long_running_command(0, "env PS1='highlight> ' bash --noprofile --norc -i".into()))
        .with_step(TestStep::new("Wait for the unintegrated shell prompt")
            .add_named_assertion("The raw shell prompt receives a separator", |app, window_id| {
                let terminal = single_terminal_view_for_tab(app, window_id, 0);
                terminal.read(app, |view, ctx| {
                    let model = view.model.lock();
                    let grid = model.block_list().active_block().output_grid().grid_handler();
                    let highlights = HighlightRuntime::as_ref(ctx).for_grid(grid, &model.colors(), Appearance::as_ref(ctx).theme());
                    async_assert!(!highlights.separators.is_empty(), "The raw prompt has no header decoration")
                })
            }))
        .with_step(TestStep::new("Run a command without installing shell hooks")
            .with_input_string("echo ERROR", Some(&["enter"]))
            .add_named_assertion("Successive prompts stay separated in the same output grid", |app, window_id| {
                let terminal = single_terminal_view_for_tab(app, window_id, 0);
                terminal.read(app, |view, ctx| {
                    let model = view.model.lock();
                    let block = model.block_list().active_block();
                    let grid = block.output_grid().grid_handler();
                    let highlights = HighlightRuntime::as_ref(ctx).for_grid(grid, &model.colors(), Appearance::as_ref(ctx).theme());
                    async_assert!(highlights.separators.len() >= 2
                        && block.output_grid().contents_to_string(false, None).contains("ERROR"),
                        "Expected two successive raw prompt headers and the unchanged ERROR output")
                })
            }))
        .with_step(screenshot("raw-prompt-headers.png"))
        .with_step(TestStep::new("Exit the unintegrated shell").with_input_string("exit", Some(&["enter"])))
}

pub fn test_terminal_highlighting_settings_preview() -> Builder {
    new_builder()
        .with_user_defaults(highlighting_defaults())
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(open_settings_page(SettingsSection::Appearance))
        .with_step(type_settings_search("prompt header"))
        .with_step(screenshot("prompt-header-settings.png"))
}

pub fn test_terminal_highlighting_rule_editor_saves_valid_drafts() -> Builder {
    new_builder()
        .with_user_defaults(highlighting_defaults())
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(open_settings_page(SettingsSection::Appearance))
        .with_step(type_settings_search("priority"))
        .with_step(
            TestStep::new("Add a highlight rule")
                .with_click_on_saved_position("highlight_editor:false:button:Add"),
        )
        .with_step(
            TestStep::new("Name the new rule")
                .with_click_on_saved_position("highlight_editor:false:field:0")
                .with_keystrokes(&["cmdorctrl-a"])
                .with_typed_characters(&["Warnings"]),
        )
        .with_step(
            TestStep::new("Enter an invalid regex")
                .with_click_on_saved_position("highlight_editor:false:field:1")
                .with_typed_characters(&["("]),
        )
        .with_step(
            TestStep::new("Reject the invalid draft")
                .with_click_on_saved_position("highlight_editor:false:button:Save")
                .add_named_assertion("Invalid draft does not replace saved rules", |app, _| {
                    app.read(|ctx| {
                        async_assert!(
                            HighlightSettings::as_ref(ctx).rules.len() == 1,
                            "Invalid draft changed persisted highlighting rules"
                        )
                    })
                }),
        )
        .with_step(
            TestStep::new("Correct the regex")
                .with_click_on_saved_position("highlight_editor:false:field:1")
                .with_keystrokes(&["cmdorctrl-a"])
                .with_typed_characters(&["WARN"]),
        )
        .with_step(
            TestStep::new("Save the valid draft")
                .with_click_on_saved_position("highlight_editor:false:button:Save")
                .add_named_assertion(
                    "The new rule is saved with a distinct stable ID",
                    |app, _| {
                        app.read(|ctx| {
                            let settings = HighlightSettings::as_ref(ctx);
                            async_assert!(
                                settings.rules.len() == 2
                                    && settings.rules[1].name == "Warnings"
                                    && settings.rules[1].pattern == "WARN"
                                    && settings.rules[0].id != settings.rules[1].id,
                                "The valid draft was not saved correctly"
                            )
                        })
                    },
                ),
        )
        .with_step(screenshot("highlight-rule-settings.png"))
}

pub fn test_prompt_headers_with_classic_ps1() -> Builder {
    let mut defaults = highlighting_defaults();
    defaults.insert("HonorPS1".into(), "true".into());
    defaults.insert(
        "PromptHeaderColors".into(),
        serde_json::to_string(&PromptHeaderConfig {
            enabled: true,
            background: "#102030".into(),
            foreground: Some("#FFFFFF".into()),
            ..Default::default()
        })
        .unwrap(),
    );
    new_builder()
        .with_user_defaults(defaults)
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(clear_blocklist_to_remove_bootstrapped_blocks())
        .with_step(execute_command_successfully("echo ERROR"))
        .with_step(
            new_step_with_default_assertions("Use known PS1 boundaries without a prompt regex")
                .add_named_assertion(
                    "Only the prompt foreground is overridden",
                    |app, window_id| {
                        let terminal = single_terminal_view_for_tab(app, window_id, 0);
                        terminal.read(app, |view, ctx| {
                            let model = view.model.lock();
                            let Some(index) = model
                                .block_list()
                                .last_matching_block_by_index(BlockFilter::commands())
                            else {
                                return AssertionOutcome::failure(
                                    "No completed command block".into(),
                                );
                            };
                            let block = model.block_list().block_at(index).unwrap();
                            let Some(end) = block.prompt_highlight_end() else {
                                return AssertionOutcome::failure(
                                    "No known prompt boundary".into(),
                                );
                            };
                            let grid = block.prompt_and_command_grid().grid_handler();
                            let highlights = HighlightRuntime::as_ref(ctx).for_grid(
                                grid,
                                &model.colors(),
                                Appearance::as_ref(ctx).theme(),
                            );
                            let prompt_style = highlights.style_at(end, grid.columns());
                            let command_start = Point::new(
                                end.row + (end.col + 1) / grid.columns(),
                                (end.col + 1) % grid.columns(),
                            );
                            let command_style = highlights.style_at(command_start, grid.columns());
                            async_assert!(
                                block.honor_ps1()
                                    && prompt_style.and_then(|style| style.foreground)
                                        == Some(warpui_core::color::ColorU::white())
                                    && command_style.is_some_and(|style| style
                                        .foreground
                                        .is_none()
                                        && style.background
                                            == Some(warpui_core::color::ColorU::new(
                                                16, 32, 48, 255
                                            ))),
                                "Expected metadata colors on PS1 and background only on the command"
                            )
                        })
                    },
                ),
        )
        .with_step(
            TestStep::new("Type into the active Classic prompt")
                .with_input_string("echo next", None),
        )
        .with_step(screenshot("classic-ps1-headers.png"))
}

fn screenshot(filename: &str) -> TestStep {
    let step = TestStep::new("Capture highlighted terminal output");
    if std::env::var_os("WARPUI_USE_REAL_DISPLAY_IN_INTEGRATION_TESTS").is_some() {
        step.with_take_screenshot(filename)
    } else {
        step
    }
}
