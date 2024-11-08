use crate::DbeApp;
use camino::{Utf8Path, Utf8PathBuf};
use dbe2::project::Project;
use egui::{CollapsingHeader, Label, RichText, Sense, Ui};
use inline_tweak::tweak;
use itertools::Itertools;
use std::iter::Peekable;

#[derive(Debug)]
enum Command {
    OpenFile { path: Utf8PathBuf },
}

pub fn file_tab(ui: &mut Ui, app: &mut DbeApp) {
    egui::ScrollArea::both()
        .auto_shrink(tweak!(false))
        .show(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            if let Some(project) = &mut app.project {
                let commands = file_tree(ui, project);
                consume_commands(commands, app);
            } else {
                if ui.button("Open Project").clicked() {
                    app.open_project();
                }

                ui.group(|ui| {
                    ui.label("Recent projects");
                    app.history_button_list(ui);
                });
            }
        });
}

fn file_tree(ui: &mut Ui, project: &mut Project) -> Vec<Command> {
    let mut commands = vec![];

    show_folder(
        ui,
        "".as_ref(),
        &mut project.files.keys().peekable(),
        &|_| false,
        &mut commands,
    );

    commands
}

fn consume_commands(commands: Vec<Command>, app: &mut DbeApp) {
    for cmd in commands {
        match cmd {
            Command::OpenFile { path } => app.open_tab_for(path),
        }
    }
}

fn show_folder(
    ui: &mut Ui,
    path: &Utf8Path,
    fs: &mut Peekable<impl Iterator<Item = impl AsRef<Utf8Path>>>,
    disabled: &impl Fn(&Utf8Path) -> bool,
    commands: &mut Vec<Command>,
) {
    let is_enabled = !disabled(path);
    let mut header = RichText::new(path.file_name().unwrap_or("Project Root"));
    if !is_enabled {
        header = header.color(ui.style().visuals.widgets.noninteractive.text_color())
    }
    let response = CollapsingHeader::new(header)
        // .enabled(is_enabled)
        .default_open(is_enabled)
        .show(ui, |ui| {
            let mut files = vec![];
            let mut folders = vec![];
            while let Some(next) = fs.peek().map(|e| e.as_ref().to_path_buf()) {
                let Ok(remaining) = next.strip_prefix(path) else {
                    break;
                };
                match remaining.components().at_most_one() {
                    Ok(file_name) => {
                        let Some(file_name) = file_name else {
                            panic!("File matches directory name: `{}`", next);
                        };
                        fs.next();
                        let name = file_name.to_string();
                        files.push((next, name));
                    }
                    Err(mut iter) => {
                        let sub_path = path.join(iter.next().expect("Should not be empty"));
                        let mut folder_items = vec![];
                        while fs
                            .peek()
                            .map(|e| e.as_ref().starts_with(&sub_path))
                            .unwrap_or(false)
                        {
                            folder_items.push(fs.next().expect("Peeked item should be present"));
                        }
                        folders.push((sub_path, folder_items));
                    }
                }
            }

            for (sub_path, folder) in folders {
                show_folder(
                    ui,
                    &sub_path,
                    &mut folder.into_iter().peekable(),
                    disabled,
                    commands,
                );
            }
            for (file, file_name) in files {
                if ui
                    .add_enabled(
                        is_enabled,
                        Label::new(file_name)
                            .sense(Sense::click())
                            .selectable(false),
                    )
                    .double_clicked()
                {
                    commands.push(Command::OpenFile {
                        path: file.to_path_buf(),
                    });
                }
            }
        });

    if is_enabled {
        response
            .header_response
            .context_menu(|ui| folder_context_menu(ui, path));
    }
}

fn folder_context_menu(ui: &mut Ui, _path: &Utf8Path) {
    if ui.button("New File").clicked() {
        todo!("Create new File");
        // commands.push(TabCommand::CreateNewFile {
        //     parent_folder: path.to_path_buf(),
        // });
        // ui.close_menu()
    }
}
