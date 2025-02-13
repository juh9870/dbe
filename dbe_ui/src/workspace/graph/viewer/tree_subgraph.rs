use crate::workspace::graph::viewer::default_view::DefaultNodeView;
use crate::workspace::graph::viewer::NodeView;
use crate::workspace::graph::{any_pin, GraphViewer};
use dbe_backend::graph::node::groups::tree_subgraph::{
    TreeContext, TreeSubgraph, TreeSubgraphFactory,
};
use dbe_backend::graph::node::{Node, NodeFactory, SnarlNode};
use egui::{vec2, InnerResponse, Rect, RichText, Stroke, Ui};
use egui_snarl::ui::PinInfo;
use egui_snarl::{InPin, Snarl};
use inline_tweak::tweak;
use itertools::Itertools;
use ustr::Ustr;

#[derive(Debug)]
pub struct TreeSubgraphNodeViewer;

impl NodeView for TreeSubgraphNodeViewer {
    fn id(&self) -> Ustr {
        TreeSubgraphFactory.id()
    }

    fn show_input(
        &self,
        viewer: &mut GraphViewer,
        pin: &InPin,
        ui: &mut Ui,
        scale: f32,
        snarl: &mut Snarl<SnarlNode>,
    ) -> miette::Result<InnerResponse<PinInfo>> {
        let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
        let color = stroke.color;
        let rect_size = tweak!(4.0) * scale;
        let shrink = (rect_size - stroke.width) / 2.0;
        let margin = ui.style().spacing.item_spacing.y;

        let tree_node = snarl[pin.id.node].downcast_mut::<TreeSubgraph>().unwrap();
        let state = tree_node.tree_cache(viewer.ctx.as_node_context());
        if pin.id.input >= state.hierarchy.len() {
            let res =
                ui.label(RichText::new("Invalid input").color(ui.style().visuals.error_fg_color));
            return Ok(InnerResponse::new(any_pin(), res));
        }
        let mut start_of = state.start_of[pin.id.input].clone();
        let mut end_of = state.end_of[pin.id.input].clone();
        let mut hierarchy = state.hierarchy[pin.id.input].clone();
        let root = hierarchy.pop().unwrap();
        if start_of.ends_with(&[root]) {
            start_of.pop();
        }
        if end_of.ends_with(&[root]) {
            end_of.pop();
        }

        assert!(start_of.len() <= hierarchy.len());
        assert!(end_of.len() <= hierarchy.len());
        debug_assert_eq!(&start_of, &hierarchy[..start_of.len()]);
        debug_assert_eq!(&end_of, &hierarchy[..end_of.len()]);

        // let _guard = trace_span!("tree_node_show_input", input=pin.id.input, ?hierarchy_width, hierarchy_len=?hierarchy.len()).entered();
        let titles = start_of
            .iter()
            .map(|x| {
                tree_node.node_title(
                    *x,
                    TreeContext {
                        registry: viewer.ctx.registry,
                        docs: viewer.ctx.docs,
                        graphs: viewer.ctx.graphs,
                    },
                )
            })
            .collect_vec();

        let full_width = ui.available_width();

        let res = ui
            .vertical(|ui| -> miette::Result<_> {
                let input_start_pos = ui.cursor().min;
                let mut hline_start_pos = vec![];
                for (idx, (_, title)) in start_of.iter().zip(titles).rev().enumerate() {
                    let pos = ui.cursor().min;
                    hline_start_pos.push(pos);
                    let offset = hierarchy.len() + idx - start_of.len();
                    // trace!(?offset, ?node, ?idx, "start of input");
                    let width = full_width - rect_size * offset as f32;
                    ui.painter().rect(
                        Rect::from_min_size(pos, vec2(width, rect_size)).shrink(shrink),
                        0.0,
                        color,
                        Stroke::NONE,
                    );
                    ui.add_space(rect_size);
                    ui.add_space(margin);
                    ui.label(title);
                }
                if !start_of.is_empty() {
                    ui.add_space(margin);
                }
                hline_start_pos.reverse();

                let strokes_pos = ui.cursor().min;

                let mut res = DefaultNodeView.show_input(viewer, pin, ui, scale, snarl)?;
                let res_height = res.response.rect.height();

                res.inner.position = Some(res.response.rect.left_center().y);

                for (idx, _) in hierarchy.iter().enumerate() {
                    let offset = hierarchy.len() - idx;
                    // trace!(?offset, node=?id, ?idx, "right side stroke");
                    let mut pos =
                        strokes_pos + vec2(full_width - rect_size * offset as f32, rect_size / 2.0);
                    let mut size = vec2(rect_size, res_height);
                    if start_of.is_empty() || idx >= start_of.iter().len() {
                        let expand = pos.y - input_start_pos.y - rect_size / 2.0;
                        pos.y -= expand;
                        size.y += expand;
                    } else {
                        let expand = pos.y - hline_start_pos[idx].y - rect_size / 2.0;
                        pos.y -= expand;
                        size.y += expand;
                    }
                    if !end_of.is_empty() && idx >= end_of.len() {
                        let expand = margin * 2.0 + rect_size * end_of.len() as f32;
                        size.y += expand;
                    } else if end_of.is_empty() {
                        size.y += margin;
                    }
                    ui.painter().rect(
                        Rect::from_min_size(pos, size).shrink2(vec2(shrink, 0.0)),
                        0.0,
                        color,
                        Stroke::NONE,
                    );
                }

                for (idx, _) in end_of.iter().rev().enumerate().rev() {
                    let pos = ui.cursor().min;
                    let offset = hierarchy.len() + idx - end_of.len();
                    // trace!(?offset, ?node, ?idx, "start of input");
                    let width = full_width - rect_size * offset as f32;
                    ui.painter().rect(
                        Rect::from_min_size(pos, vec2(width, rect_size)).shrink(shrink),
                        0.0,
                        color,
                        Stroke::NONE,
                    );
                    let height = rect_size * (end_of.len() - idx) as f32;
                    ui.painter().rect(
                        Rect::from_min_size(
                            pos + vec2(width - rect_size, -height + rect_size / 2.0),
                            vec2(rect_size, height),
                        )
                        .shrink2(vec2(shrink, 0.0)),
                        0.0,
                        color,
                        Stroke::NONE,
                    );
                    ui.add_space(rect_size);
                }

                Ok(res)
            })
            .inner?;

        ui.allocate_ui(
            vec2(
                rect_size * hierarchy.len() as f32,
                res.response.rect.height(),
            ),
            |_| {},
        );
        Ok(res)
    }
}
