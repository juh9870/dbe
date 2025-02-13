use crate::workspace::graph::viewer::default_view::DefaultNodeView;
use crate::workspace::graph::viewer::destructuring::DestructuringNodeViewer;
use crate::workspace::graph::viewer::reroute::RerouteNodeViewer;
use crate::workspace::graph::viewer::subgraph::SubgraphNodeViewer;
use crate::workspace::graph::viewer::tree_subgraph::TreeSubgraphNodeViewer;
use crate::workspace::graph::GraphViewer;
use atomic_refcell::AtomicRefCell;
use dbe_backend::graph::node::SnarlNode;
use egui::{InnerResponse, Ui};
use egui_snarl::ui::{NodeLayout, PinInfo};
use egui_snarl::{InPin, NodeId, OutPin, Snarl};
use std::fmt::Debug;
use std::ops::Deref;
use std::sync::{Arc, LazyLock};
use ustr::{Ustr, UstrMap};

pub mod default_view;
pub mod destructuring;
pub mod reroute;
pub mod subgraph;
pub mod tree_subgraph;

static NODE_VIEWERS: LazyLock<AtomicRefCell<UstrMap<Arc<dyn NodeView>>>> =
    LazyLock::new(|| AtomicRefCell::new(default_viewers().collect()));
static DEFAULT_VIEWER: LazyLock<Arc<dyn NodeView>> = LazyLock::new(|| Arc::new(DefaultNodeView));

fn default_viewers() -> impl Iterator<Item = (Ustr, Arc<dyn NodeView>)> {
    let v: Vec<Arc<dyn NodeView>> = vec![
        Arc::new(RerouteNodeViewer),
        Arc::new(SubgraphNodeViewer),
        Arc::new(DestructuringNodeViewer),
        Arc::new(TreeSubgraphNodeViewer),
    ];
    v.into_iter().map(|item| (Ustr::from(&item.id()), item))
}

pub fn get_viewer(id: &Ustr) -> Arc<dyn NodeView> {
    NODE_VIEWERS
        .borrow()
        .get(id)
        .cloned()
        .unwrap_or_else(|| Arc::clone(DEFAULT_VIEWER.deref()))
}

pub trait NodeView: Send + Sync + Debug + 'static {
    fn id(&self) -> Ustr;

    #[allow(clippy::too_many_arguments)]
    fn show_header(
        &self,
        viewer: &GraphViewer,
        node: NodeId,
        inputs: &[InPin],
        outputs: &[OutPin],
        ui: &mut Ui,
        scale: f32,
        snarl: &mut Snarl<SnarlNode>,
    ) -> miette::Result<()> {
        DefaultNodeView.show_header(viewer, node, inputs, outputs, ui, scale, snarl)
    }

    fn show_input(
        &self,
        viewer: &mut GraphViewer,
        pin: &InPin,
        ui: &mut Ui,
        scale: f32,
        snarl: &mut Snarl<SnarlNode>,
    ) -> miette::Result<InnerResponse<PinInfo>> {
        DefaultNodeView.show_input(viewer, pin, ui, scale, snarl)
    }

    fn show_output(
        &self,
        viewer: &mut GraphViewer,
        pin: &OutPin,
        ui: &mut Ui,
        scale: f32,
        snarl: &mut Snarl<SnarlNode>,
    ) -> miette::Result<InnerResponse<PinInfo>> {
        DefaultNodeView.show_output(viewer, pin, ui, scale, snarl)
    }

    fn node_layout(&self, viewer: &mut GraphViewer, node: &SnarlNode) -> NodeLayout {
        DefaultNodeView.node_layout(viewer, node)
    }

    fn has_body(&self, viewer: &mut GraphViewer, node: &SnarlNode) -> miette::Result<bool> {
        DefaultNodeView.has_body(viewer, node)
    }

    #[allow(clippy::too_many_arguments)]
    fn show_body(
        &self,
        viewer: &mut GraphViewer,
        node: NodeId,
        inputs: &[InPin],
        outputs: &[OutPin],
        ui: &mut Ui,
        scale: f32,
        snarl: &mut Snarl<SnarlNode>,
    ) -> miette::Result<()> {
        DefaultNodeView.show_body(viewer, node, inputs, outputs, ui, scale, snarl)
    }
}
