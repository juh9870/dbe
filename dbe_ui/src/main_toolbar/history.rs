use crate::DbeApp;
use dbe_backend::project::undo::{FileSnapshot, SnapshotKind};
use egui::{Button, Ui};

pub fn history_tab(ui: &mut Ui, app: &mut DbeApp) {
    ui.label("Undo History");

    let height =
        ui.text_style_height(&egui::TextStyle::Button) + ui.style().spacing.button_padding.y * 2.0;

    app.undo_buttons(ui);

    let Some(project) = &mut app.project else {
        ui.label("No project loaded");
        return;
    };

    enum Entry<'a> {
        Past(&'a FileSnapshot),
        UndoneHistory(&'a FileSnapshot),
        Future(&'a FileSnapshot),
    }

    fn text_for_snapshot(snapshot: &FileSnapshot) -> String {
        match snapshot.kind {
            SnapshotKind::Change => {
                format!(
                    "{}: Changed: {} ({})",
                    snapshot.id, snapshot.path, snapshot.state
                )
            }
            SnapshotKind::Undo(target) => {
                if target == snapshot.id {
                    format!(
                        "Undid change #{} to: {} ({})",
                        snapshot.id, snapshot.path, snapshot.state
                    )
                } else {
                    format!(
                        "{} Undid change #{} to: {} ({})",
                        snapshot.id, target, snapshot.path, snapshot.state
                    )
                }
            }
        }
    }

    let history = project.history.history();
    let undone = project.history.undone_history();
    let future = project.history.future();
    let count = history.len() + undone.len() + future.len();
    let items = history
        .map(Entry::Past)
        .chain(undone.map(Entry::UndoneHistory))
        .chain(future.map(Entry::Future));

    egui::ScrollArea::vertical()
        .min_scrolled_height(height * 10.0)
        .show_rows(ui, height, count, |ui, x| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            for i in items.skip(x.start).take(x.len()) {
                match i {
                    Entry::Past(snapshot) => {
                        ui.add_enabled(false, Button::new(text_for_snapshot(snapshot)));
                    }
                    Entry::UndoneHistory(snapshot) => {
                        ui.indent(&snapshot.path, |ui| {
                            ui.add_enabled(false, Button::new(text_for_snapshot(snapshot)));
                        });
                    }
                    Entry::Future(snapshot) => {
                        ui.indent(&snapshot.path, |ui| {
                            ui.add_enabled(false, Button::new(text_for_snapshot(snapshot)));
                        });
                    }
                }
            }
        });
}
