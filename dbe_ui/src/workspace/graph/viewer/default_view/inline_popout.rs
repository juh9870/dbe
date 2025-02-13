use crate::error::report_error;
use crate::workspace::graph::search::{search_ui_always, GraphSearch};
use crate::workspace::graph::GraphViewer;
use dbe_backend::graph::node::groups::tree_subgraph::TreeSubgraph;
use dbe_backend::graph::node::{Node, SnarlNode};
use egui::Ui;
use egui_hooks::UseHookExt;
use egui_snarl::{InPin, Snarl};

pub fn show_input_popout_menu(
    viewer: &mut GraphViewer,
    snarl: &mut Snarl<SnarlNode>,
    pin: &InPin,
    ui: &mut Ui,
) {
    let node = &snarl[pin.id.node];
    let data = match node.try_input(viewer.ctx.as_node_context(), pin.id.input) {
        Ok(data) => data,
        Err(err) => {
            report_error(err);
            ui.close_menu();
            return;
        }
    };
    let graphs = viewer.ctx.graphs;
    let registry = viewer.ctx.registry;
    let search = ui.use_memo(
        move || GraphSearch::for_input_data(graphs, registry, &data),
        (),
    );

    if let Some(created_node) = search_ui_always(ui, "dropped_wire_in_search_menu", search) {
        ui.close_menu();
        let node = &mut snarl[pin.id.node];
        let tree_node = if let Some(node) = node.downcast_mut::<TreeSubgraph>() {
            node
        } else {
            let tree = TreeSubgraph::new(node.clone());

            node.node = Box::new(tree);

            if let Err(err) = node.update_state(
                viewer.ctx.as_node_context(),
                &mut viewer.commands,
                pin.id.node,
            ) {
                report_error(err);
                return;
            }

            node.downcast_mut::<TreeSubgraph>().unwrap()
        };

        if let Err(err) =
            tree_node.create_input(viewer.ctx.as_node_context(), pin.id.input, created_node)
        {
            report_error(err);
        }
    }
}
