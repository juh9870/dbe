use crate::workspace::graph::viewer::NodeView;
use crate::workspace::graph::GraphViewer;
use dbe_backend::graph::node::groups::subgraph::{SubgraphNode, SubgraphNodeFactory};
use dbe_backend::graph::node::{NodeFactory, SnarlNode};
use dbe_backend::project::project_graph::ProjectGraphs;
use egui::Ui;
use egui_hooks::UseHookExt;
use egui_snarl::{InPin, NodeId, OutPin, Snarl};
use std::ops::DerefMut;
use ustr::Ustr;
use uuid::Uuid;

#[derive(Debug)]
pub struct SubgraphNodeViewer;

impl NodeView for SubgraphNodeViewer {
    fn id(&self) -> Ustr {
        SubgraphNodeFactory.id()
    }

    fn has_body(&self, _viewer: &mut GraphViewer, _node: &SnarlNode) -> miette::Result<bool> {
        Ok(true)
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
        let node = snarl[node_id]
            .downcast_mut::<SubgraphNode>()
            .expect("SubgraphViewer should only be used with SubgraphNode");

        let Some(graphs) = viewer.ctx.graphs else {
            return Ok(());
        };

        graphs_combobox(ui, &mut node.graph_id, graphs);

        Ok(())
    }
}

fn graphs_combobox(ui: &mut Ui, selected: &mut Uuid, graphs: &ProjectGraphs) {
    ui.push_id("graphs_combobox", |ui| {
        egui::ComboBox::from_id_salt("dropdown")
            .selected_text(
                graphs
                    .graphs
                    .get(selected)
                    .map(|g| g.display_name())
                    .unwrap_or_else(|| "!!deleted graph!!".to_string()),
            )
            .show_ui(ui, |ui| {
                let mut search_query = ui.use_state(|| "".to_string(), *selected).into_var();
                let search_bar = ui.text_edit_singleline(search_query.deref_mut());
                search_bar.request_focus();

                let query = search_query.trim();
                let query = if query.is_empty() { None } else { Some(query) };

                for (id, graph) in graphs.graphs.iter().filter(|(_, g)| {
                    let Some(q) = query else { return true };
                    g.display_name().contains(q)
                }) {
                    ui.selectable_value(selected, *id, graph.display_name());
                }
            })
    });
}