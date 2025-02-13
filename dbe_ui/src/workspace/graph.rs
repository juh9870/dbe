use crate::error::report_error;
use crate::m_try;
use crate::ui_props::PROP_OBJECT_PIN_COLOR;
use crate::widgets::report::diagnostic_widget;
use crate::workspace::graph::rects::NodeRects;
use crate::workspace::graph::search::GraphSearch;
use crate::workspace::graph::search::{category_tree, search_ui, search_ui_always};
use crate::workspace::graph::viewer::get_viewer;
use dbe_backend::diagnostic::context::DiagnosticContextRef;
use dbe_backend::diagnostic::prelude::{Diagnostic, DiagnosticLevel};
use dbe_backend::etype::econst::ETypeConst;
use dbe_backend::etype::eobject::EObject;
use dbe_backend::etype::EDataType;
use dbe_backend::graph::editing::PartialGraphEditingContext;
use dbe_backend::graph::node::commands::SnarlCommands;
use dbe_backend::graph::node::creation::NodeCombo;
use dbe_backend::graph::node::ports::NodePortType;
use dbe_backend::graph::node::SnarlNode;
use dbe_backend::registry::ETypesRegistry;
use egui::epaint::PathShape;
use egui::{Color32, Frame, Painter, Pos2, Rect, ScrollArea, Stroke, Style, Ui};
use egui_hooks::UseHookExt;
use egui_snarl::ui::{
    AnyPins, BackgroundPattern, NodeLayout, PinInfo, SnarlStyle, SnarlViewer, Viewport,
};
use egui_snarl::{InPin, NodeId, OutPin, Snarl};
use inline_tweak::tweak;
use itertools::{Itertools, PeekingNext};
use random_color::options::Luminosity;
use random_color::RandomColor;
use std::iter::Peekable;

pub mod rects;
pub mod search;
pub mod toolbar;
pub mod viewer;

#[derive(Debug)]
pub struct GraphViewer<'a> {
    pub ctx: PartialGraphEditingContext<'a>,
    pub diagnostics: DiagnosticContextRef<'a>,
    pub node_rects: &'a mut NodeRects,
    commands: SnarlCommands,
}

impl<'a> GraphViewer<'a> {
    pub fn new(
        ctx: PartialGraphEditingContext<'a>,
        diagnostics: DiagnosticContextRef<'a>,
        node_rects: &'a mut NodeRects,
    ) -> Self {
        Self {
            ctx,
            diagnostics,
            commands: Default::default(),
            node_rects,
        }
    }
}

impl SnarlViewer<SnarlNode> for GraphViewer<'_> {
    fn title(&mut self, _node: &SnarlNode) -> String {
        unreachable!("Custom header doesn't call SnarlViewer::title")
    }

    fn node_frame(
        &mut self,
        default: Frame,
        node: NodeId,
        inputs: &[InPin],
        outputs: &[OutPin],
        snarl: &Snarl<SnarlNode>,
    ) -> Frame {
        self.header_frame(default, node, inputs, outputs, snarl)
    }

    fn header_frame(
        &mut self,
        mut default: Frame,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        snarl: &Snarl<SnarlNode>,
    ) -> Frame {
        if let Ok(data) = self.ctx.region_graph.try_as_data() {
            if let Some(node_region) = data.node_region(&node) {
                if let Some(reg) = self.ctx.regions.get(&node_region) {
                    let color = reg.color();
                    default = default.stroke(Stroke::new(
                        default.stroke.width,
                        color.gamma_multiply(tweak!(1.0)),
                    ));
                }
            }
        }

        if let Some(scheme) = &snarl[node].color_scheme {
            default = default.fill(scheme.theme.tokens.subtle_background())
        }

        default
    }

    fn has_node_style(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        snarl: &Snarl<SnarlNode>,
    ) -> bool {
        let node = &snarl[node];
        node.color_scheme.is_some()
    }

    fn apply_node_style(
        &mut self,
        style: &mut Style,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        snarl: &Snarl<SnarlNode>,
    ) {
        let Some(scheme) = &snarl[node].color_scheme else {
            unreachable!()
        };

        scheme.theme.tokens.set_egui_style(style)
    }

    fn node_layout(
        &mut self,
        _default: NodeLayout,
        node_id: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        snarl: &Snarl<SnarlNode>,
    ) -> NodeLayout {
        let node = &snarl[node_id];

        let viewer = get_viewer(&node.id());
        viewer.node_layout(self, node)
    }

    fn show_header(
        &mut self,
        node_id: NodeId,
        inputs: &[InPin],
        outputs: &[OutPin],
        ui: &mut Ui,
        scale: f32,
        snarl: &mut Snarl<SnarlNode>,
    ) {
        m_try(|| {
            let node = &snarl[node_id];

            let viewer = get_viewer(&node.id());

            let has_body = viewer.has_body(self, node)?;

            let node = &mut snarl[node_id];

            if !has_body {
                node.update_state(self.ctx.as_node_context(), &mut self.commands, node_id)?;
                self.commands.execute(&mut self.ctx.as_full(snarl))?;
            }

            viewer.show_header(self, node_id, inputs, outputs, ui, scale, snarl)?;

            Ok(())
        })
        .unwrap_or_else(|err| {
            ui.set_max_width(128.0);
            diagnostic_widget(
                ui,
                &Diagnostic {
                    info: err,
                    level: DiagnosticLevel::Error,
                },
            );
        })
    }

    fn inputs(&mut self, node: &SnarlNode) -> usize {
        node.inputs_count(self.ctx.as_node_context())
    }

    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut Ui,
        _scale: f32,
        snarl: &mut Snarl<SnarlNode>,
    ) -> PinInfo {
        m_try(|| get_viewer(&snarl[pin.id.node].id()).show_input(self, pin, ui, _scale, snarl))
            .map(|r| r.inner)
            .unwrap_or_else(|err| {
                // ui.set_max_width(128.0);
                diagnostic_widget(
                    ui,
                    &Diagnostic {
                        info: err,
                        level: DiagnosticLevel::Error,
                    },
                );
                PinInfo::circle().with_fill(Color32::BLACK)
            })
    }

    fn outputs(&mut self, node: &SnarlNode) -> usize {
        node.outputs_count(self.ctx.as_node_context())
    }

    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut Ui,
        _scale: f32,
        snarl: &mut Snarl<SnarlNode>,
    ) -> PinInfo {
        m_try(|| get_viewer(&snarl[pin.id.node].id()).show_output(self, pin, ui, _scale, snarl))
            .map(|r| r.inner)
            .unwrap_or_else(|err| {
                ui.set_max_width(128.0);
                diagnostic_widget(
                    ui,
                    &Diagnostic {
                        info: err,
                        level: DiagnosticLevel::Error,
                    },
                );
                PinInfo::circle().with_fill(Color32::BLACK)
            })
    }

    fn has_body(&mut self, node: &SnarlNode) -> bool {
        m_try(|| get_viewer(&node.id()).has_body(self, node)).unwrap_or_else(|err| {
            report_error(err);
            false
        })
    }

    fn show_body(
        &mut self,
        node_id: NodeId,
        inputs: &[InPin],
        outputs: &[OutPin],
        ui: &mut Ui,
        scale: f32,
        snarl: &mut Snarl<SnarlNode>,
    ) {
        ui.vertical(|ui| {
            m_try(|| {
                get_viewer(&snarl[node_id].id())
                    .show_body(self, node_id, inputs, outputs, ui, scale, snarl)?;

                snarl[node_id].update_state(
                    self.ctx.as_node_context(),
                    &mut self.commands,
                    node_id,
                )?;

                self.commands.execute(&mut self.ctx.as_full(snarl))?;
                Ok(())
            })
            .unwrap_or_else(|err| {
                diagnostic_widget(
                    ui,
                    &Diagnostic {
                        info: err,
                        level: DiagnosticLevel::Error,
                    },
                );
            })
        });
    }

    fn final_node_rect(
        &mut self,
        node: NodeId,
        _ui_rect: Rect,
        graph_rect: Rect,
        _ui: &mut Ui,
        _scale: f32,
        _snarl: &mut Snarl<SnarlNode>,
    ) {
        self.node_rects.add_graph_space_rect(node, graph_rect);
    }

    fn has_graph_menu(&mut self, _pos: Pos2, _snarl: &mut Snarl<SnarlNode>) -> bool {
        true
    }

    fn show_graph_menu(
        &mut self,
        pos: Pos2,
        ui: &mut Ui,
        _scale: f32,
        snarl: &mut Snarl<SnarlNode>,
    ) {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        ui.menu_button("Add node", |ui| {
            let search = ui.use_memo(
                || GraphSearch::all_nodes(self.ctx.graphs, self.ctx.registry, |_| true),
                (),
            );

            let node = search_ui(ui, "add_node_searchbar", search, |ui| {
                let categories =
                    ui.use_memo(|| category_tree(self.ctx.graphs, self.ctx.registry), ());
                let mut categories = categories
                    .iter()
                    .map(|x| (x.0.split('.').collect_vec(), x.1))
                    .peekable();

                fn show<'a, C: AsRef<[&'a str]>>(
                    ui: &mut Ui,
                    parent: &[&str],
                    categories: &mut Peekable<impl Iterator<Item = (C, &'a Vec<NodeCombo>)>>,
                ) -> Option<NodeCombo> {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    while let Some((cat, _)) = categories.peek() {
                        let cat = cat.as_ref();
                        if !parent.is_empty() && !cat.starts_with(parent) {
                            return None;
                        }
                        let cat_name = cat.strip_prefix(parent).unwrap();

                        if cat_name.len() > 1 {
                            let next_cat = parent
                                .iter()
                                .chain(cat_name.iter().take(1))
                                .copied()
                                .collect_vec();
                            return ui
                                .menu_button(cat_name[0], |ui| {
                                    show(ui, next_cat.as_slice(), categories)
                                })
                                .inner
                                .flatten();
                        }

                        if !parent.is_empty() {
                            ui.separator();
                        }

                        let (category, factories) = categories.next().unwrap();
                        let category = category.as_ref();
                        let cat_name = category.strip_prefix(parent).unwrap();
                        if let Some(node) = ui
                            .menu_button(cat_name.join("."), |ui| {
                                ScrollArea::vertical()
                                    .max_height(ui.ctx().screen_rect().height() / 2.0)
                                    .show(ui, |ui| {
                                        for node in factories.iter() {
                                            if ui.button(node.display_title()).clicked() {
                                                ui.close_menu();
                                                return Some(node.clone());
                                            }
                                        }

                                        show(ui, category, categories)
                                    })
                                    .inner
                            })
                            .inner
                            .flatten()
                        {
                            return Some(node);
                        }

                        while categories
                            .peeking_next(|(next_cat, _)| next_cat.as_ref().starts_with(category))
                            .is_some()
                        {}
                    }

                    None
                }

                show(ui, &[], &mut categories)
            });

            if let Some(to_insert) = node {
                ui.close_menu();
                if let Err(err) = to_insert.create(&mut self.ctx.as_full(snarl), pos) {
                    report_error(err)
                }
            }
        });
    }

    fn has_dropped_wire_menu(&mut self, _src_pins: AnyPins, _snarl: &mut Snarl<SnarlNode>) -> bool {
        true
    }

    fn show_dropped_wire_menu(
        &mut self,
        pos: Pos2,
        ui: &mut Ui,
        _scale: f32,
        src_pins: AnyPins,
        snarl: &mut Snarl<SnarlNode>,
    ) {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        match src_pins {
            AnyPins::Out(pins) => {
                for pin in pins {
                    let node = &snarl[pin.node];
                    let data = match node.try_output(self.ctx.as_node_context(), pin.output) {
                        Ok(data) => data,
                        Err(err) => {
                            report_error(err);
                            ui.close_menu();
                            return;
                        }
                    };
                    let graphs = self.ctx.graphs;
                    let registry = self.ctx.registry;
                    let search = ui.use_memo(
                        move || GraphSearch::for_output_data(graphs, registry, &data),
                        (),
                    );

                    if let Some(node) = search_ui_always(ui, "dropped_wire_out_search_menu", search)
                    {
                        ui.close_menu();
                        if let Err(err) = node.create_from_output_pin(
                            &mut self.ctx.as_full(snarl),
                            pos,
                            pin,
                            &mut self.commands,
                        ) {
                            report_error(err)
                        }
                    }
                }
            }
            AnyPins::In(pins) => {
                for pin in pins {
                    let node = &snarl[pin.node];
                    let data = match node.try_input(self.ctx.as_node_context(), pin.input) {
                        Ok(data) => data,
                        Err(err) => {
                            report_error(err);
                            ui.close_menu();
                            return;
                        }
                    };
                    let graphs = self.ctx.graphs;
                    let registry = self.ctx.registry;
                    let search = ui.use_memo(
                        move || GraphSearch::for_input_data(graphs, registry, &data),
                        (),
                    );

                    if let Some(node) = search_ui_always(ui, "dropped_wire_in_search_menu", search)
                    {
                        ui.close_menu();
                        if let Err(err) = node.create_from_input_pin(
                            &mut self.ctx.as_full(snarl),
                            pos,
                            pin,
                            &mut self.commands,
                        ) {
                            report_error(err)
                        }
                    }
                }
            }
        }
    }

    fn has_node_menu(&mut self, _node: &SnarlNode) -> bool {
        true
    }

    fn show_node_menu(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        _scale: f32,
        snarl: &mut Snarl<SnarlNode>,
    ) {
        m_try(|| {
            if ui.button("Duplicate").clicked() {
                self.ctx.as_full(snarl).duplicate_node(node)?;
                ui.close_menu();
            }
            if ui.button("Remove").clicked() {
                self.ctx
                    .as_full(snarl)
                    .remove_node(node, &mut self.commands)?;
                ui.close_menu();
            }
            Ok(())
        })
        .unwrap_or_else(report_error)
    }

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<SnarlNode>) {
        if let Err(err) = self
            .ctx
            .as_full(snarl)
            .connect(from, to, &mut self.commands)
        {
            report_error(err)
        }
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<SnarlNode>) {
        if let Err(err) = self
            .ctx
            .as_full(snarl)
            .disconnect(from, to, &mut self.commands)
        {
            report_error(err);
        }
    }

    fn drop_outputs(&mut self, pin: &OutPin, snarl: &mut Snarl<SnarlNode>) {
        if let Err(err) = m_try(|| {
            for remote in &pin.remotes {
                let in_pin = snarl.in_pin(*remote);
                self.ctx
                    .as_full(snarl)
                    .disconnect(pin, &in_pin, &mut self.commands)?;
            }

            Ok(())
        }) {
            report_error(err);
        }
    }

    fn drop_inputs(&mut self, pin: &InPin, snarl: &mut Snarl<SnarlNode>) {
        if let Err(err) = m_try(|| {
            for remote in &pin.remotes {
                let out_pin = snarl.out_pin(*remote);
                self.ctx
                    .as_full(snarl)
                    .disconnect(&out_pin, pin, &mut self.commands)?;
            }

            Ok(())
        }) {
            report_error(err);
        }
    }

    fn draw_background(
        &mut self,
        _background: Option<&BackgroundPattern>,
        viewport: &Viewport,
        snarl_style: &SnarlStyle,
        style: &Style,
        painter: &Painter,
        snarl: &Snarl<SnarlNode>,
    ) {
        BackgroundPattern::Grid(Default::default()).draw(viewport, snarl_style, style, painter);

        self.ctx.region_graph.ensure_ready(snarl);
        let Ok(data) = self.ctx.region_graph.try_as_data() else {
            return;
        };

        let scale = viewport.scale;

        // TODO: cache region hull calculations

        for region in data.ordered_regions() {
            let Some(region_info) = self.ctx.regions.get(region) else {
                continue;
            };

            let Some(points) = self.node_rects.screen_space_hull(region, data, viewport) else {
                continue;
            };

            let shape = PathShape::convex_polygon(
                points,
                region_info.color().gamma_multiply(tweak!(0.2)),
                Stroke::new(
                    tweak!(2.0) * scale,
                    region_info.color().gamma_multiply(tweak!(0.5)),
                ),
            );

            painter.add(shape);
        }
    }
}

fn pin_color(ty: EDataType, registry: &ETypesRegistry) -> Color32 {
    const NUMBER_COLOR: Color32 = Color32::from_rgb(161, 161, 161);
    const BOOLEAN_COLOR: Color32 = Color32::from_rgb(204, 166, 214);
    const STRING_COLOR: Color32 = Color32::from_rgb(112, 178, 255);
    const NULL_COLOR: Color32 = Color32::from_rgb(0, 0, 0);
    const UNKNOWN_COLOR: Color32 = Color32::from_rgb(255, 178, 112);
    match ty {
        EDataType::Number => NUMBER_COLOR,
        EDataType::String => STRING_COLOR,
        EDataType::Boolean => BOOLEAN_COLOR,
        EDataType::Const { value } => match value {
            ETypeConst::String(_) => STRING_COLOR,
            ETypeConst::Number(_) => NUMBER_COLOR,
            ETypeConst::Boolean(_) => BOOLEAN_COLOR,
            ETypeConst::Null => NULL_COLOR,
        },
        EDataType::Object { ident } => match registry.get_object(&ident) {
            None => NULL_COLOR,
            Some(data) => PROP_OBJECT_PIN_COLOR
                .try_get(data.extra_properties())
                .map(|e| e.0)
                .unwrap_or_else(|| {
                    let c = RandomColor::new()
                        .seed(ident.to_string())
                        .luminosity(Luminosity::Light)
                        .alpha(1.0)
                        .to_rgb_array();
                    Color32::from_rgb(c[0], c[1], c[2])
                }),
        },
        EDataType::List { id } => registry
            .get_list(&id)
            .map(|e| pin_color(e.value_type, registry))
            .unwrap_or(NULL_COLOR),
        EDataType::Map { id } => registry
            .get_map(&id)
            .map(|e| pin_color(e.value_type, registry))
            .unwrap_or(NULL_COLOR),
        EDataType::Unknown => UNKNOWN_COLOR,
    }
}

fn pin_stroke(ty: EDataType, registry: &ETypesRegistry) -> Stroke {
    if let EDataType::Map { id } = ty {
        let color = registry
            .get_map(&id)
            .map(|e| pin_color(e.key_type, registry))
            .unwrap_or_else(|| pin_color(ty, registry));
        Stroke::new(tweak!(2.0), color)
    } else {
        Stroke::NONE
    }
}

fn pin_info(ty: &NodePortType, registry: &ETypesRegistry) -> PinInfo {
    match ty {
        NodePortType::BasedOnSource | NodePortType::BasedOnTarget => any_pin(),
        NodePortType::Specific(ty) => {
            let shape = match ty.ty() {
                EDataType::Boolean
                | EDataType::Number
                | EDataType::String
                | EDataType::Const { .. } => PinInfo::circle(),
                EDataType::Object { .. } => PinInfo::circle(),
                EDataType::List { .. } => PinInfo::square(),
                EDataType::Map { .. } => PinInfo::square(),
                EDataType::Unknown => PinInfo::star(),
            };

            shape
                .with_fill(pin_color(ty.ty(), registry))
                .with_stroke(pin_stroke(ty.ty(), registry))
        }
        NodePortType::Invalid => PinInfo::circle().with_fill(Color32::BLACK),
    }
}

fn any_pin() -> PinInfo {
    PinInfo::circle()
        .with_fill(Color32::from_rgb(tweak!(128), tweak!(128), tweak!(128)))
        .with_stroke(Stroke::new(
            tweak!(2.0),
            Color32::from_rgb(tweak!(44), tweak!(44), tweak!(44)),
        ))
}
