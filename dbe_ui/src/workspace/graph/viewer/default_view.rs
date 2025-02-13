use crate::error::report_error;
use crate::main_toolbar::docs::{docs_hover_type, docs_label};
use crate::workspace::editors::{editor_for_item, EditorContext};
use crate::workspace::graph::viewer::default_view::inline_popout::show_input_popout_menu;
use crate::workspace::graph::viewer::default_view::state_editor::show_state_editor;
use crate::workspace::graph::viewer::NodeView;
use crate::workspace::graph::{any_pin, pin_info, GraphViewer};
use dbe_backend::graph::node::SnarlNode;
use dbe_backend::project::docs::{DocsRef, DocsWindowRef};
use dbe_backend::value::EValue;
use egui::{InnerResponse, Ui};
use egui_snarl::ui::{NodeLayout, PinInfo};
use egui_snarl::{InPin, NodeId, OutPin, Snarl};
use std::fmt::Debug;
use ustr::Ustr;

pub mod inline_popout;
pub mod state_editor;

#[derive(Debug)]
pub struct DefaultNodeView;

pub fn format_value(value: &EValue) -> String {
    match value {
        EValue::Null => "null".to_string(),
        EValue::Boolean { value } => value.to_string(),
        EValue::Number { value } => format!("{:.5}", value)
            .trim_end_matches(['0', '.'])
            .to_string(),
        EValue::String { value } => {
            if value.len() > 8 {
                format!("{:?}...", &value[..8])
            } else {
                format!("{:?}", value)
            }
        }
        EValue::Struct { .. } => "".to_string(),
        EValue::Enum { .. } => "".to_string(),
        EValue::List { .. } => "".to_string(),
        EValue::Map { .. } => "".to_string(),
    }
}

impl NodeView for DefaultNodeView {
    fn id(&self) -> Ustr {
        "default".into()
    }

    fn show_header(
        &self,
        viewer: &GraphViewer,
        node_id: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        _scale: f32,
        snarl: &mut Snarl<SnarlNode>,
    ) -> miette::Result<()> {
        let node = &snarl[node_id];

        let res =
            ui.label(node_id.0.to_string()) | ui.label(node.title(viewer.ctx.as_node_context()));

        docs_hover_type(
            ui,
            res,
            "header",
            viewer.ctx.docs,
            viewer.ctx.registry,
            DocsWindowRef::Node(node.id()),
        );

        Ok(())
    }

    fn show_input(
        &self,
        viewer: &mut GraphViewer,
        pin: &InPin,
        ui: &mut Ui,
        _scale: f32,
        snarl: &mut Snarl<SnarlNode>,
    ) -> miette::Result<InnerResponse<PinInfo>> {
        let mut has_connection = false;
        let res = ui.push_id(pin.id, |ui| -> miette::Result<_> {
            let registry = viewer.ctx.registry;
            let docs = viewer.ctx.docs;
            let node = &snarl[pin.id.node];
            let node_ident = node.id();
            let input_data = node.try_input(viewer.ctx.as_node_context(), pin.id.input)?;

            let docs_ref = input_data
                .custom_docs
                .unwrap_or(DocsRef::NodeInput(node_ident, input_data.name));
            let ctx = EditorContext::new(registry, docs, docs_ref);

            let Some(info) = input_data.ty.item_info() else {
                docs_label(ui, &input_data.name, docs, registry, ctx.docs_ref);
                return Ok(any_pin());
            };

            if pin.remotes.is_empty() {
                let mut full_ctx = viewer.ctx.as_full(snarl);
                if let Some(value) = full_ctx.get_inline_input_mut(pin.id)? {
                    if input_data.ty.has_inline_value(registry) {
                        if value.ty() != input_data.ty.ty() {
                            *value = input_data.ty.default_value(registry).into_owned();
                        }

                        let editor = editor_for_item(registry, info);
                        ui.vertical(|ui| {
                            editor.show(
                                ui,
                                ctx,
                                viewer.diagnostics.enter_field(input_data.name.as_str()),
                                &input_data.name,
                                value,
                            )
                        });
                    } else {
                        ui.horizontal(|ui| {
                            ui.label(format_value(value));
                        });
                    }
                } else {
                    docs_label(ui, &input_data.name, docs, registry, ctx.docs_ref);
                }
            } else {
                has_connection = true;
                docs_label(ui, &input_data.name, docs, registry, ctx.docs_ref);
            }

            Ok(pin_info(&input_data.ty, registry))
        });
        let inner = res.inner?;

        res.response.context_menu(|ui| {
            ui.add_enabled_ui(!has_connection, |ui| {
                ui.menu_button("Inline Node", |ui| {
                    show_input_popout_menu(viewer, snarl, pin, ui);
                })
            });
        });

        Ok(InnerResponse::new(inner, res.response))
    }

    fn show_output(
        &self,
        viewer: &mut GraphViewer,
        pin: &OutPin,
        ui: &mut Ui,
        _scale: f32,
        snarl: &mut Snarl<SnarlNode>,
    ) -> miette::Result<InnerResponse<PinInfo>> {
        let res = ui.push_id(pin.id, |ui| -> miette::Result<_> {
            let registry = viewer.ctx.registry;
            let node = &snarl[pin.id.node];
            let output_data = node.try_output(viewer.ctx.as_node_context(), pin.id.output)?;
            let docs = viewer.ctx.docs;
            let docs_ref = output_data
                .custom_docs
                .unwrap_or_else(|| DocsRef::NodeOutput(node.id(), output_data.name));
            ui.horizontal(|ui| {
                docs_label(ui, &output_data.name, docs, registry, docs_ref);
            });

            Ok(pin_info(&output_data.ty, registry))
        });

        let inner = res.inner?;
        Ok(InnerResponse::new(inner, res.response))
    }

    fn node_layout(&self, _viewer: &mut GraphViewer, _node: &SnarlNode) -> NodeLayout {
        NodeLayout::FlippedSandwich
    }

    fn has_body(&self, _viewer: &mut GraphViewer, node: &SnarlNode) -> miette::Result<bool> {
        Ok(node.has_editable_state())
    }

    fn show_body(
        &self,
        viewer: &mut GraphViewer,
        node_id: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        _scale: f32,
        snarl: &mut Snarl<SnarlNode>,
    ) -> miette::Result<()> {
        let node = &mut snarl[node_id];
        let mut state = node.editable_state();

        let res = show_state_editor(ui, viewer, node.id(), &mut state)?;

        if res.changed {
            node.apply_editable_state(
                viewer.ctx.as_node_context(),
                state,
                &mut viewer.commands,
                node_id,
            )
            .unwrap_or_else(report_error);
        }

        Ok(())
    }
}
