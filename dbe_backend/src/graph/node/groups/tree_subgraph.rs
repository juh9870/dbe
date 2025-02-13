use crate::etype::default::DefaultEValue;
use crate::graph::node::commands::{SnarlCommand, SnarlCommands};
use crate::graph::node::creation::NodeCombo;
use crate::graph::node::extras::ExecutionExtras;
use crate::graph::node::groups::utils::sync_fields;
use crate::graph::node::ports::fields::IoDirection;
use crate::graph::node::ports::{InputData, NodePortType, OutputData};
use crate::graph::node::{ExecutionResult, Node, NodeContext, NodeFactory, SnarlNode};
use crate::graph::Graph;
use crate::json_utils::json_serde::JsonSerde;
use crate::json_utils::JsonValue;
use crate::project::docs::{Docs, DocsRef};
use crate::project::project_graph::ProjectGraphs;
use crate::registry::ETypesRegistry;
use crate::value::EValue;
use egui_snarl::{InPin, InPinId, NodeId, OutPin, OutPinId};
use emath::pos2;
use miette::{bail, miette, Context, IntoDiagnostic};
use serde_json::json;
use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};
use ustr::Ustr;
use uuid::Uuid;

const DEFAULT_MAX_ITERATIONS: usize = 10;

#[derive(Debug, Clone, Hash)]
pub struct TreeSubgraph {
    tree: AccessWrapper<tree::TreeGraphData>,
    inputs: Vec<Uuid>,
    outputs: Vec<Uuid>,
    connected_inputs: BTreeMap<Uuid, bool>,
}

impl TreeSubgraph {
    pub fn new(node: SnarlNode) -> Self {
        let mut graph = Graph::default();
        let root = graph.snarl.insert_node(pos2(0.0, 0.0), node);
        Self {
            tree: AccessWrapper(tree::TreeGraphData::new(graph, root)),
            inputs: vec![],
            outputs: vec![],
            connected_inputs: Default::default(),
        }
    }

    pub fn create_input(
        &mut self,
        context: NodeContext,
        input: usize,
        node: NodeCombo,
    ) -> miette::Result<()> {
        self.tree.insert_node(context.into(), input, node)
    }

    pub fn tree_cache(&mut self, context: NodeContext) -> &TreeState {
        self.tree.calculate_tree_cache(context.into())
    }

    pub fn node_title<'a>(&self, id: NodeId, context: impl Into<TreeContext<'a>>) -> String {
        self.tree.node_title(id, context.into())
    }

    fn mark_dirty(&mut self) {
        self.tree.clear_cache()
    }
}
impl Node for TreeSubgraph {
    fn write_json(&self, registry: &ETypesRegistry) -> miette::Result<JsonValue> {
        let tree = self
            .tree
            .write_json(registry, ())
            .context("failed to serialize `tree` field")?;
        let inputs = serde_json::to_value(&self.inputs)
            .into_diagnostic()
            .context("failed to serialize `inputs` field")?;
        let outputs = serde_json::to_value(&self.outputs)
            .into_diagnostic()
            .context("failed to serialize `outputs` field")?;
        let connected_inputs = serde_json::to_value(&self.connected_inputs)
            .into_diagnostic()
            .context("failed to serialize `connected_inputs` field")?;
        Ok(json!({
            "tree": tree,
            "inputs": inputs,
            "outputs": outputs,
            "connected_inputs": connected_inputs,
        }))
    }

    fn parse_json(
        &mut self,
        registry: &ETypesRegistry,
        value: &mut JsonValue,
    ) -> miette::Result<()> {
        let JsonValue::Object(mut obj) = value.take() else {
            bail!("expected object");
        };
        self.tree
            .parse_json(
                registry,
                (),
                &mut obj
                    .remove("tree")
                    .ok_or_else(|| miette!("missing `tree` field"))?,
            )
            .context("failed to deserialize `tree` field")?;
        self.inputs = serde_json::from_value(
            obj.remove("inputs")
                .ok_or_else(|| miette!("missing `inputs` field"))?,
        )
        .into_diagnostic()
        .context("failed to deserialize `inputs` field")?;
        self.outputs = serde_json::from_value(
            obj.remove("outputs")
                .ok_or_else(|| miette!("missing `outputs` field"))?,
        )
        .into_diagnostic()
        .context("failed to deserialize `outputs` field")?;
        self.connected_inputs = serde_json::from_value(
            obj.remove("connected_inputs")
                .ok_or_else(|| miette!("missing `connected_inputs` field"))?,
        )
        .into_diagnostic()
        .context("failed to deserialize `connected_inputs` field")?;

        Ok(())
    }

    fn id(&self) -> Ustr {
        TreeSubgraphFactory.id()
    }

    fn default_input_value(
        &self,
        context: NodeContext,
        input: usize,
    ) -> miette::Result<DefaultEValue> {
        let Some((node, pin)) = self.tree.node_for_input(input) else {
            bail!("input cache not initialized")
        };

        node.default_input_value(context, pin.input)
    }

    fn title(&self, context: NodeContext) -> String {
        let title = self.tree.root_node().title(context);
        format!("{} (Nested)", title)
    }

    fn update_state(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        id: NodeId,
    ) -> miette::Result<()> {
        for _ in 0..DEFAULT_MAX_ITERATIONS {
            self.tree.update_nodes_state(context.into())?;

            let bad_body = self.tree.sync_tree_to_graph();

            let changed =
                self.tree
                    .sync_tree_state(context.into(), &mut self.connected_inputs, bad_body)?;

            if changed {
                let inputs = self.tree.group_inputs();

                sync_fields(
                    commands,
                    inputs,
                    &mut self.inputs,
                    None,
                    id,
                    IoDirection::Input(0),
                );

                self.outputs.clear();
                self.outputs
                    .extend(self.tree.group_outputs().iter().map(|x| x.id));

                for idx in 0..self.outputs.len() {
                    commands.push(SnarlCommand::ReconnectOutput {
                        id: OutPinId {
                            node: id,
                            output: idx,
                        },
                    })
                }
            } else {
                return Ok(());
            }
        }

        bail!(
            "failed to stabilize tree subgraph within {} iterations",
            DEFAULT_MAX_ITERATIONS
        )
    }

    fn has_inline_values(&self, input: usize) -> bool {
        let Some((node, pin)) = self.tree.node_for_input(input) else {
            return false;
        };

        node.has_inline_values(pin.input)
    }

    fn inputs_count(&self, _context: NodeContext) -> usize {
        self.inputs.len()
    }

    fn input_unchecked(&self, context: NodeContext, input: usize) -> miette::Result<InputData> {
        let Some((node, pin)) = self.tree.node_for_input(input) else {
            bail!("input cache not initialized")
        };

        let mut input = node.input_unchecked(context, pin.input)?;

        if input.custom_docs.is_none() {
            input.custom_docs = Some(DocsRef::NodeInput(node.node.id(), input.name));
        }

        Ok(input)
    }

    fn outputs_count(&self, _context: NodeContext) -> usize {
        self.outputs.len()
    }

    fn output_unchecked(&self, context: NodeContext, output: usize) -> miette::Result<OutputData> {
        let (node, pin) = self.tree.node_for_output(output);

        let mut output = node.output_unchecked(context, pin.output)?;

        if output.custom_docs.is_none() {
            output.custom_docs = Some(DocsRef::NodeInput(node.node.id(), output.name));
        }

        Ok(output)
    }

    fn try_connect(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        from: &OutPin,
        to: &InPin,
        incoming_type: &NodePortType,
    ) -> miette::Result<bool> {
        if self._default_try_connect(context, commands, from, to, incoming_type)? {
            self.connected_inputs.insert(self.inputs[to.id.input], true);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn try_disconnect(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        from: &OutPin,
        to: &InPin,
    ) -> miette::Result<()> {
        self.connected_inputs
            .insert(self.inputs[to.id.input], false);
        self._default_try_disconnect(context, commands, from, to)?;
        Ok(())
    }

    fn has_side_effects(&self) -> bool {
        self.tree.all_nodes().any(|n| n.1.has_side_effects())
    }

    fn execute(
        &self,
        context: NodeContext,
        inputs: &[EValue],
        outputs: &mut Vec<EValue>,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<ExecutionResult> {
        self.tree
            .execute(context.into(), inputs, outputs, variables)?;
        Ok(ExecutionResult::Done)
    }
}

#[derive(Debug, Clone)]
pub struct TreeSubgraphFactory;

impl NodeFactory for TreeSubgraphFactory {
    fn id(&self) -> Ustr {
        "tree_subgraph".into()
    }

    fn categories(&self) -> &'static [&'static str] {
        &[]
    }

    fn create(&self) -> Box<dyn Node> {
        Box::new(TreeSubgraph {
            tree: AccessWrapper(tree::TreeGraphData::new_invalid()),
            inputs: vec![],
            outputs: vec![],
            connected_inputs: Default::default(),
        })
    }
}

mod tree {
    use crate::graph::editing::GraphEditingContext;
    use crate::graph::execution::GraphExecutionContext;
    use crate::graph::inputs::{GraphInput, GraphOutput};
    use crate::graph::node::commands::{SnarlCommand, SnarlCommands};
    use crate::graph::node::creation::NodeCombo;
    use crate::graph::node::extras::ExecutionExtras;
    use crate::graph::node::groups::input::GroupInputNode;
    use crate::graph::node::groups::output::GroupOutputNode;
    use crate::graph::node::groups::tree_subgraph::{TreeContext, TreeState};
    use crate::graph::node::{NodeContext, SnarlNode};
    use crate::graph::Graph;
    use crate::json_utils::json_serde::JsonSerde;
    use crate::json_utils::JsonValue;
    use crate::project::side_effects::SideEffectsContext;
    use crate::registry::ETypesRegistry;
    use crate::value::EValue;
    use collection_traits::Iterable;
    use egui_snarl::{InPinId, NodeId, OutPinId};
    use emath::pos2;
    use itertools::Itertools;
    use miette::{bail, miette, Context, IntoDiagnostic};
    use serde_json::json;
    use smallvec::smallvec;
    use std::collections::BTreeMap;
    use std::hash::{Hash, Hasher};
    use tracing::{error_span, trace};
    use utils::map::HashMap;
    use uuid::Uuid;

    #[derive(Debug, Clone)]
    pub struct TreeGraphData {
        graph: Graph,
        root: NodeId,
        tree: BTreeMap<NodeId, Vec<Option<OutPinId>>>,
        input_ids: BTreeMap<InPinId, Uuid>,
        // cached data
        tree_cache: Option<TreeState>,
        group_input_node: Option<NodeId>,
        group_output_node: Option<NodeId>,
    }

    macro_rules! context {
        ($graph:expr, $tree_context:expr) => {
            NodeContext {
                registry: $tree_context.registry,
                docs: $tree_context.docs,
                inputs: &$graph.inputs,
                outputs: &$graph.outputs,
                regions: &$graph.regions,
                region_graph: &$graph.region_graph,
                graphs: $tree_context.graphs,
            }
        };
    }

    impl TreeGraphData {
        pub fn new(graph: Graph, root: NodeId) -> Self {
            Self {
                graph,
                tree: [(root, vec![])].into_iter().collect(),
                root,
                input_ids: Default::default(),
                tree_cache: None,
                group_input_node: None,
                group_output_node: None,
            }
        }

        pub fn new_invalid() -> Self {
            Self {
                graph: Default::default(),
                root: NodeId(usize::MAX),
                tree: Default::default(),
                input_ids: Default::default(),
                tree_cache: None,
                group_input_node: None,
                group_output_node: None,
            }
        }

        pub fn group_inputs(&self) -> &[GraphInput] {
            self.graph.inputs.as_slice()
        }

        pub fn group_outputs(&self) -> &[GraphOutput] {
            self.graph.outputs.as_slice()
        }

        pub fn insert_node(
            &mut self,
            context: TreeContext,
            input: usize,
            node: NodeCombo,
        ) -> miette::Result<()> {
            self.calculate_tree_cache(context);
            let inputs = &self.tree_cache.as_ref().unwrap().inputs;
            let Some(pin) = inputs.get(input) else {
                bail!("invalid input index")
            };
            let mut commands = SnarlCommands::new();

            let mut _outputs = None;
            let mut ctx = GraphEditingContext::from_graph(
                &mut self.graph,
                context.registry,
                context.docs,
                context.graphs,
                SideEffectsContext::Unavailable,
                true,
                &[],
                &mut _outputs,
            );

            let nodes = node.create_from_input_pin(&mut ctx, pos2(0.0, 0.0), pin, &mut commands)?;
            if nodes.len() != 1 {
                for id in &nodes {
                    commands.push(SnarlCommand::DeleteNode { node: *id })
                }
            }

            commands
                .execute(&mut ctx)
                .context("failed to execute node creation commands")?;

            if nodes.len() != 1 {
                bail!("can only inline singular nodes")
            }

            let id = nodes[0];

            self.tree.insert(id, vec![]);
            let inputs = self.tree.get_mut(&pin.node).unwrap();
            inputs[pin.input] = Some(OutPinId {
                node: id,
                output: 0,
            });

            self.tree_cache = None;

            Ok(())
        }

        pub fn node_for_input(&self, input: usize) -> Option<(&SnarlNode, InPinId)> {
            let inputs = &self.tree_cache.as_ref()?.inputs;

            let pin = inputs.get(input)?;

            let node = self.graph.snarl.get_node(pin.node)?;

            Some((node, *pin))
        }

        pub fn node_for_output(&self, output: usize) -> (&SnarlNode, OutPinId) {
            (
                self.graph
                    .snarl
                    .get_node(self.root)
                    .expect("Root node should exist"),
                OutPinId {
                    node: self.root,
                    output,
                },
            )
        }

        pub fn root_node(&self) -> &SnarlNode {
            self.graph
                .snarl
                .get_node(self.root)
                .expect("Root node should exist")
        }

        pub fn all_nodes(&self) -> impl Iterator<Item = (NodeId, &SnarlNode)> {
            self.graph.snarl.node_ids()
        }

        pub fn node_title(&self, id: NodeId, context: TreeContext) -> String {
            self.graph.snarl[id].title(context!(self.graph, context))
        }

        pub fn update_nodes_state(&mut self, context: TreeContext) -> miette::Result<()> {
            let mut commands = SnarlCommands::new();

            for (id, node) in self.graph.snarl.nodes_ids_mut() {
                node.update_state(context!(self.graph, context), &mut commands, id)?;
            }

            let mut _outputs = None;
            let mut ctx = GraphEditingContext::from_graph(
                &mut self.graph,
                context.registry,
                context.docs,
                context.graphs,
                SideEffectsContext::Unavailable,
                true,
                &[],
                &mut _outputs,
            );
            commands
                .execute(&mut ctx)
                .context("failed to execute state update commands")?;

            Ok(())
        }

        pub fn execute(
            &self,
            context: TreeContext,
            inputs: &[EValue],
            outputs: &mut Vec<EValue>,
            variables: &mut ExecutionExtras,
        ) -> miette::Result<()> {
            outputs.clear();
            let mut sub_outputs = None;

            let side_effects_available = variables.side_effects.is_available();
            let mut ctx = GraphExecutionContext::from_graph(
                &self.graph,
                context.registry,
                context.docs,
                context.graphs,
                variables.side_effects.with_subgraph(Uuid::default()),
                true,
                inputs,
                &mut sub_outputs,
            );

            ctx.full_eval(side_effects_available)
                .context("failed to execute tree_subgraph")?;

            drop(ctx);

            *outputs = sub_outputs.ok_or_else(|| miette!("Node group did not emit any outputs"))?;

            Ok(())
        }

        pub fn clear_cache(&mut self) {
            self.tree_cache = None;
        }

        /// Calculates "leaf" inputs, that should be connected to the group input
        pub fn calculate_tree_cache(&mut self, context: TreeContext) -> &TreeState {
            self.graph.ensure_region_graph_ready();
            if self.tree_cache.is_none() {
                enum Item {
                    Input(InPinId),
                    Node(NodeId),
                }

                let mut inputs_count = HashMap::default();
                let mut inputs = vec![Item::Node(self.root)];
                let mut tmp_hold = vec![];

                let mut i: usize = 0;
                while i < inputs.len() {
                    match &inputs[i] {
                        Item::Input(_) => {
                            i += 1;
                        }
                        Item::Node(node) => {
                            let tree_node = self.tree.get_mut(node).unwrap();
                            let snarl_node = &self.graph.snarl[*node];
                            let count = snarl_node.inputs_count(context!(self.graph, context));
                            inputs_count.insert(*node, count);
                            tree_node.resize_with(count, Default::default);

                            for (idx, input) in tree_node.iter().enumerate() {
                                if let Some(input) = input {
                                    tmp_hold.push(Item::Node(input.node));
                                } else {
                                    tmp_hold.push(Item::Input(InPinId {
                                        node: *node,
                                        input: idx,
                                    }));
                                }
                            }

                            inputs.splice(i..=i, tmp_hold.drain(..));
                        }
                    }
                }

                let inputs = inputs
                    .into_iter()
                    .map(|x| match x {
                        Item::Input(input) => input,
                        _ => unreachable!(),
                    })
                    .collect_vec();

                let mut inverse: HashMap<NodeId, (NodeId, usize)> = HashMap::default();

                for (id, inputs) in self.tree.iter() {
                    for (idx, child) in inputs
                        .iter()
                        .enumerate()
                        .filter_map(|x| x.1.map(|id| (x.0, id)))
                    {
                        if inverse.insert(child.node, (*id, idx)).is_some() {
                            panic!("Each node should have a unique parent");
                        }
                    }
                }

                let mut hierarchy = vec![vec![]; inputs.len()];
                let mut start_of = vec![vec![]; inputs.len()];
                let mut end_of = vec![vec![]; inputs.len()];

                for (idx, id) in inputs.iter().enumerate() {
                    let mut current = id.node;
                    hierarchy[idx].push(current);
                    if id.input == 0 {
                        start_of[idx].push(current);
                    }

                    if id.input == inputs_count.get(&current).unwrap() - 1 {
                        end_of[idx].push(current);
                    }

                    while let Some((parent, input)) = inverse.get(&current) {
                        hierarchy[idx].push(*parent);
                        if *input == 0 && start_of[idx].ends_with(&[current]) {
                            start_of[idx].push(*parent);
                        }
                        if *input == inputs_count.get(parent).unwrap() - 1
                            && end_of[idx].ends_with(&[current])
                        {
                            end_of[idx].push(*parent);
                        }
                        current = *parent;
                    }
                }

                self.tree_cache = Some(TreeState {
                    inputs,
                    width: hierarchy.iter().map(|x| x.len()).max().unwrap_or(0),
                    hierarchy,
                    start_of,
                    end_of,
                })
            };

            self.tree_cache.as_ref().unwrap()
        }

        pub fn sync_tree_to_graph(&mut self) -> bool {
            let _guard = error_span!("syncing tree to graph").entered();
            fn insert_resizing<T: Default>(vec: &mut Vec<T>, index: usize, value: T) -> Option<T> {
                if vec.len() <= index {
                    vec.resize_with(index + 1, Default::default);
                    vec[index] = value;
                    return None;
                }
                let mut swapped = value;
                std::mem::swap(&mut vec[index], &mut swapped);
                Some(swapped)
            }

            let (input_node, output_node) = self.get_group_io_nodes();
            let root_node = self.root;
            let mut new_tree = BTreeMap::new();
            new_tree.insert(root_node, vec![]);
            for (out_pin, in_pin) in self.graph.snarl.wires() {
                if out_pin.node == input_node
                    || out_pin.node == root_node
                    || in_pin.node == output_node
                {
                    continue;
                }
                new_tree.entry(out_pin.node).or_default();
                insert_resizing(
                    new_tree.entry(in_pin.node).or_default(),
                    in_pin.input,
                    Some(out_pin),
                );
            }

            let dangling = self
                .graph
                .snarl
                .node_ids()
                .filter(|(id, _)| {
                    !new_tree.contains_key(id)
                        && id != &input_node
                        && id != &output_node
                        && id != &root_node
                })
                .map(|x| x.0)
                .collect_vec();

            if !dangling.is_empty() {
                // TODO: delete node?
                self.graph.region_graph.mark_dirty();
                return false;
            }

            if self.tree != new_tree {
                self.tree = new_tree;
                self.tree_cache = None;
            }

            true
        }
    }

    impl TreeGraphData {
        pub fn transform_context<'a>(&'a self, context: TreeContext<'a>) -> NodeContext<'a> {
            NodeContext {
                registry: context.registry,
                docs: context.docs,
                inputs: &self.graph.inputs,
                outputs: &self.graph.outputs,
                regions: &self.graph.regions,
                region_graph: &self.graph.region_graph,
                graphs: context.graphs,
            }
        }
        pub fn sync_tree_state(
            &mut self,
            context: TreeContext,
            connected_inputs: &mut BTreeMap<Uuid, bool>,
            full_check: bool,
        ) -> miette::Result<bool> {
            self.clear_cache();
            let all_wires = self.graph.snarl.wires().collect::<Vec<_>>();
            let _guard = error_span!("Sync Tree State", ?all_wires).entered();
            let mut commands = SnarlCommands::new();
            self.calculate_tree_cache(context);
            let (group_input_node, group_output_node) = self.get_group_io_nodes();

            let mut body_correct = !full_check || self.check_body_correctness(context);
            let mut inputs_correct =
                body_correct && self.check_inputs_correctness(context, connected_inputs);
            let mut outputs_correct = body_correct && self.check_output_correctness(context);

            if !full_check && (!inputs_correct || !outputs_correct) {
                body_correct = self.check_body_correctness(context);
                if !body_correct {
                    inputs_correct = false;
                    outputs_correct = false;
                }
            }

            if body_correct && inputs_correct && outputs_correct {
                return Ok(false);
            }

            if !body_correct {
                for (out_pin, in_pin) in self.graph.snarl.wires() {
                    commands.push(SnarlCommand::Disconnect {
                        from: out_pin,
                        to: in_pin,
                    });
                }

                for (id, connections) in &self.tree {
                    for (idx, in_node) in connections
                        .iter()
                        .enumerate()
                        .filter_map(|x| x.1.map(|y| (x.0, y)))
                    {
                        commands.push(SnarlCommand::Connect {
                            from: in_node,
                            to: InPinId {
                                node: *id,
                                input: idx,
                            },
                        });
                    }
                }
            }

            // inputs
            if !inputs_correct {
                // no need to drop again if whole body was reconstructed
                if body_correct {
                    commands.push(SnarlCommand::DropNodeOutputs {
                        from: group_input_node,
                    });
                }

                let mut ids = vec![];
                for (idx, pin) in self.tree_cache.as_ref().unwrap().inputs.iter().enumerate() {
                    let id = *self.input_ids.entry(*pin).or_insert_with(Uuid::new_v4);
                    let pin_node = &self.graph.snarl[pin.node].node;
                    let in_pin = pin_node
                        .try_input(context!(self.graph, context), pin.input)
                        .with_context(|| {
                            format!(
                                "failed to read input of node {}({:?})",
                                pin_node.id(),
                                pin.node
                            )
                        })?;
                    ids.push((id, in_pin));
                    let is_connected = *connected_inputs.entry(id).or_insert(false);
                    if is_connected || pin_node.has_inline_values(pin.input) {
                        commands.push(SnarlCommand::Connect {
                            from: OutPinId {
                                node: group_input_node,
                                output: idx,
                            },
                            to: *pin,
                        })
                    }
                }

                // Only keep live IDs
                self.input_ids.retain(|id, _| {
                    self.tree_cache
                        .as_ref()
                        .unwrap()
                        .inputs
                        .iter()
                        .any(|x| x == id)
                });

                connected_inputs.retain(|id, _| ids.iter().any(|(x, _)| x == id));

                self.graph.inputs = ids
                    .iter()
                    .map(|(id, pin)| GraphInput {
                        id: *id,
                        ty: Some(pin.ty.ty()),
                        name: pin.name.to_string(),
                    })
                    .collect();

                let in_node = self.graph.snarl[group_input_node]
                    .downcast_mut::<GroupInputNode>()
                    .expect("Group input node should have correct type");

                in_node.ids = ids.iter().map(|x| x.0).collect();
            }

            if !outputs_correct {
                // no need to drop again if whole body was reconstructed
                if body_correct {
                    commands.push(SnarlCommand::DropNodeInputs {
                        to: group_output_node,
                    });
                    commands.push(SnarlCommand::DropNodeOutputs { from: self.root });
                }

                let root_node = &self.graph.snarl[self.root].node;

                let count = root_node.outputs_count(context!(self.graph, context));
                self.graph.outputs = smallvec![];

                for i in 0..count {
                    let port = root_node
                        .try_output(context!(self.graph, context), i)
                        .with_context(|| {
                            format!(
                                "failed to read output of root node {}({:?})",
                                root_node.id(),
                                self.root
                            )
                        })?;

                    let id = Uuid::new_v4();
                    self.graph.outputs.push(GraphOutput {
                        id,
                        ty: Some(port.ty.ty()),
                        name: port.name.to_string(),
                    });

                    commands.push(SnarlCommand::Connect {
                        from: OutPinId {
                            node: self.root,
                            output: i,
                        },
                        to: InPinId {
                            node: group_output_node,
                            input: i,
                        },
                    });
                }

                self.graph.snarl[group_output_node]
                    .node
                    .downcast_mut::<GroupOutputNode>()
                    .expect("Group output node should have correct type")
                    .ids = self.graph.outputs.iter().map(|x| x.id).collect();
            }

            let mut _outputs = None;
            let mut ctx = GraphEditingContext::from_graph(
                &mut self.graph,
                context.registry,
                context.docs,
                context.graphs,
                SideEffectsContext::Unavailable,
                true,
                &[],
                &mut _outputs,
            );
            commands
                .execute(&mut ctx)
                .context("failed to execute reconstruction commands")?;

            drop(ctx);

            if !self.check_body_correctness(context)
                || !self.check_inputs_correctness(context, connected_inputs)
                || !self.check_output_correctness(context)
            {
                bail!("tree graph state is still invalid after reconstruction");
            }

            self.graph.ensure_region_graph_ready();

            Ok(true)
        }

        fn check_inputs_correctness(
            &mut self,
            context: TreeContext,
            connected_inputs: &mut BTreeMap<Uuid, bool>,
        ) -> bool {
            let (input, _) = self.get_group_io_nodes();
            self.calculate_tree_cache(context);
            let inputs = &self.tree_cache.as_ref().unwrap().inputs;
            let group_input_connections: BTreeMap<_, _> = self
                .graph
                .snarl
                .wires()
                .filter(|w| w.0.node == input)
                .map(|(i, o)| (i.output, o))
                .collect();

            if inputs.len() != self.graph.inputs().len() || inputs.len() != connected_inputs.len() {
                trace!(
                    inputs_cache = inputs.len(),
                    graph_inputs = self.graph.inputs().len(),
                    connected_inputs = connected_inputs.len(),
                    "inputs count mismatch"
                );
                return false;
            }

            for ((idx, pin), graph_input) in inputs
                .iter()
                .enumerate()
                .zip_eq(self.graph.inputs.iter_mut())
            {
                let Some(uuid) = self.input_ids.get(pin) else {
                    trace!(idx, pin = ?pin, "input not found in input_ids");
                    return false;
                };
                if graph_input.id != *uuid {
                    graph_input.id = *uuid;
                    trace!(idx, pin = ?pin, uuid = ?uuid, "input id mismatch");
                    return false;
                }
                let is_connected = *connected_inputs.entry(*uuid).or_insert(false);
                if is_connected || self.graph.snarl[pin.node].has_inline_values(pin.input) {
                    let Some(connection) = group_input_connections.get(&idx) else {
                        trace!(idx, pin = ?pin, uuid = ?uuid, ?group_input_connections, "input not connected when expected to");
                        return false;
                    };

                    if connection != pin {
                        trace!(idx, pin = ?pin, uuid = ?uuid, connection = ?connection, "input connection mismatch");
                        return false;
                    }
                } else if group_input_connections.contains_key(&idx) {
                    trace!(idx, pin = ?pin, uuid = ?uuid, "input connected when expected not to");
                    return false;
                }
            }

            true
        }

        fn check_output_correctness(&mut self, context: TreeContext) -> bool {
            let (_, output) = self.get_group_io_nodes();

            let root_node = &self.graph.snarl[self.root].node;

            let count = root_node.outputs_count(context!(self.graph, context));

            let mut wires = 0;
            for (out_pin, in_pin) in self.graph.snarl.wires() {
                if out_pin.node != self.root && in_pin.node != output {
                    continue;
                }
                if out_pin.node != self.root || in_pin.node != output {
                    return false;
                }
                if out_pin.output != in_pin.input {
                    return false;
                }
                wires += 1;
            }
            self.graph.outputs.len() == wires && self.graph.outputs.len() == count
        }

        fn check_body_correctness(&mut self, _context: TreeContext) -> bool {
            let (input, output) = self.get_group_io_nodes();

            for (out_pin, in_pin) in self.graph.snarl.wires() {
                if in_pin.node == output || out_pin.node == input {
                    continue;
                }
                if out_pin.output != 0 {
                    return false;
                }

                let Some(target) = self.tree.get(&in_pin.node) else {
                    return false;
                };

                let Some(node_out_pin) = target.get(in_pin.input).and_then(|x| x.as_ref()) else {
                    return false;
                };

                if node_out_pin != &out_pin {
                    return false;
                }
            }

            true
        }

        fn get_group_io_nodes(&mut self) -> (NodeId, NodeId) {
            let snarl = &mut self.graph.snarl;

            let group_input_node = if let Some(node) = self.group_input_node {
                node
            } else {
                let group_input_node = if let Some(node) = snarl
                    .node_ids()
                    .find(|(_, node)| node.downcast_ref::<GroupInputNode>().is_some())
                    .map(|(id, _)| id)
                {
                    node
                } else {
                    self.graph.region_graph.mark_dirty();
                    let input = GroupInputNode::new();
                    snarl.insert_node(pos2(0.0, 0.0), SnarlNode::new(Box::new(input)))
                };

                self.group_input_node = Some(group_input_node);
                group_input_node
            };

            let group_output_node = if let Some(node) = self.group_output_node {
                node
            } else {
                let group_output_node = if let Some(node) = snarl
                    .node_ids()
                    .find(|(_, node)| node.downcast_ref::<GroupOutputNode>().is_some())
                    .map(|(id, _)| id)
                {
                    node
                } else {
                    self.graph.region_graph.mark_dirty();
                    let output = GroupOutputNode::new();
                    snarl.insert_node(pos2(0.0, 0.0), SnarlNode::new(Box::new(output)))
                };

                self.group_output_node = Some(group_output_node);
                group_output_node
            };

            (group_input_node, group_output_node)
        }
    }

    impl JsonSerde for TreeGraphData {
        type State<'a> = ();

        fn write_json(
            &self,
            registry: &ETypesRegistry,
            _: Self::State<'_>,
        ) -> miette::Result<JsonValue> {
            let graph = self
                .graph
                .write_json(registry)
                .context("failed to serialize `graph` field")?;
            let root = serde_json::to_value(self.root)
                .into_diagnostic()
                .context("failed to serialize `root` field")?;
            let tree = serde_json::to_value(&self.tree)
                .into_diagnostic()
                .context("failed to serialize `tree` field")?;
            let input_ids = self.input_ids.iter().collect_vec();
            let input_ids = serde_json::to_value(&input_ids)
                .into_diagnostic()
                .context("failed to serialize `input_ids` field")?;
            Ok(json!({
                "graph": graph,
                "root": root,
                "tree": tree,
                "input_ids": input_ids,
            }))
        }

        fn parse_json(
            &mut self,
            registry: &ETypesRegistry,
            _: Self::State<'_>,
            value: &mut JsonValue,
        ) -> miette::Result<()> {
            let JsonValue::Object(mut obj) = value.take() else {
                bail!("expected object");
            };
            self.graph = Graph::parse_json(
                registry,
                obj.get_mut("graph")
                    .ok_or_else(|| miette!("missing `graph` field"))?,
            )
            .context("failed to deserialize `graph` field")?;
            self.root = serde_json::from_value(
                obj.remove("root")
                    .ok_or_else(|| miette!("missing `root` field"))?,
            )
            .into_diagnostic()
            .context("failed to deserialize `root` field")?;
            self.tree = serde_json::from_value(
                obj.remove("tree")
                    .ok_or_else(|| miette!("missing `tree` field"))?,
            )
            .into_diagnostic()
            .context("failed to deserialize `tree` field")?;
            let input_ids: Vec<(InPinId, Uuid)> = serde_json::from_value(
                obj.remove("input_ids")
                    .ok_or_else(|| miette!("missing `input_ids` field"))?,
            )
            .into_diagnostic()
            .context("failed to deserialize `input_ids` field")?;
            self.input_ids = input_ids.into_iter().collect();

            self.graph.ensure_region_graph_ready();

            Ok(())
        }
    }

    impl Hash for TreeGraphData {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.graph.hash(state);
            self.root.hash(state);
            self.tree.hash(state);
            self.input_ids.hash(state);
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct TreeContext<'a> {
    pub registry: &'a ETypesRegistry,
    pub docs: &'a Docs,
    pub graphs: Option<&'a ProjectGraphs>,
}

impl<'a> From<NodeContext<'a>> for TreeContext<'a> {
    fn from(context: NodeContext<'a>) -> Self {
        Self {
            registry: context.registry,
            docs: context.docs,
            graphs: context.graphs,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TreeState {
    pub inputs: Vec<InPinId>,
    pub hierarchy: Vec<Vec<NodeId>>,
    pub start_of: Vec<Vec<NodeId>>,
    pub end_of: Vec<Vec<NodeId>>,
    pub width: usize,
}

#[derive(Debug, Copy, Clone, Hash, Default)]
struct AccessWrapper<T>(T);

impl<T> Deref for AccessWrapper<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for AccessWrapper<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // trace!("Accessing tracked data with a mutable reference");
        &mut self.0
    }
}
