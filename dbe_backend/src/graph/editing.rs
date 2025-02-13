use crate::graph::execution::GraphExecutionContext;
use crate::graph::inputs::{GraphInput, GraphOutput};
use crate::graph::node::commands::{SnarlCommand, SnarlCommands};
use crate::graph::node::enum_node::EnumNode;
use crate::graph::node::groups::subgraph::SubgraphNode;
use crate::graph::node::list::ListNode;
use crate::graph::node::struct_node::StructNode;
use crate::graph::node::{get_node_factory, Node, NodeContext, SnarlNode};
use crate::graph::region::region_graph::RegionGraph;
use crate::graph::region::RegionInfo;
use crate::graph::Graph;
use crate::m_try;
use crate::project::docs::Docs;
use crate::project::project_graph::ProjectGraphs;
use crate::project::side_effects::SideEffectsContext;
use crate::registry::{EObjectType, ETypesRegistry};
use crate::value::id::{EListId, ETypeId};
use crate::value::EValue;
use egui_snarl::{InPin, InPinId, NodeId, OutPin, OutPinId, Snarl};
use emath::{Pos2, Vec2};
use inline_tweak::tweak;
use itertools::Itertools;
use miette::{miette, Context};
use smallvec::{smallvec, SmallVec};
use std::ops::{Deref, DerefMut};
use ustr::Ustr;
use utils::map::OrderMap;
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

#[derive(Debug)]
pub struct GraphEditingContext<'a, 'snarl> {
    pub snarl: &'snarl mut Snarl<SnarlNode>,
    ctx: PartialGraphEditingContext<'a>,
}

impl<'a> GraphEditingContext<'a, 'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn from_graph(
        graph: &'a mut Graph,
        registry: &'a ETypesRegistry,
        docs: &'a Docs,
        graphs: Option<&'a ProjectGraphs>,
        side_effects: SideEffectsContext<'a>,
        is_node_group: bool,
        input_values: &'a [EValue],
        output_values: &'a mut Option<Vec<EValue>>,
    ) -> Self {
        let (ctx, snarl) = PartialGraphEditingContext::from_graph(
            graph,
            registry,
            docs,
            graphs,
            side_effects,
            is_node_group,
            input_values,
            output_values,
        );
        Self { snarl, ctx }
    }

    pub fn from_graph_and_context(
        graph: &'a mut Graph,
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
}

impl<'a> Deref for GraphEditingContext<'a, '_> {
    type Target = PartialGraphEditingContext<'a>;

    fn deref(&self) -> &Self::Target {
        &self.ctx
    }
}
impl DerefMut for GraphEditingContext<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ctx
    }
}

impl GraphEditingContext<'_, '_> {
    pub fn as_execution_context(&mut self) -> GraphExecutionContext {
        GraphExecutionContext::new(
            self.snarl,
            self.ctx.inputs,
            self.ctx.outputs,
            self.ctx.inline_values,
            self.ctx.registry,
            self.ctx.docs,
            self.ctx.graphs,
            self.ctx.side_effects.clone(),
            self.ctx.is_node_group,
            self.ctx.input_values,
            self.ctx.output_values,
            self.ctx.regions,
            self.ctx.region_graph,
        )
    }

    pub fn as_node_context(&self) -> NodeContext {
        node_context!(self)
    }

    /// Ensures that the inline input value of the given pin is present
    pub fn ensure_inline_input(&mut self, pin: InPinId) -> miette::Result<bool> {
        let node = &self.snarl[pin.node];

        if !node.has_inline_values(pin.input) {
            return Ok(false);
        }

        if !node
            .try_input(node_context!(self.ctx), pin.input)?
            .ty
            .has_inline_value(self.registry)
        {
            return Ok(false);
        };

        match self.ctx.inline_values.entry(pin) {
            utils::map::OrderMapEntry::Occupied(_) => Ok(true),
            utils::map::OrderMapEntry::Vacant(e) => {
                let value = node.default_input_value(node_context!(self.ctx), pin.input)?;
                e.insert(value.into_owned());
                Ok(true)
            }
        }
    }

    /// Returns mutable reference to the input value of the given pin
    pub fn get_inline_input_mut(&mut self, pin: InPinId) -> miette::Result<Option<&mut EValue>> {
        if !self.ensure_inline_input(pin)? {
            return Ok(None);
        }

        Ok(Some(
            self.inline_values
                .get_mut(&pin)
                .expect("input value should be present"),
        ))
    }

    pub fn update_all_nodes_state(&mut self, commands: &mut SnarlCommands) -> miette::Result<()> {
        for (id, node) in self.snarl.nodes_ids_mut() {
            node.update_state(node_context!(self.ctx), commands, id)
                .with_context(|| format!("failed to update state of node: {:?}", id))?;
        }
        Ok(())
    }

    pub fn connect(
        &mut self,
        from: &OutPin,
        to: &InPin,
        commands: &mut SnarlCommands,
    ) -> miette::Result<bool> {
        let success = m_try(|| -> miette::Result<bool> {
            let from_node = &self.snarl[from.id.node];

            let from_pin = from_node.try_output(node_context!(self), from.id.output)?;

            let based_on_input = from_pin.ty.is_based_on_target();
            let can_output = if based_on_input {
                let to_node = &self.snarl[to.id.node];
                let ty = to_node.try_input(node_context!(self), to.id.input)?;
                from_node.can_output_to(node_context!(self), from, to, &ty.ty)?
            } else {
                true
            };

            if can_output {
                let to_node = &mut self.snarl[to.id.node];

                if !to_node.try_connect(
                    node_context!(self.ctx),
                    commands,
                    from,
                    to,
                    &from_pin.ty,
                )? {
                    return Ok(false);
                }

                if based_on_input {
                    let to_pin = to_node.try_input(node_context!(self.ctx), to.id.input)?;
                    let from_node = &mut self.snarl[from.id.node];
                    from_node.connected_to_output(
                        node_context!(self.ctx),
                        commands,
                        from,
                        to,
                        &to_pin.ty,
                    )?;
                }

                Ok(true)
            } else {
                Ok(false)
            }
        })
        .with_context(|| format!("failed to connect pins: {:?} -> {:?}", from.id, to.id))?;

        commands.execute(self)?;

        Ok(success)
    }

    pub fn disconnect(
        &mut self,
        from: &OutPin,
        to: &InPin,
        commands: &mut SnarlCommands,
    ) -> miette::Result<()> {
        self.snarl[to.id.node].try_disconnect(node_context!(self.ctx), commands, from, to)?;

        commands.execute(self)
    }

    pub fn remove_node(
        &mut self,
        node: NodeId,
        commands: &mut SnarlCommands,
    ) -> miette::Result<()> {
        commands.push(SnarlCommand::DeleteNode { node });

        commands
            .execute(self)
            .with_context(|| format!("failed to remove node: {:?}", node))
    }

    pub fn duplicate_node(&mut self, node_id: NodeId) -> miette::Result<()> {
        let node = self.snarl.get_node_info(node_id).unwrap();
        let factory_id = node.value.id();
        let duplicate_nodes = node.value.duplicate();
        let pos = node.pos + Vec2::splat(tweak!(50.0));
        let created_ids = get_node_factory(&factory_id)
            .unwrap()
            .create_nodes(self.snarl, pos);

        if duplicate_nodes.len() != created_ids.len() {
            for node in created_ids.iter() {
                self.snarl.remove_node(*node);
            }
            return Err(miette!(
                help =
                    "This is a bug in the node implementation, please report it to the developers.",
                "created node count mismatch. Factory: {}, created: {}",
                duplicate_nodes.len(),
                created_ids.len()
            ))
            .with_context(|| format!("failed to duplicate node {}({:?})", factory_id, node_id));
        }

        self.mark_dirty();

        for (id, duplicated_node) in created_ids.iter().zip_eq(duplicate_nodes) {
            let node = &mut self.snarl[*id].node;
            *node = duplicated_node;
        }
        self.process_created_nodes(created_ids.iter().copied())?;

        let inline_values = self
            .inline_values
            .iter()
            .filter(|(in_pin, _)| in_pin.node == node_id)
            .map(|(in_pin, value)| (in_pin.input, value.clone()))
            .collect_vec();

        for (input, value) in inline_values {
            self.inline_values.insert(
                InPinId {
                    node: created_ids[0],
                    input,
                },
                value.clone(),
            );
        }

        Ok(())
    }

    pub fn create_node(&mut self, id: Ustr, pos: Pos2) -> miette::Result<SmallVec<[NodeId; 2]>> {
        let ids = get_node_factory(&id).unwrap().create_nodes(self.snarl, pos);
        self.mark_dirty();

        self.process_created_nodes(ids.iter().copied())?;

        Ok(ids)
    }

    fn process_created_nodes(
        &mut self,
        ids: impl IntoIterator<Item = NodeId>,
    ) -> miette::Result<()> {
        for id in ids {
            let node = &self.snarl[id].node;
            for reg in [node.region_source(), node.region_end()].iter().flatten() {
                self.ctx
                    .regions
                    .entry(*reg)
                    .or_insert_with(|| RegionInfo::new(*reg));
            }
            self.inline_values.retain(|in_pin, _| in_pin.node != id);
        }

        Ok(())
    }

    pub fn create_subgraph_node(
        &mut self,
        id: Uuid,
        pos: Pos2,
    ) -> miette::Result<SmallVec<[NodeId; 2]>> {
        let node = Box::new(SubgraphNode::with_graph(id));
        let id = self.snarl.insert_node(pos, SnarlNode::new(node));
        self.mark_dirty();
        self.process_created_nodes([id])?;

        Ok(smallvec![id])
    }

    pub fn create_object_node(
        &mut self,
        object: ETypeId,
        pos: Pos2,
        apply_value: Option<EValue>,
    ) -> miette::Result<SmallVec<[NodeId; 2]>> {
        let info = self
            .registry
            .get_object(&object)
            .expect("object id should be valid");
        let info = info.deref();
        let node: Box<dyn Node> = match (info, &apply_value) {
            (EObjectType::Struct(_), None) => Box::new(StructNode::new(object)),
            (EObjectType::Struct(_), Some(value)) => Box::new(StructNode::from_value(value)?),
            (EObjectType::Enum(data), None) => Box::new(EnumNode::new(data.variant_ids()[0])),
            (EObjectType::Enum(_), Some(value)) => Box::new(EnumNode::from_value(value)?),
        };

        let id = self.snarl.insert_node(pos, SnarlNode::new(node));
        self.inline_values.retain(|in_pin, _| in_pin.node != id);

        if let Some(value) = apply_value {
            match (info, value) {
                (EObjectType::Struct(_), EValue::Struct { fields, .. }) => {
                    for (idx, (_, value)) in fields.into_iter().enumerate() {
                        self.inline_values.insert(
                            InPinId {
                                node: id,
                                input: idx,
                            },
                            value,
                        );
                    }
                }
                (EObjectType::Enum(_), EValue::Enum { data, .. }) => {
                    self.inline_values
                        .insert(InPinId { node: id, input: 0 }, *data);
                }
                _ => unreachable!(),
            }
        }

        self.mark_dirty();

        Ok(smallvec![id])
    }

    pub fn create_list_node(
        &mut self,
        item_ty: EListId,
        pos: Pos2,
    ) -> miette::Result<SmallVec<[NodeId; 2]>> {
        let item_ty = self
            .registry
            .get_list(&item_ty)
            .expect("list id should be valid")
            .value_type;
        let node = Box::new(ListNode::of_type(item_ty));
        let id = self.snarl.insert_node(pos, SnarlNode::new(node));
        self.inline_values.retain(|in_pin, _| in_pin.node != id);
        self.mark_dirty();

        Ok(smallvec![id])
    }

    pub fn read_output(&mut self, id: OutPinId) -> miette::Result<EValue> {
        self.as_execution_context().read_output(id)
    }

    pub fn read_input(&mut self, id: InPinId) -> miette::Result<EValue> {
        self.as_execution_context().read_input(id)
    }

    pub fn mark_dirty(&mut self) {
        self.region_graph.mark_dirty();
    }

    pub fn ensure_regions_graph_ready(&mut self) -> &mut RegionGraph {
        self.ctx.region_graph.ensure_ready(self.snarl);
        self.region_graph
    }
}

#[derive(Debug)]
pub struct PartialGraphEditingContext<'a> {
    pub inline_values: &'a mut OrderMap<InPinId, EValue>,
    pub inputs: &'a mut SmallVec<[GraphInput; 1]>,
    pub outputs: &'a mut SmallVec<[GraphOutput; 1]>,
    pub regions: &'a mut OrderMap<Uuid, RegionInfo>,
    pub region_graph: &'a mut RegionGraph,
    pub registry: &'a ETypesRegistry,
    pub docs: &'a Docs,
    pub graphs: Option<&'a ProjectGraphs>,
    side_effects: SideEffectsContext<'a>,
    is_node_group: bool,
    input_values: &'a [EValue],
    output_values: &'a mut Option<Vec<EValue>>,
}

impl<'a> PartialGraphEditingContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn from_graph(
        graph: &'a mut Graph,
        registry: &'a ETypesRegistry,
        docs: &'a Docs,
        graphs: Option<&'a ProjectGraphs>,
        side_effects: SideEffectsContext<'a>,
        is_node_group: bool,
        input_values: &'a [EValue],
        output_values: &'a mut Option<Vec<EValue>>,
    ) -> (Self, &'a mut Snarl<SnarlNode>) {
        (
            PartialGraphEditingContext {
                inline_values: &mut graph.inline_values,
                inputs: &mut graph.inputs,
                registry,
                docs,
                graphs,
                side_effects,
                is_node_group,
                input_values,
                outputs: &mut graph.outputs,
                output_values,
                regions: &mut graph.regions,
                region_graph: &mut graph.region_graph,
            },
            &mut graph.snarl,
        )
    }

    pub fn as_full<'b, 'snarl>(
        &'b mut self,
        snarl: &'snarl mut Snarl<SnarlNode>,
    ) -> GraphEditingContext<'b, 'snarl>
    where
        'a: 'b,
    {
        GraphEditingContext {
            snarl,
            ctx: PartialGraphEditingContext {
                inline_values: self.inline_values,
                registry: self.registry,
                docs: self.docs,
                graphs: self.graphs,
                side_effects: self.side_effects.clone(),
                inputs: self.inputs,
                outputs: self.outputs,
                input_values: self.input_values,
                output_values: self.output_values,
                is_node_group: self.is_node_group,
                regions: self.regions,
                region_graph: self.region_graph,
            },
        }
    }

    pub fn as_node_context(&self) -> NodeContext {
        node_context!(self)
    }
}
