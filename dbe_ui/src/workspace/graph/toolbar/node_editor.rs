use crate::main_toolbar::colors::colorix_editor;
use crate::main_toolbar::docs::docs_label;
use crate::workspace::graph::toolbar::edit_inputs::edit_io;
use dbe_backend::graph::node::colors::NodeColorScheme;
use dbe_backend::graph::region::RegionVariable;
use dbe_backend::graph::Graph;
use dbe_backend::project::docs::{Docs, DocsRef};
use dbe_backend::registry::ETypesRegistry;
use egui::{CollapsingHeader, ScrollArea, Ui};
use egui_colors::Colorix;
use egui_snarl::NodeId;
use uuid::Uuid;

pub fn edit_node_properties(
    ui: &mut Ui,
    registry: &ETypesRegistry,
    docs: &Docs,
    graph: &mut Graph,
    selected_nodes: &[NodeId],
) {
    ui.label("Node Properties");
    ui.label("Select a node to edit its properties");
    ui.label("SHIFT+Click to select nodes");
    ui.label("CTRL+Click to deselect nodes");

    ScrollArea::vertical().show(ui, |ui| {
        CollapsingHeader::new("Node")
            .default_open(true)
            .show(ui, |ui| {
                edit_node(ui, registry, docs, graph, selected_nodes);
            });
        CollapsingHeader::new("Region")
            .default_open(true)
            .show(ui, |ui| {
                edit_node_region(ui, registry, docs, graph, selected_nodes);
            });
    });
}

pub fn edit_node(
    ui: &mut Ui,
    registry: &ETypesRegistry,
    docs: &Docs,
    graph: &mut Graph,
    selected_nodes: &[NodeId],
) {
    let Some(node_id) = exactly_one_node(ui, selected_nodes) else {
        return;
    };

    let (snarl, context) = graph.snarl_and_context(registry, docs);
    let node = &mut snarl[node_id];

    ui.horizontal(|ui| {
        let default_name = node.title(context);
        edit_opt(
            ui,
            &mut node.custom_title,
            default_name,
            |ui| {
                docs_label(
                    ui,
                    "Name",
                    docs,
                    registry,
                    DocsRef::Custom("Name of the node as visible in graph editor".into()),
                );
            },
            |ui, name| {
                ui.text_edit_singleline(name);
            },
        );
    });

    let default_scheme = NodeColorScheme {
        theme: Box::new(Colorix::local(ui, Default::default())),
        dark_mode: ui.ctx().style().visuals.dark_mode,
    };

    edit_opt(
        ui,
        &mut node.color_scheme,
        default_scheme,
        |ui| {
            docs_label(
                ui,
                "Color Scheme",
                docs,
                registry,
                DocsRef::Custom("Custom color scheme of the node".into()),
            );
        },
        |ui, scheme| {
            if ui.checkbox(&mut scheme.dark_mode, "Dark Mode").changed() {
                if scheme.dark_mode {
                    scheme.theme.set_dark(ui);
                } else {
                    scheme.theme.set_light(ui);
                }
            };
            colorix_editor(ui, &mut scheme.theme, false);
        },
    );
}

pub fn edit_node_region(
    ui: &mut Ui,
    registry: &ETypesRegistry,
    docs: &Docs,
    graph: &mut Graph,
    selected_nodes: &[NodeId],
) {
    let Some(node) = exactly_one_node(ui, selected_nodes) else {
        return;
    };

    let node = &graph.snarl()[node];
    let Some(region) = node.region_source().or_else(|| node.region_end()) else {
        ui.label("Selected node must be start or end of a region");
        return;
    };

    let reg = graph
        .regions_mut()
        .get_mut(&region)
        .expect("Region should exist");

    let default_color = reg.color();

    ui.horizontal(|ui| {
        edit_opt(
            ui,
            &mut reg.color,
            default_color,
            |ui| {
                docs_label(ui, "Region Color", docs, registry, DocsRef::Custom("Color of the region as visible in graph editor. If not set, a random color will be used".into()));
            },
            |ui, color| {
                ui.color_edit_button_srgba(color);
                // rgb_edit(ui, color);
            },
        );
    });

    ui.label("Variables");
    edit_io(ui, &mut reg.variables, "variables", |i| RegionVariable {
        ty: None,
        id: Uuid::new_v4(),
        name: format!("value {}", i),
    });
}

fn at_least_one_node(ui: &mut Ui, selected_nodes: &[NodeId]) -> bool {
    if selected_nodes.is_empty() {
        ui.label("No node selected");
        ui.label("SHIFT+Click to select nodes");
        return false;
    }
    true
}

fn exactly_one_node(ui: &mut Ui, selected_nodes: &[NodeId]) -> Option<NodeId> {
    if !at_least_one_node(ui, selected_nodes) {
        return None;
    } else if selected_nodes.len() > 1 {
        ui.label("A single node must be selected");
        ui.label("CTRL+Click to deselect nodes");
        return None;
    }

    Some(selected_nodes[0])
}

fn edit_opt<T>(
    ui: &mut Ui,
    value: &mut Option<T>,
    mut default_value: T,
    label: impl FnOnce(&mut Ui),
    edit: impl FnOnce(&mut Ui, &mut T),
) {
    let mut checked = value.is_some();
    ui.horizontal(|ui| {
        label(ui);
        ui.checkbox(&mut checked, "")
    });

    if checked {
        let value = value.get_or_insert(default_value);
        edit(ui, value);
    } else {
        *value = None;
        ui.add_enabled_ui(false, |ui| {
            edit(ui, &mut default_value);
        });
    }
}
