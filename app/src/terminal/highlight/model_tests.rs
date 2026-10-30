use std::time::Duration;

use settings::Setting;
use warp_terminal::model::ansi::{Handler, LineClearMode, ProcessorInput};
use warpui::App;

use super::*;
use crate::terminal::highlight::settings::TextHighlightRule;
use crate::test_util::settings::initialize_settings_for_tests;
use crate::themes::default_themes::dark_theme;

fn highlights(app: &App, grid: &GridHandler) -> GridHighlights {
    let theme = dark_theme();
    let colors = color::List::from(&color::Colors::from(theme.clone()));
    app.read(|ctx| HighlightRuntime::as_ref(ctx).for_grid(grid, &colors, &theme))
}

async fn matched(app: &App, grid: &GridHandler) -> GridHighlights {
    futures_lite::future::or(
        async {
            loop {
                let result = highlights(app, grid);
                if !result.pending {
                    return result;
                }
                futures_lite::future::yield_now().await;
            }
        },
        async {
            warpui::r#async::Timer::after(Duration::from_secs(5)).await;
            panic!("highlight matching did not finish");
        },
    )
    .await
}

fn set_rule(app: &mut App, pattern: &str) {
    let rule = TextHighlightRule {
        pattern: pattern.into(),
        foreground: Some("#FF0000".into()),
        ..Default::default()
    };
    HighlightSettings::handle(app).update(app, |settings, ctx| {
        settings.rules.set_value(vec![rule], ctx).unwrap();
    });
}

fn grid() -> GridHandler {
    GridHandler::new(
        crate::terminal::SizeInfo::new_without_font_metrics(4, 16),
        0,
        warp_terminal::event_listener::ChannelEventListener::new_for_test(),
        false,
        crate::terminal::model::ObfuscateSecrets::No,
        warp_terminal::model::grid::grid_handler::PerformResetGridChecks::No,
    )
}

#[test]
fn partial_output_and_rewrites_invalidate_rendered_styles_immediately() {
    App::test((), |mut app| async move {
        let _flag = FeatureFlag::TerminalHighlighting.override_enabled(true);
        initialize_settings_for_tests(&mut app);
        set_rule(&mut app, "ERROR");
        let mut grid = grid();
        grid.input('E');
        grid.input('R');
        grid.on_finish_byte_processing(&ProcessorInput::new(&[]));
        app.read(|ctx| HighlightRuntime::as_ref(ctx).prepare_grid(&grid, 0..1));
        assert_eq!(
            matched(&app, &grid).await.style_at(Point::new(0, 0), 16),
            None
        );

        grid.input('R');
        grid.input('O');
        grid.input('R');
        grid.on_finish_byte_processing(&ProcessorInput::new(&[]));
        assert!(highlights(&app, &grid).pending);
        app.read(|ctx| HighlightRuntime::as_ref(ctx).prepare_grid(&grid, 0..1));
        assert_eq!(
            matched(&app, &grid)
                .await
                .style_at(Point::new(0, 0), 16)
                .unwrap()
                .foreground,
            Some(ColorU::new(255, 0, 0, 255))
        );

        grid.carriage_return();
        grid.clear_line(LineClearMode::All);
        grid.input('O');
        grid.input('K');
        grid.on_finish_byte_processing(&ProcessorInput::new(&[]));
        assert_eq!(highlights(&app, &grid).style_at(Point::new(0, 0), 16), None);
        app.read(|ctx| HighlightRuntime::as_ref(ctx).prepare_grid(&grid, 0..1));
        assert_eq!(
            matched(&app, &grid).await.style_at(Point::new(0, 0), 16),
            None
        );
    });
}

#[test]
fn changing_rules_discards_pending_results_from_the_previous_configuration() {
    App::test((), |mut app| async move {
        let _flag = FeatureFlag::TerminalHighlighting.override_enabled(true);
        initialize_settings_for_tests(&mut app);
        set_rule(&mut app, "ERROR");
        let mut grid = grid();
        grid.input('E');
        grid.input('R');
        grid.input('R');
        grid.input('O');
        grid.input('R');
        app.read(|ctx| HighlightRuntime::as_ref(ctx).prepare_grid(&grid, 0..1));

        set_rule(&mut app, "WARN");
        app.read(|ctx| HighlightRuntime::as_ref(ctx).prepare_grid(&grid, 0..1));
        assert_eq!(
            matched(&app, &grid).await.style_at(Point::new(0, 0), 16),
            None
        );
    });
}
