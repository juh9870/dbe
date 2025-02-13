use crate::graph::editing::GraphEditingContext;
use crate::graph::inputs::{GraphInput, GraphOutput};
use crate::graph::node::colors::PackedNodeColorScheme;
use crate::graph::node::commands::SnarlCommands;
use crate::graph::node::{get_node_factory, NodeContext, SnarlNode};
use crate::graph::region::region_graph::RegionGraph;
use crate::graph::region::RegionInfo;
use crate::json_utils::JsonValue;
use crate::m_try;
use crate::project::docs::Docs;
use crate::project::side_effects::SideEffectsContext;
use crate::registry::ETypesRegistry;
use crate::value::EValue;
use egui_snarl::{InPinId, NodeId, OutPinId, Snarl};
use emath::Pos2;
use itertools::Itertools;
use miette::{miette, Context, IntoDiagnostic};
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use ustr::Ustr;
use utils::map::{HashMap, OrderMap};
use uuid::Uuid;

pub mod cache;
pub mod editing;
pub mod execution;
pub mod inputs;
pub mod node;
pub mod region;

/// A container of a graph with inline values. It contains all the data
/// that is unique to this graph and is required for both node groups and standalone graphs
#[derive(Debug, Default)]
pub struct Graph {
    snarl: Snarl<SnarlNode>,
    inline_values: OrderMap<InPinId, EValue>,
    inputs: SmallVec<[GraphInput; 1]>,
    outputs: SmallVec<[GraphOutput; 1]>,
    regions: OrderMap<Uuid, RegionInfo>,
    region_graph: RegionGraph,
}

impl Clone for Graph {
    fn clone(&self) -> Self {
        Self {
            snarl: self.snarl.clone(),
            inline_values: self.inline_values.clone(),
            inputs: self.inputs.clone(),
            outputs: self.outputs.clone(),
            regions: self.regions.clone(),
            region_graph: RegionGraph::default(),
        }
    }
}

impl Hash for Graph {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.inline_values.hash(state);
        self.inputs.hash(state);
        self.outputs.hash(state);

        for (id, node) in self
            .snarl
            .nodes_ids_data()
            .sorted_unstable_by_key(|(id, _)| *id)
        {
            id.hash(state);
            node.value.hash(state);
            OrderedFloat(node.pos.x).hash(state);
            OrderedFloat(node.pos.y).hash(state);
            node.open.hash(state);
        }

        self.snarl
            .wires()
            .sorted_unstable()
            .for_each(|(out_pin, in_pin)| {
                in_pin.hash(state);
                out_pin.hash(state);
            });
    }
}

impl Graph {
    pub fn parse_json(registry: &ETypesRegistry, value: &mut JsonValue) -> miette::Result<Self> {
        let mut snarl = Snarl::<SnarlNode>::new();
        let packed: PackedGraph = PackedGraph::deserialize(value.take()).into_diagnostic()?;
        let mut mapping = HashMap::with_capacity_and_hasher(
            packed.nodes.len(),
            utils::map::BuildHasher::default(),
        );

        m_try(|| {
            for (serialized_id, mut node) in packed.nodes {
                let created_node = m_try(|| {
                    let mut created_node = get_node_factory(&node.id)
                        .ok_or_else(|| miette!("node type {} not found", node.id))?
                        .create();

                    created_node.parse_json(registry, &mut node.data)?;
                    Ok(created_node)
                })
                .with_context(|| {
                    format!("failed to create node {}({:?})", node.id, serialized_id)
                })?;

                let mut created_node = SnarlNode::new(created_node);

                created_node.color_scheme = node.color_scheme.map(|c| c.unpack());
                created_node.custom_title = node.custom_title;

                let node_id = if node.open {
                    snarl.insert_node(node.pos, created_node)
                } else {
                    snarl.insert_node_collapsed(node.pos, created_node)
                };
                mapping.insert(serialized_id, node_id);
            }
            Ok(())
        })
        .context("failed to create nodes")?;

        let inputs =
            OrderMap::with_capacity_and_hasher(packed.inline_values.len(), Default::default());

        let mut graph = Self {
            snarl,
            inline_values: inputs,
            inputs: packed.inputs,
            outputs: packed.outputs,
            regions: packed.regions.into_iter().map(|r| (r.id(), r)).collect(),
            region_graph: Default::default(),
        };

        m_try(|| {
            let out_values = &mut None;
            let mut ctx = GraphEditingContext::from_graph(
                &mut graph,
                registry,
                &Docs::Stub,
                None,
                SideEffectsContext::unavailable(),
                true,
                &[],
                out_values,
            );
            let commands = &mut SnarlCommands::new();

            ctx.update_all_nodes_state(commands)?;

            commands
                .execute(&mut ctx)
                .with_context(|| "failed to execute commands")?;

            let mut to_connect = Vec::with_capacity(packed.edges.len());

            for (mut out_pin, mut in_pin) in packed.edges {
                out_pin.node = *mapping
                    .get(&out_pin.node)
                    .ok_or_else(|| miette!("node {:?} not found", out_pin.node))?;
                in_pin.node = *mapping
                    .get(&in_pin.node)
                    .ok_or_else(|| miette!("node {:?} not found", in_pin.node))?;
                // snarl.connect(out_pin, in_pin);
                let out_pin = ctx.snarl.out_pin(out_pin);
                let in_pin = ctx.snarl.in_pin(in_pin);
                to_connect.push((out_pin, in_pin));
            }

            to_connect.sort_by_key(|(out_pin, in_pin)| {
                (
                    out_pin.id.node,
                    out_pin.id.output,
                    in_pin.id.node,
                    in_pin.id.input,
                )
            });

            for (out_pin, in_pin) in to_connect {
                ctx.connect(&out_pin, &in_pin, commands)?;
            }

            ctx.update_all_nodes_state(commands)?;

            commands
                .execute(&mut ctx)
                .with_context(|| "failed to execute commands")?;

            drop(ctx);

            Ok(())
        })
        .context("failed to connect pins")?;

        let mut region_graph = RegionGraph::default();
        m_try(|| {
            for (mut in_pin, mut value) in packed.inline_values {
                in_pin.node = *mapping
                    .get(&in_pin.node)
                    .ok_or_else(|| miette!("node {:?} not found", in_pin.node))?;

                let node = graph
                    .snarl
                    .get_node(in_pin.node)
                    .expect("Mappings should be correct");

                let input_type = node
                    .try_input(
                        NodeContext {
                            registry,
                            docs: &Docs::Stub,
                            inputs: &graph.inputs,
                            outputs: &graph.outputs,
                            regions: &graph.regions,
                            region_graph: &region_graph,
                            graphs: None,
                        },
                        in_pin.input,
                    )
                    .with_context(|| format!("failed to get inputs for node {:?}", in_pin.node))?;

                let Some(info) = input_type.ty.item_info() else {
                    continue;
                };

                let value = info
                    .ty()
                    .parse_json(registry, &mut value, false)
                    .with_context(|| {
                        format!(
                            "failed to parse input value #{} for node {:?}",
                            in_pin.input, in_pin.node
                        )
                    })?;
                graph.inline_values.insert(in_pin, value);
            }
            Ok(())
        })
        .context("failed to populate inputs")?;

        region_graph.force_rebuild(&graph.snarl);
        graph.region_graph = region_graph;

        Ok(graph)
    }

    pub fn write_json(&self, registry: &ETypesRegistry) -> miette::Result<JsonValue> {
        let mut packed = PackedGraph {
            nodes: Default::default(),
            edges: Default::default(),
            inline_values: Default::default(),
            inputs: self.inputs.clone(),
            outputs: self.outputs.clone(),
            regions: self.regions.values().cloned().collect_vec(),
        };

        let mut inline_values = HashMap::default();

        for (pin, value) in &self.inline_values {
            let Some(node) = self.snarl.get_node(pin.node) else {
                continue;
            };

            if !node.has_inline_values(pin.input) {
                continue;
            }

            let value_json = value.write_json(registry).with_context(|| {
                format!(
                    "failed to serialize input value #{} for {:?}",
                    pin.input, pin.node
                )
            })?;
            inline_values.insert(*pin, value_json);
        }

        for (id, node) in self.snarl.node_ids() {
            let info = self.snarl.get_node_info(id).expect("Node should exist");
            let packed_node = PackedNode {
                id: node.id(),
                data: node
                    .write_json(registry)
                    .with_context(|| format!("failed to serialize node {:?}", id))?,
                pos: info.pos,
                open: info.open,
                color_scheme: info.value.color_scheme.as_ref().map(|c| c.pack()),
                custom_title: info.value.custom_title.clone(),
            };
            packed.nodes.push((id, packed_node));
        }

        for (out_pin, in_pin) in self.snarl.wires() {
            // do not serialize inputs with connections
            inline_values.remove(&in_pin);
            packed.edges.push((out_pin, in_pin));
        }

        packed.inline_values = inline_values.into_iter().collect();
        packed.inline_values.sort_by_key(|(in_pin, _)| *in_pin);
        packed.nodes.sort_by_key(|(id, _)| *id);
        packed
            .edges
            .sort_by_key(|(out_pin, in_pin)| (*out_pin, *in_pin));

        serde_json::value::to_value(&packed)
            .into_diagnostic()
            .with_context(|| "failed to serialize graph")
    }

    pub fn snarl(&self) -> &Snarl<SnarlNode> {
        &self.snarl
    }

    pub fn snarl_mut(&mut self) -> &mut Snarl<SnarlNode> {
        &mut self.snarl
    }

    pub fn snarl_and_context<'a>(
        &'a mut self,
        registry: &'a ETypesRegistry,
        docs: &'a Docs,
    ) -> (&'a mut Snarl<SnarlNode>, NodeContext<'a>) {
        (
            &mut self.snarl,
            NodeContext {
                registry,
                docs,
                inputs: &self.inputs,
                outputs: &self.outputs,
                regions: &self.regions,
                region_graph: &self.region_graph,
                graphs: None,
            },
        )
    }

    pub fn inputs(&self) -> &SmallVec<[GraphInput; 1]> {
        &self.inputs
    }

    pub fn inputs_mut(&mut self) -> &mut SmallVec<[GraphInput; 1]> {
        &mut self.inputs
    }

    pub fn outputs(&self) -> &SmallVec<[GraphOutput; 1]> {
        &self.outputs
    }

    pub fn outputs_mut(&mut self) -> &mut SmallVec<[GraphOutput; 1]> {
        &mut self.outputs
    }

    pub fn regions(&self) -> &OrderMap<Uuid, RegionInfo> {
        &self.regions
    }

    pub fn regions_mut(&mut self) -> &mut OrderMap<Uuid, RegionInfo> {
        &mut self.regions
    }

    pub fn region_graph(&self) -> &RegionGraph {
        &self.region_graph
    }

    pub fn region_graph_mut(&mut self) -> &mut RegionGraph {
        &mut self.region_graph
    }

    pub fn ensure_region_graph_ready(&mut self) {
        self.region_graph.ensure_ready(&self.snarl)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct PackedGraph {
    nodes: Vec<(NodeId, PackedNode)>,
    edges: Vec<(OutPinId, InPinId)>,
    inline_values: Vec<(InPinId, JsonValue)>,
    #[serde(default)]
    inputs: SmallVec<[GraphInput; 1]>,
    #[serde(default)]
    outputs: SmallVec<[GraphOutput; 1]>,
    #[serde(default)]
    regions: Vec<RegionInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PackedNode {
    id: Ustr,
    data: JsonValue,
    pos: Pos2,
    open: bool,
    #[serde(default)]
    color_scheme: Option<PackedNodeColorScheme>,
    #[serde(default)]
    custom_title: Option<String>,
}
