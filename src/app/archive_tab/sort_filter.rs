//! The menus in the archive table's header: how to order the letters by date, and which
//! category to show. They only change the app's choices; the database does the sorting
//! and filtering (see `pdf_crud/sort_filter.rs`), and the app reloads the table after.

use crate::pdf_crud::DateOrder;
use eframe::egui;

/// Every date order, in the order the menu shows them.
const DATE_ORDERS: [DateOrder; 2] = [DateOrder::NewestFirst, DateOrder::OldestFirst];

/// The menu's text for an order.
fn label(order: DateOrder) -> &'static str {
    match order {
        DateOrder::NewestFirst => "Newest first",
        DateOrder::OldestFirst => "Oldest first",
    }
}

/// The menu under the "Letter date" header: how to order the table.
/// Clicking a choice changes `order` directly.
pub fn date_order_menu(ui: &mut egui::Ui, order: &mut DateOrder) {
    // `from_id_salt`: a menu with no label beside it; the header above names it.
    egui::ComboBox::from_id_salt("date_order")
        .selected_text(label(*order)) // `*`: a copy of the order `order` points to
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for choice in DATE_ORDERS {
                ui.selectable_value(order, choice, label(choice));
            }
        });
}

/// The menu under the "Category" header: show all letters or one category.
/// Clicking a choice changes `category` directly; `None` means all categories.
pub fn category_menu(ui: &mut egui::Ui, category: &mut Option<String>, categories: &[String]) {
    // `clone`: an owned copy for the button text, so `category` is free to be changed below.
    let shown = category.clone().unwrap_or_else(|| "All".to_string());
    egui::ComboBox::from_id_salt("category")
        .selected_text(shown)
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            ui.selectable_value(category, None, "All");
            for name in categories {
                ui.selectable_value(category, Some(name.clone()), name.as_str());
            }
        });
}
