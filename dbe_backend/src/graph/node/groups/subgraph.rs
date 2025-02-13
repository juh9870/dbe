use crate::etype::eitem::EItemInfo;
use crate::etype::EDataType;
use crate::graph::execution::GraphExecutionContext;
use crate::graph::node::commands::SnarlCommands;
use crate::graph::node::groups::utils::{
    get_port_input, get_port_output, map_group_inputs, map_group_outputs, sync_fields,
};
use crate::graph::node::ports::fields::IoDirection;
use crate::graph::node::ports::{InputData, OutputData};
use crate::graph::node::serde_node::impl_serde_node;
use crate::graph::node::{ExecutionExtras, ExecutionResult, Node, NodeContext, NodeFactory};
use crate::m_try;
use crate::project::project_graph::ProjectGraph;
use crate::value::EValue;
use egui_snarl::NodeId;
use miette::{bail, miette, Context};
use serde::{Deserialize, Serialize};
use ustr::Ustr;
use uuid::Uuid;

#[derive(Debug, Clone, Hash, Default, Serialize, Deserialize)]
pub struct SubgraphNode {
    pub graph_id: Uuid,
    inputs: Vec<Uuid>,
    outputs: Vec<Uuid>,

    input_types: Vec<EDataType>,
    output_types: Vec<EDataType>,
}

impl SubgraphNode {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with_graph(graph_id: Uuid) -> Self {
        Self {
            graph_id,
            ..Default::default()
        }
    }

    fn get_graph<'ctx>(&self, context: NodeContext<'ctx>) -> miette::Result<&'ctx ProjectGraph> {
        let graphs = context.graphs.ok_or_else(|| miette!("No graph context"))?;
        let graph = graphs
            .graphs
            .get(&self.graph_id)
            .ok_or_else(|| miette!("No graph context"))?;

        if !graph.is_node_group {
            bail!("Graph `{}` ({}) is not a node group", graph.name, graph.id);
        }

        Ok(graph)
    }
}

impl Node for SubgraphNode {
    impl_serde_node!();

    fn id(&self) -> Ustr {
        SubgraphNodeFactory.id()
    }

    fn title(&self, context: NodeContext) -> String {
        let Ok(graph) = self.get_graph(context) else {
            return "!!unknown graph!!".to_string();
        };
        graph.name.clone()
    }

    fn update_state(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        id: NodeId,
    ) -> miette::Result<()> {
        let Ok(graph) = self.get_graph(context) else {
            return Ok(());
        };

        sync_fields(
            commands,
            graph.inputs(),
            &mut self.inputs,
            Some(&mut self.input_types),
            id,
            IoDirection::Input(0),
        );
        sync_fields(
            commands,
            graph.outputs(),
            &mut self.outputs,
            Some(&mut self.output_types),
            id,
            IoDirection::Output(0),
        );

        Ok(())
    }

    fn inputs_count(&self, _context: NodeContext) -> usize {
        self.inputs.len()
    }

    fn input_unchecked(&self, context: NodeContext, input: usize) -> miette::Result<InputData> {
        if context.graphs.is_none() {
            return Ok(InputData::new(
                EItemInfo::simple_type(self.input_types[input]).into(),
                Default::default(),
            ));
        }
        let graph = self.get_graph(context)?;
        get_port_input(graph.inputs(), &self.inputs, input)
    }

    fn outputs_count(&self, _context: NodeContext) -> usize {
        self.outputs.len()
    }

    fn output_unchecked(&self, context: NodeContext, output: usize) -> miette::Result<OutputData> {
        if context.graphs.is_none() {
            return Ok(OutputData::new(
                EItemInfo::simple_type(self.output_types[output]).into(),
                Default::default(),
            ));
        }
        let graph = self.get_graph(context)?;
        get_port_output(graph.outputs(), &self.outputs, output)
    }

    fn has_side_effects(&self) -> bool {
        // TODO: check inner nodes
        true
    }

    fn execute(
        &self,
        context: NodeContext,
        inputs: &[EValue],
        outputs: &mut Vec<EValue>,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<ExecutionResult> {
        let graph = self.get_graph(context)?;

        if !graph.is_node_group {
            bail!("Graph `{}` ({}) is not a node group", graph.name, graph.id);
        }

        let mut graph_in = Vec::with_capacity(graph.inputs().len());
        let mut graph_out = None;

        m_try(|| {
            map_group_inputs(
                context.registry,
                graph.inputs(),
                &self.inputs,
                inputs,
                &mut graph_in,
            )?;

            let side_effects_available = variables.side_effects.is_available();
            let mut ctx = GraphExecutionContext::from_graph_and_context(
                graph.graph(),
                context,
                variables.side_effects.with_subgraph(self.graph_id),
                true,
                &graph_in,
                &mut graph_out,
            );

            ctx.full_eval(side_effects_available)?;

            drop(ctx);

            if graph.outputs().is_empty() {
                return Ok(());
            }

            let graph_out =
                graph_out.ok_or_else(|| miette!("Node group did not emit any outputs"))?;

            map_group_outputs(
                context.registry,
                graph.outputs(),
                &self.outputs,
                &graph_out,
                outputs,
            )?;
            Ok(())
        })
        .with_context(|| {
            format!(
                "failed to execute node group `{}` ({})",
                graph.name, graph.id
            )
        })?;

        Ok(ExecutionResult::Done)
    }
}

#[derive(Debug, Clone)]
pub struct SubgraphNodeFactory;

impl NodeFactory for SubgraphNodeFactory {
    fn id(&self) -> Ustr {
        "subgraph".into()
    }

    fn categories(&self) -> &'static [&'static str] {
        &["node groups"]
    }

    fn create(&self) -> Box<dyn Node> {
        Box::new(SubgraphNode::new())
    }
}
