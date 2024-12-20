use crate::widgets::report::diagnostics_column;
use crate::workspace::editors::utils::{unsupported, EditorResultExt, EditorSize};
use crate::workspace::editors::{editor_for_type, DynProps, Editor, EditorResponse};
use dbe_backend::diagnostic::context::DiagnosticContextRef;
use dbe_backend::registry::ETypesRegistry;
use dbe_backend::value::EValue;
use egui::collapsing_header::CollapsingState;
use egui::{Ui, Widget};
use miette::miette;

#[derive(Debug)]
pub struct ListEditor;

impl Editor for ListEditor {
    fn size(&self, _props: &DynProps) -> EditorSize {
        EditorSize::Block
    }

    fn edit(
        &self,
        ui: &mut Ui,
        reg: &ETypesRegistry,
        mut diagnostics: DiagnosticContextRef,
        field_name: &str,
        value: &mut EValue,
        _props: &DynProps,
    ) -> EditorResponse {
        let EValue::List { values, id } = value else {
            unsupported!(ui, field_name, value, self);
        };

        let mut changed = false;

        reg.get_list(id)
            .ok_or_else(|| miette!("!!INTERNAL ERROR!! unknown list `{}`", id))
            .then_draw(ui, |ui, list_data| {
                CollapsingState::load_with_default_open(
                    ui.ctx(),
                    ui.id().with(field_name),
                    values.len() < 20,
                )
                .show_header(ui, |ui| {
                    egui::Label::new(field_name).selectable(false).ui(ui)
                })
                .body_unindented(|ui| {
                    let ty = list_data.value_type;
                    let editor = editor_for_type(reg, &ty);
                    list_edit::list_editor::<EValue, _>(ui.id().with(field_name).with("list"))
                        .new_item(|_| ty.default_value(reg).into_owned())
                        .show(ui, values, |ui, i, val| {
                            let mut d = diagnostics.enter_index(i.index);
                            if editor.show(ui, reg, d.enter_inline(), "", val).changed {
                                changed = true;
                            }

                            diagnostics_column(ui, d.get_reports_shallow());
                        });
                });
            });

        EditorResponse::new(changed)
    }
}
