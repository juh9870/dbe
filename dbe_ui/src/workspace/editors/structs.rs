use crate::widgets::report::diagnostics_column;
use crate::workspace::editors::utils::{
    labeled_field, prop, unsupported, EditorResultExt, EditorSize,
};
use crate::workspace::editors::{
    cast_props, editor_for_item, DynProps, Editor, EditorProps, EditorResponse,
};
use dbe_backend::diagnostic::context::DiagnosticContextRef;
use dbe_backend::etype::eitem::EItemInfo;
use dbe_backend::registry::ETypesRegistry;
use dbe_backend::value::EValue;
use egui::{Ui, Widget};
use itertools::Itertools;
use miette::miette;

#[derive(Debug)]
pub struct StructEditor;

impl Editor for StructEditor {
    fn props(&self, _reg: &ETypesRegistry, item: Option<&EItemInfo>) -> miette::Result<DynProps> {
        if prop(item.map(|i| i.extra_properties()), "inline", false)? {
            Ok(StructProps { inline: true }.pack())
        } else {
            Ok(StructProps { inline: false }.pack())
        }
    }

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
        props: &DynProps,
    ) -> EditorResponse {
        let EValue::Struct { fields, ident } = value else {
            unsupported!(ui, field_name, value, self);
        };

        let props = cast_props::<StructProps>(props);

        let mut changed = false;
        reg.get_struct(ident)
            .ok_or_else(|| miette!("unknown struct `{}`", ident))
            .then_draw(ui, |ui, data| {
                let items = data
                    .fields
                    .iter()
                    .map(|f| (f, editor_for_item(reg, &f.ty)))
                    .collect_vec();

                let inline =
                    items.len() <= 3 && items.iter().all(|f| f.1.size() <= EditorSize::Inline);

                let draw_fields = |ui: &mut Ui| {
                    for (field, editor) in items {
                        fields
                            .get_mut(&field.name)
                            .ok_or_else(|| miette!("field `{}` is missing", field.name))
                            .then_draw(ui, |ui, value| {
                                let mut d = diagnostics.enter_field(field.name.as_str());
                                if editor
                                    .show(ui, reg, d.enter_inline(), field.name.as_ref(), value)
                                    .changed
                                {
                                    changed = true;
                                };
                                diagnostics_column(ui, d.get_reports_shallow())
                            });
                    }
                };

                if inline {
                    ui.horizontal(|ui| {
                        labeled_field(ui, field_name, draw_fields);
                    });
                } else if props.inline {
                    draw_fields(ui);
                } else if field_name.is_empty() {
                    ui.group(|ui| {
                        ui.vertical(|ui| {
                            draw_fields(ui);
                        })
                    });
                } else {
                    egui::collapsing_header::CollapsingState::load_with_default_open(
                        ui.ctx(),
                        ui.id().with(field_name),
                        true,
                    )
                    .show_header(ui, |ui| {
                        egui::Label::new(field_name).selectable(false).ui(ui);
                    })
                    .body(|ui| {
                        ui.vertical(|ui| {
                            draw_fields(ui);
                        })
                    });
                }
            });

        EditorResponse::new(changed)
    }
}

#[derive(Debug, Clone)]
struct StructProps {
    inline: bool,
}

impl EditorProps for StructProps {}