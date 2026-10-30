use warpui::App;

use super::*;
use crate::settings_view::settings_page::{FilteredPageType, PageType};

#[test]
fn highlighting_settings_search_isolates_specific_controls() {
    App::test((), |app| async move {
        let mut page = PageType::<AppearanceSettingsPageView>::new_uncategorized(
            vec![
                Box::new(HighlightToggleWidget::default()),
                Box::new(HighlightRulesWidget),
                Box::new(PromptHeaderToggleWidget::default()),
                Box::new(PromptHeaderWidget),
            ],
            None,
        );
        app.read(|ctx| {
            page.update_filter("priority", ctx);
        });
        let FilteredPageType::Uncategorized { widgets, .. } = page.get_filtered() else {
            panic!("expected widget list");
        };
        assert_eq!(widgets.len(), 1);
        app.read(|ctx| {
            page.update_filter("foreground", ctx);
        });
        let FilteredPageType::Uncategorized { widgets, .. } = page.get_filtered() else {
            panic!("expected widget list");
        };
        assert_eq!(widgets.len(), 1);
        app.read(|ctx| {
            page.update_filter("", ctx);
        });
        let FilteredPageType::Uncategorized { widgets, .. } = page.get_filtered() else {
            panic!("expected widget list");
        };
        assert_eq!(widgets.len(), 4);
    });
}
