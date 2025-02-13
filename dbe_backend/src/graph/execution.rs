use crate::graph::cache::GraphCache;
use crate::graph::inputs::{GraphInput, GraphOutput};
use crate::graph::node::extras::ExecutionExtras;
use crate::graph::node::ports::NodePortType;
use crate::graph::node::SnarlNode;
use crate::graph::node::{ExecutionResult, NodeContext};
use crate::graph::region::region_graph::{RegionGraph, RegionGraphData};
use crate::graph::region::{RegionExecutionData, RegionInfo};
use crate::graph::Graph;
use crate::m_try;
use crate::project::docs::Docs;
use crate::project::project_graph::ProjectGraphs;
use crate::project::side_effects::SideEffectsContext;
use crate::registry::ETypesRegistry;
use crate::value::EValue;
use egui_snarl::{InPinId, NodeId, OutPinId, Snarl};
use maybe_owned::MaybeOwnedMut;
use miette::{bail, miette, Context};
use smallvec::SmallVec;
use std::ops::{Deref, DerefMut};
use utils::map::{HashMap, OrderMap};
use uuid::Uuid;

macro_rules! node_context {
    ($source:expr) => {
        NodeContext {
            registry: $source.registry,
            docs: $source.docs,
            inputs: $source.inputs,
            outputs: $source.outputs,
            regions: $source.regions,
            region_graph: $source.region_graph,
            graphs: $source.graphs,
        }
    };
}

#[derive(derive_more::Debug)]
pub struct GraphExecutionContext<'a, 'snarl> {
    pub snarl: &'snarl Snarl<SnarlNode>,
    pub ctx: PartialGraphExecutionContext<'a>,
}

impl<'a> GraphExecutionContext<'a, 'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn from_graph(
        graph: &'a Graph,
        registry: &'a ETypesRegistry,
        docs: &'a Docs,
        graphs: Option<&'a ProjectGraphs>,
        side_effects: SideEffectsContext<'a>,
        is_node_group: bool,
        input_values: &'a [EValue],
        output_values: &'a mut Option<Vec<EValue>>,
    ) -> Self {
        Self::new(
            &graph.snarl,
            &graph.inputs,
            &graph.outputs,
            &graph.inline_values,
            registry,
            docs,
            graphs,
            side_effects,
            is_node_group,
            input_values,
            output_values,
            graph.regions(),
            graph.region_graph(),
        )
    }

    pub fn from_graph_and_context(
        graph: &'a Graph,
        context: NodeContext<'a>,
        side_effects: SideEffectsContext<'a>,
        is_node_group: bool,
        input_values: &'a [EValue],
        output_values: &'a mut Option<Vec<EValue>>,
    ) -> Self {
        Self::from_graph(
            graph,
            context.registry,
            context.docs,
            context.graphs,
            side_effects,
            is_node_group,
            input_values,
            output_values,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        snarl: &'a Snarl<SnarlNode>,
        inputs: &'a SmallVec<[GraphInput; 1]>,
        outputs: &'a SmallVec<[GraphOutput; 1]>,
        inline_values: &'a OrderMap<InPinId, EValue>,
        registry: &'a ETypesRegistry,
        docs: &'a Docs,
        graphs: Option<&'a ProjectGraphs>,
        side_effects: SideEffectsContext<'a>,
        is_node_group: bool,
        input_values: &'a [EValue],
        output_values: &'a mut Option<Vec<EValue>>,
        regions: &'a OrderMap<Uuid, RegionInfo>,
        region_graph: &'a RegionGraph,
    ) -> Self {
        Self {
            snarl,
            ctx: PartialGraphExecutionContext {
                inputs,
                outputs,
                inline_values,
                registry,
                docs,
                side_effects,
                graphs,
                cache: Default::default(),
                input_values,
                output_values,
                is_node_group,
                regions,
                region_graph,
                regional_data: Default::default(),
            },
        }
    }
}

impl<'a> Deref for GraphExecutionContext<'a, '_> {
    type Target = PartialGraphExecutionContext<'a>;

    fn deref(&self) -> &Self::Target {
        &self.ctx
    }
}
impl DerefMut for GraphExecutionContext<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ctx
    }
}

impl GraphExecutionContext<'_, '_> {
    /// Marks the node and all downstream nodes as dirty
    pub fn mark_dirty(&mut self, node: NodeId) {
        self.mark_dirty_inner(node, &mut SmallVec::new());
    }

    pub fn full_eval(&mut self, side_effects: bool) -> miette::Result<()> {
        self.region_graph
            .try_as_data()
            .context("Graph structure is invalid")?;

        self.cache.clear();
        for (id, node) in self.snarl.node_ids() {
            if !node.has_side_effects() {
                continue;
            }

            // Skip executing nodes in regions, except for the top-level region endpoints
            let regions_graph = expect_region_graph(self.region_graph);
            if !should_run_node(regions_graph, id, node, None) {
                continue;
            }

            self.eval_node_if_uncached(id, side_effects)?
        }

        Ok(())
    }

    pub fn read_output(&mut self, id: OutPinId) -> miette::Result<EValue> {
        self.read_node_output_inner(id, false)
    }

    pub fn read_input(&mut self, id: InPinId) -> miette::Result<EValue> {
        let node = self
            .snarl
            .get_node(id.node)
            .ok_or_else(|| miette!("Node {:?} not found", id.node))?;
        self.read_node_input_inner(id, node, false)
    }
}

impl GraphExecutionContext<'_, '_> {
    pub fn mark_dirty_inner(&mut self, node: NodeId, marked: &mut SmallVec<[NodeId; 4]>) {
        if marked.contains(&node) {
            return;
        }
        marked.push(node);

        if self.cache.remove(&node).is_none() {
            // Node was not cached, so it's already dirty
            return;
        }

        for (out_pin, in_pin) in self.snarl.wires() {
            if out_pin.node == node {
                self.mark_dirty_inner(in_pin.node, marked);
            }
        }
    }

    fn read_node_output_inner(
        &mut self,
        pin: OutPinId,
        side_effects: bool,
    ) -> miette::Result<EValue> {
        // trace!("Reading output #{} of node {:?}", pin.output, pin.node);
        m_try(|| {
            if let Some(node) = self.cache.get(&pin.node) {
                return Ok(node
                    .get(pin.output)
                    .ok_or_else(|| miette!("Node doesn't have output #{}", pin.output))?
                    .clone());
            }

            self.eval_node_inner(pin.node, side_effects)?;

            let node = self.cache.get(&pin.node).ok_or_else(|| {
                miette!("!!INTERNAL ERROR!! Node was not cached after evaluation")
            })?;

            Ok(node
                .get(pin.output)
                .ok_or_else(|| miette!("Node doesn't have output #{}", pin.output))?
                .clone())
        })
        .with_context(|| {
            format!(
                "failed to read output #{} of node {:?}",
                pin.output, pin.node
            )
        })
    }

    fn inline_input_value(
        &mut self,
        pin: InPinId,
        node: &SnarlNode,
    ) -> miette::Result<Option<&EValue>> {
        if !node.has_inline_values(pin.input) {
            return Ok(None);
        }
        if !node
            .try_input(node_context!(self), pin.input)?
            .ty
            .has_inline_value(self.registry)
        {
            return Ok(None);
        }
        Ok(self.inline_values.get(&pin))
    }

    fn read_node_input_inner(
        &mut self,
        id: InPinId,
        node: &SnarlNode,
        side_effects: bool,
    ) -> miette::Result<EValue> {
        let in_info = node.try_input(node_context!(self), id.input)?;
        // trace!("Reading input #{} of node {:?}", id.input, id.node);
        m_try(|| {
            // TODO: check for valid types
            let slot = self.snarl.in_pin(id);
            let value = if slot.remotes.is_empty() {
                match self.inline_input_value(slot.id, node)? {
                    None => node
                        .default_input_value(node_context!(self), id.input)?
                        .into_owned(),
                    Some(val) => val.clone(),
                }
            } else if slot.remotes.len() == 1 {
                let remote = slot.remotes[0];
                let output = self.read_node_output_inner(remote, side_effects)?;
                if output.ty() != in_info.ty.ty() {
                    let remote_info =
                        self.snarl[remote.node].try_output(node_context!(self), remote.output)?;

                    NodePortType::convert_value(
                        self.registry,
                        &remote_info.ty,
                        &in_info.ty,
                        output,
                    )?
                } else {
                    output
                }
            } else {
                // TODO: allow multi-connect for inputs
                bail!(
                    "Node {:?} input #{} is connected to multiple outputs",
                    id,
                    id.input
                );
            };

            Ok(value)
        })
        .with_context(|| format!("failed to read input #{} of node {:?}", id.input, id.node))
    }

    fn eval_node_if_uncached(&mut self, id: NodeId, side_effects: bool) -> miette::Result<()> {
        if self.cache.contains_key(&id) {
            return Ok(());
        }

        self.eval_node_inner(id, side_effects)
    }

    fn eval_node_inner(&mut self, id: NodeId, run_side_effects: bool) -> miette::Result<()> {
        // trace!("Evaluating node {:?}", id);
        let mut iteration = 0;
        m_try(|| {
            loop {
                iteration += 1;
                let node = self
                    .snarl
                    .get_node(id)
                    .ok_or_else(|| miette!("Node {:?} not found", id))?;

                let inputs_count = node.inputs_count(node_context!(self));
                let mut input_values = Vec::<EValue>::with_capacity(inputs_count);

                let region_graph = expect_region_graph(self.region_graph);
                if let Some(region) = node.region_end() {
                    let start = region_graph.region_data(&region).start_node;
                    // always execute the start node of the region
                    self.eval_node_if_uncached(start, run_side_effects)?;
                }

                if node.should_execute_dependencies(
                    node_context!(self.ctx),
                    &mut ExecutionExtras::new(
                        self.ctx.is_node_group,
                        self.ctx.input_values,
                        self.ctx.output_values,
                        self.ctx.regional_data.as_mut(),
                        self.ctx.side_effects.with_node(id),
                    ),
                )? {
                    for i in 0..inputs_count {
                        input_values.push(self.read_node_input_inner(
                            InPinId { node: id, input: i },
                            node,
                            run_side_effects,
                        )?);
                    }

                    if run_side_effects {
                        if let Some(region) = node.region_end() {
                            let data = region_graph.region_data(&region);

                            if data.has_side_effects {
                                for node in region_graph.region_nodes(&region) {
                                    let node_data = &self.snarl[node.node];
                                    // only evaluate separation 0 nodes (direct nodes of the region, excluding children)
                                    // or if the node is a separation 1 node and is the end node of the region
                                    if node.node != id
                                        && node_data.has_side_effects()
                                        && (node.separation == 0
                                            || (node.separation == 1
                                                && node_data.region_end().is_some()))
                                    {
                                        self.eval_node_if_uncached(node.node, run_side_effects)?;
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // When dependencies are not executed, provide default values
                    for i in 0..inputs_count {
                        input_values.push(
                            node.try_input(node_context!(self), i)?
                                .ty
                                .default_value(self.registry)
                                .into_owned(),
                        );
                    }
                }

                let outputs_count = node.outputs_count(node_context!(self));
                let mut outputs = Vec::with_capacity(outputs_count);

                let result = node.execute(
                    node_context!(self.ctx),
                    &input_values,
                    &mut outputs,
                    &mut ExecutionExtras::new(
                        self.ctx.is_node_group,
                        self.ctx.input_values,
                        self.ctx.output_values,
                        self.ctx.regional_data.as_mut(),
                        self.ctx.side_effects.with_node(id),
                    ),
                )?;

                match result {
                    ExecutionResult::Done => {
                        // TODO: check for validity of returned values types
                        self.cache.insert(id, outputs);
                        return Ok(());
                    }
                    ExecutionResult::RerunRegion { region } => {
                        let data = expect_region_graph(self.region_graph);

                        // clear cache of all nodes in the region
                        for node in data.region_nodes(&region) {
                            self.cache.remove(&node.node);
                        }
                    }
                }
            }
        })
        .with_context(|| {
            if iteration == 1 {
                format!("failed to evaluate node {:?}", id)
            } else {
                format!(
                    "failed to evaluate iteration #{} of node {:?}",
                    iteration, id
                )
            }
        })
    }
}

fn expect_region_graph(region_graph: &RegionGraph) -> &RegionGraphData {
    region_graph
        .try_as_data()
        .expect("Region graph was checked for before execution started")
}

fn should_run_node(
    regions_graph: &RegionGraphData,
    id: NodeId,
    node: &SnarlNode,
    cur_region: Option<Uuid>,
) -> bool {
    // Can run nodes without region
    let Some(region) = regions_graph.node_region(&id) else {
        return true;
    };

    if let Some(cur_region) = cur_region {
        // Can run the node in the same region
        if cur_region == region {
            return true;
        }

        // Can't run nodes in region that isn't the direct child
        if regions_graph
            .region_parents(&region)
            .first()
            .is_none_or(|reg| *reg != cur_region)
        {
            return false;
        }
    } else {
        // Can't run nodes in region that has parents
        if !regions_graph.region_parents(&region).is_empty() {
            return false;
        }
    }

    // Can only run node in the direct child region if it's the end node
    node.region_end() == Some(region)
}

#[derive(derive_more::Debug)]
pub struct PartialGraphExecutionContext<'a> {
    pub inputs: &'a SmallVec<[GraphInput; 1]>,
    pub outputs: &'a SmallVec<[GraphOutput; 1]>,
    pub inline_values: &'a OrderMap<InPinId, EValue>,
    pub registry: &'a ETypesRegistry,
    pub docs: &'a Docs,
    pub graphs: Option<&'a ProjectGraphs>,
    pub side_effects: SideEffectsContext<'a>,
    pub is_node_group: bool,
    pub input_values: &'a [EValue],
    pub output_values: &'a mut Option<Vec<EValue>>,
    pub regions: &'a OrderMap<Uuid, RegionInfo>,
    pub region_graph: &'a RegionGraph,
    #[debug("(...)")]
    pub regional_data: MaybeOwnedMut<'a, HashMap<Uuid, Box<dyn RegionExecutionData>>>,
    cache: MaybeOwnedMut<'a, GraphCache>,
}

impl<'a> PartialGraphExecutionContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn from_graph(
        graph: &'a Graph,
        registry: &'a ETypesRegistry,
        docs: &'a Docs,
        graphs: Option<&'a ProjectGraphs>,
        side_effects: SideEffectsContext<'a>,
        is_node_group: bool,
        input_values: &'a [EValue],
        output_values: &'a mut Option<Vec<EValue>>,
    ) -> (Self, &'a Snarl<SnarlNode>) {
        (
            Self {
                inputs: &graph.inputs,
                outputs: &graph.outputs,
                inline_values: &graph.inline_values,
                cache: Default::default(),
                registry,
                docs,
                graphs,
                side_effects,
                is_node_group,
                input_values,
                output_values,
                regions: graph.regions(),
                region_graph: graph.region_graph(),
                regional_data: Default::default(),
            },
            &graph.snarl,
        )
    }

    pub fn as_full<'b, 'snarl>(
        &'b mut self,
        snarl: &'snarl Snarl<SnarlNode>,
    ) -> GraphExecutionContext<'b, 'snarl>
    where
        'a: 'b,
    {
        GraphExecutionContext {
            snarl,
            ctx: PartialGraphExecutionContext {
                inputs: self.inputs,
                outputs: self.outputs,
                inline_values: self.inline_values,
                cache: MaybeOwnedMut::Borrowed(&mut self.cache),
                registry: self.registry,
                docs: self.docs,
                side_effects: self.side_effects.clone(),
                graphs: self.graphs,
                is_node_group: self.is_node_group,
                input_values: self.input_values,
                output_values: self.output_values,
                regions: self.regions,
                region_graph: self.region_graph,
                regional_data: MaybeOwnedMut::Borrowed(&mut self.regional_data),
            },
        }
    }
}
