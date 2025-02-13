use crate::etype::EDataType;
use crate::graph::node::commands::SnarlCommands;
use crate::graph::node::editable_state::EditableState;
use crate::graph::node::extras::ExecutionExtras;
use crate::graph::node::ports::{InputData, NodePortType, OutputData};
use crate::graph::node::{ExecutionResult, NodeContext};
use crate::registry::ETypesRegistry;
use crate::value::EValue;
use egui_snarl::{InPin, NodeId, OutPin};
use std::fmt::Debug;
use std::hash::Hash;
use std::ops::ControlFlow;
use ustr::Ustr;

pub mod generic;

pub trait StatefulNode: 'static + Debug + Clone + Hash + Send + Sync {
    type State<'a>;

    fn id() -> Ustr;

    /// See [Node::has_editable_state]
    fn has_editable_state(&self, external_state: Self::State<'_>) -> bool {
        let _ = (external_state,);
        false
    }

    /// See [Node::editable_state]
    fn editable_state(&self, external_state: Self::State<'_>) -> EditableState {
        assert!(
            self.has_editable_state(external_state),
            "editable_state should only be called if has_editable_state returns true"
        );
        unimplemented!()
    }

    /// See [Node::apply_editable_state]
    fn apply_editable_state(
        &mut self,
        _context: NodeContext,
        external_state: Self::State<'_>,
        state: EditableState,
        commands: &mut SnarlCommands,
        node_id: NodeId,
    ) -> miette::Result<()> {
        let _ = (state, commands, node_id);
        assert!(
            self.has_editable_state(external_state),
            "apply_editable_state should only be called if has_editable_state returns true"
        );
        unimplemented!()
    }

    fn inputs_count(&self, context: NodeContext, external_state: Self::State<'_>) -> usize;
    fn outputs_count(&self, context: NodeContext, external_state: Self::State<'_>) -> usize;

    fn input_unchecked(
        &self,
        context: NodeContext,
        external_state: Self::State<'_>,
        input: usize,
    ) -> miette::Result<InputData>;

    fn output_unchecked(
        &self,
        context: NodeContext,
        external_state: Self::State<'_>,
        output: usize,
    ) -> miette::Result<OutputData>;

    #[allow(clippy::too_many_arguments)]
    fn try_connect(
        &mut self,
        context: NodeContext,
        external_state: Self::State<'_>,
        commands: &mut SnarlCommands,
        from: &OutPin,
        to: &InPin,
        incoming_type: &NodePortType,
    ) -> miette::Result<ControlFlow<bool>> {
        let _ = (context, external_state, commands, from, to, incoming_type);
        Ok(ControlFlow::Continue(()))
    }

    /// Custom logic for checking if the node can output to the given port
    ///
    /// Only called if the corresponding output has type [NodePortType::BasedOnTarget]
    fn can_output_to(
        &self,
        context: NodeContext,
        external_state: Self::State<'_>,
        from: &OutPin,
        to: &InPin,
        target_type: &NodePortType,
    ) -> miette::Result<bool> {
        let _ = (context, external_state, from, to, target_type);
        unimplemented!("Node::can_output_to")
    }

    /// Custom logic to be run after the output is connected to some input
    ///
    /// Only called if the corresponding output has type [NodePortType::BasedOnTarget]
    #[allow(clippy::too_many_arguments)]
    fn connected_to_output(
        &mut self,
        context: NodeContext,
        external_state: Self::State<'_>,
        commands: &mut SnarlCommands,
        from: &OutPin,
        to: &InPin,
        incoming_type: &NodePortType,
    ) -> miette::Result<()> {
        let _ = (context, external_state, commands, from, to, incoming_type);
        unimplemented!("Node::can_output_to")
    }

    /// Called whenever the external state is changed
    fn external_state_changed(&mut self, context: NodeContext, external_state: Self::State<'_>) {
        let _ = (context, external_state);
    }

    fn has_side_effects(&self, external_state: Self::State<'_>) -> bool {
        let _ = (external_state,);
        false
    }

    /// Checks if the region should be executed at least once
    ///
    /// This is called for the endpoint node only. Start node is always executed
    fn should_execute(
        &self,
        context: NodeContext,
        external_state: Self::State<'_>,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<bool>;

    /// Executes the region io node
    ///
    /// If the region uses regional data, make sure to remove it once the
    /// region execution is finished, to avoid issues with nested looping
    /// regions
    fn execute(
        &self,
        context: NodeContext,
        external_state: Self::State<'_>,
        inputs: &[EValue],
        outputs: &mut Vec<EValue>,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<ExecutionResult>;

    fn categories() -> &'static [&'static str];
    fn create() -> Self;

    fn output_port_for(ty: EDataType, registry: &ETypesRegistry) -> Option<usize> {
        let _ = (ty, registry);
        None
    }

    fn input_port_for(ty: EDataType, registry: &ETypesRegistry) -> Option<usize> {
        let _ = (ty, registry);
        None
    }
}
