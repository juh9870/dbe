use crate::error::report_error;
use crate::main_toolbar::{ToolPanel, ToolPanelViewer};
use crate::widgets::collapsible_toolbar::CollapsibleToolbar;
use crate::widgets::dpanel::DPanelSide;
use crate::workspace::Tab;
use ahash::AHashMap;
use dbe_backend::project::io::FilesystemIO;
use dbe_backend::project::Project;
use egui::{Align2, Color32, Context, FontData, FontDefinitions, FontFamily, Id, Ui};
use egui_colors::Colorix;
use egui_dock::DockState;
use egui_file::FileDialog;
use egui_toast::{Toast, ToastKind, ToastOptions, Toasts};
use egui_tracing::EventCollector;
use inline_tweak::tweak;
use itertools::Itertools;
use miette::{IntoDiagnostic, WrapErr};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use tracing::info;

mod diagnostics_list;
mod error;
mod file_tree;
pub mod main_toolbar;
mod ui_props;
pub mod widgets;
mod workspace;

/// A function that can be called to show a modal
///
/// The function should return true if the modal is done and should no
/// longer be called
type ModalFn = Box<dyn FnMut(&mut DbeApp, &Context) -> bool>;

pub struct DbeApp {
    project: Option<Project<FilesystemIO>>,
    open_file_dialog: Option<FileDialog>,
    collector: EventCollector,
    toasts: Vec<Toast>,
    modals: AHashMap<&'static str, ModalFn>,
    history: Vec<PathBuf>,
    tabs: DockState<Tab>,

    // Theming
    colorix: Colorix,
    dark_mode: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AppStorage {
    #[serde(default)]
    history: Vec<PathBuf>,
    #[serde(default)]
    theme: egui_colors::Theme,
    #[serde(default)]
    dark_mode: bool,
}

static ERROR_HAPPENED: AtomicBool = AtomicBool::new(false);

impl DbeApp {
    pub fn register_fonts(ctx: &Context) {
        let mut fonts = FontDefinitions::default();
        fonts.font_data.insert(
            "fira-code".to_owned(),
            FontData::from_static(include_bytes!("../../assets/fonts/FiraCode-Light.ttf")),
        );

        fonts
            .families
            .get_mut(&FontFamily::Proportional)
            .unwrap()
            .insert(0, "fira-code".to_owned());

        fonts
            .families
            .get_mut(&FontFamily::Proportional)
            .unwrap()
            .insert(1, "fira-code".to_owned());

        fonts
            .families
            .get_mut(&FontFamily::Monospace)
            .unwrap()
            .insert(0, "fira-code".to_owned());

        ctx.set_fonts(fonts);
    }

    pub fn new(collector: EventCollector) -> Self {
        ui_props::register_extra_properties();

        Self {
            colorix: Default::default(),
            project: None,
            open_file_dialog: None,
            collector,
            toasts: vec![],
            modals: Default::default(),
            history: vec![],
            tabs: DockState::new(vec![]),
            dark_mode: true,
        }
    }

    pub fn load_storage(&mut self, ctx: &Context, value: &str) {
        match serde_json5::from_str::<AppStorage>(value)
            .into_diagnostic()
            .context("Failed to load persistent app storage")
        {
            Ok(storage) => {
                self.history = storage.history;
                if let Some(head) = self.history.first() {
                    self.load_project_from_path(head.clone())
                }

                self.colorix = Colorix::init(ctx, storage.theme);

                ctx.set_visuals(egui::Visuals {
                    dark_mode: storage.dark_mode,
                    ..Default::default()
                });
            }
            Err(err) => {
                report_error(err);
            }
        };
    }

    pub fn save_storage(&self) -> Option<String> {
        match serde_json5::to_string(&AppStorage {
            history: self.history.clone(),
            theme: *self.colorix.theme(),
            dark_mode: self.dark_mode,
        })
        .into_diagnostic()
        .context("Failed to save persistent app storage")
        {
            Ok(storage) => Some(storage),
            Err(err) => {
                report_error(err);
                None
            }
        }
    }

    pub fn update(&mut self, ctx: &Context) {
        #[cfg(debug_assertions)]
        {
            ctx.set_debug_on_hover(tweak!(false));
        }

        static INIT: OnceLock<()> = OnceLock::new();

        INIT.get_or_init(|| {
            self.colorix = Colorix::init(ctx, *self.colorix.theme());
        });

        self.dark_mode = ctx.style().visuals.dark_mode;
        self.colorix.draw_background(ctx, false);

        // self.colorix = Colorix::init(ctx, [ColorPreset::Red; 12]);

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:

            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui
                        .add_enabled(self.project.is_some(), egui::Button::new("Save"))
                        .clicked()
                    {
                        self.save_project();
                        ui.close_menu();
                    }

                    if ui.button("Open").clicked() {
                        self.open_project();
                        ui.close_menu();
                    }

                    ui.menu_button("Recent Projects", |ui| {
                        self.history_button_list(ui);
                    });

                    if ui
                        .add_enabled(self.project.is_some(), egui::Button::new("Close Project"))
                        .clicked()
                    {
                        self.project = None;
                        ui.close_menu();
                    }

                    #[cfg(not(target_arch = "wasm32"))]
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        ui.close_menu();
                    }
                });
                ui.add_space(16.0);

                if ui.button("Clear All").clicked() {
                    // self.snarl = Default::default();
                }
            });
        });

        let global_drag_id = Id::from("dbe_toolbar_global_drag");
        CollapsibleToolbar::new(DPanelSide::Bottom, &[ToolPanel::Log], &[])
            .default_selected_start(0)
            .global_drag_id(global_drag_id)
            .show(ctx, "bottom_toolbar", &mut ToolPanelViewer(self));

        CollapsibleToolbar::new(
            DPanelSide::Left,
            &[ToolPanel::ProjectTree, ToolPanel::Theme],
            &[],
        )
        .default_selected_start(0)
        .global_drag_id(global_drag_id)
        .show(ctx, "left_toolbar", &mut ToolPanelViewer(self));

        CollapsibleToolbar::new(DPanelSide::Right, &[ToolPanel::Diagnostics], &[])
            .default_selected_start(0)
            .global_drag_id(global_drag_id)
            .show(ctx, "right_toolbar", &mut ToolPanelViewer(self));

        egui::CentralPanel::default().show(ctx, |ui| workspace::workspace(ui, self));

        if let Some(dialog) = &mut self.open_file_dialog {
            if dialog.show(ctx).selected() {
                if let Some(file) = dialog.path() {
                    let file = file.to_path_buf();
                    self.load_project_from_path(file)
                }
            }
        }

        if ERROR_HAPPENED.swap(false, Ordering::Acquire) {
            self.toasts.push(Toast {
                kind: ToastKind::Error,
                text: "An error has occurred, see console for details".into(),
                options: ToastOptions::default()
                    .duration_in_seconds(3.0)
                    .show_progress(true),
                style: Default::default(),
            });
        }

        let mut toasts = Toasts::new()
            .anchor(Align2::RIGHT_TOP, (-10.0, -10.0))
            .direction(egui::Direction::TopDown);
        for toast in self.toasts.drain(..) {
            toasts.add(toast);
        }
        toasts.show(ctx);

        let mut modals = std::mem::take(&mut self.modals);
        for (_, modal) in &mut modals {
            modal(self, ctx);
        }
        self.modals = modals;
    }

    fn history_button_list(&mut self, ui: &mut Ui) {
        if self.history.is_empty() {
            ui.colored_label(Color32::GRAY, "No recent projects");
            return;
        }

        let mut want_open: Option<PathBuf> = None;
        for path in &self.history {
            let last = path
                .components()
                .filter(|c| !c.as_os_str().is_empty())
                .last()
                .unwrap();
            if ui.button(last.as_os_str().to_string_lossy()).clicked() {
                want_open = Some(path.clone());
                ui.close_menu();
            }
        }
        if let Some(path) = want_open {
            self.load_project_from_path(path);
        }
    }

    fn open_project(&mut self) {
        let mut dialog = FileDialog::select_folder(
            self.project
                .as_ref()
                .map(|p| p.root.as_std_path().to_path_buf()),
        );
        dialog.open();
        self.open_file_dialog = Some(dialog);
    }

    fn save_project(&mut self) {
        if let Some(project) = &mut self.project {
            match project.save() {
                Ok(_) => {
                    info!("Project saved successfully");
                    self.toasts.push(Toast {
                        kind: ToastKind::Success,
                        text: "Project saved successfully".into(),
                        options: ToastOptions::default()
                            .duration_in_seconds(3.0)
                            .show_progress(true),
                        style: Default::default(),
                    })
                }
                Err(error) => {
                    report_error(error);
                }
            }
        }
    }

    fn load_project_from_path(&mut self, path: impl AsRef<Path>) {
        let path = path.as_ref().to_path_buf();
        self.remember_last_project(path.clone());
        match Project::from_path(&path) {
            Ok(data) => {
                self.project = Some(data);
                info!(path=%path.display(), "Project loaded successfully");
                self.toasts.push(Toast {
                    kind: ToastKind::Success,
                    text: "Project loaded successfully".into(),
                    options: ToastOptions::default()
                        .duration_in_seconds(3.0)
                        .show_progress(true),
                    style: Default::default(),
                })
            }
            Err(err) => {
                report_error(err);
            }
        }
    }

    fn remember_last_project(&mut self, path: PathBuf) {
        let index = self.history.iter().find_position(|p| *p == &path);
        if let Some((index, _)) = index {
            self.history.remove(index);
        }

        self.history.insert(0, path);
    }
}

/// Helper for wrapping a code block to help with contextualizing errors
/// Better editor support but slightly worse ergonomic than a macro
#[inline(always)]
pub(crate) fn m_try<T>(func: impl FnOnce() -> miette::Result<T>) -> miette::Result<T> {
    func()
}
