use crate::workspace::graph::viewer::NodeView;
use crate::workspace::graph::{pin_info, GraphViewer};
use dbe_backend::graph::node::reroute::RerouteFactory;
use dbe_backend::graph::node::{NodeFactory, SnarlNode};
use egui::{vec2, InnerResponse, Sense, Ui};
use egui_snarl::ui::{NodeLayout, PinInfo};
use egui_snarl::{InPin, NodeId, OutPin, Snarl};
use ustr::Ustr;

#[derive(Debug)]
pub struct RerouteNodeViewer;

impl NodeView for RerouteNodeViewer {
    fn id(&self) -> Ustr {
        RerouteFactory.id()
    }

    fn show_header(
        &self,
        _viewer: &GraphViewer,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        _scale: f32,
        _snarl: &mut Snarl<SnarlNode>,
    ) -> miette::Result<()> {
        ui.label(node.0.to_string());
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
        let registry = viewer.ctx.registry;
        let node = &snarl[pin.id.node];
        let input_data = node.try_input(viewer.ctx.as_node_context(), pin.id.input)?;
        let res = ui.allocate_exact_size(vec2(0.0, 0.0), Sense::click()).1;
        Ok(InnerResponse::new(pin_info(&input_data.ty, registry), res))
    }

    fn show_output(
        &self,
        viewer: &mut GraphViewer,
        pin: &OutPin,
        ui: &mut Ui,
        _scale: f32,
        snarl: &mut Snarl<SnarlNode>,
    ) -> miette::Result<InnerResponse<PinInfo>> {
        let registry = viewer.ctx.registry;
        let node = &snarl[pin.id.node];
        let output_data = node.try_output(viewer.ctx.as_node_context(), pin.id.output)?;
        // let value = viewer.ctx.read_output(snarl, pin.id)?;
        // if value != EValue::Null {
        //     ui.label(value.to_string());
        // }

        let res = ui.allocate_exact_size(vec2(0.0, 0.0), Sense::click()).1;
        Ok(InnerResponse::new(pin_info(&output_data.ty, registry), res))
    }

    fn node_layout(&self, _viewer: &mut GraphViewer, _node: &SnarlNode) -> NodeLayout {
        NodeLayout::Basic
    }
}
