use crate::etype::eitem::EItemInfo;
use crate::etype::EDataType;
use crate::graph::node::commands::SnarlCommands;
use crate::graph::node::editable_state::EditableState;
use crate::graph::node::extras::ExecutionExtras;
use crate::graph::node::generic::{
    generic_can_output_to, generic_connected_to_output, generic_try_connect,
    parse_generic_json_fields, write_generic_json_fields, GenericNodeField, GenericNodeFieldMut,
};
use crate::graph::node::ports::{InputData, NodePortType, OutputData};
use crate::graph::node::stateful::StatefulNode;
use crate::graph::node::{ExecutionResult, NodeContext};
use crate::json_utils::json_serde::JsonSerde;
use crate::json_utils::JsonValue;
use crate::registry::ETypesRegistry;
use crate::value::EValue;
use egui_snarl::{InPin, NodeId, OutPin};
use miette::bail;
use std::fmt::Debug;
use std::hash::Hash;
use std::ops::ControlFlow;
use ustr::Ustr;

pub trait GenericStatefulNode: 'static + Debug + Clone + Hash + Send + Sync {
    type State<'a>;
    fn id() -> Ustr;
    fn input_names(&self, external_state: &Self::State<'_>) -> &[&str];
    fn output_names(&self, external_state: &Self::State<'_>) -> &[&str];

    fn inputs(
        &self,
        registry: &ETypesRegistry,
        external_state: &Self::State<'_>,
    ) -> impl AsRef<[GenericNodeField]>;
    fn outputs(
        &self,
        registry: &ETypesRegistry,
        external_state: &Self::State<'_>,
    ) -> impl AsRef<[GenericNodeField]>;
    fn inputs_mut(
        &mut self,
        registry: &ETypesRegistry,
        external_state: &Self::State<'_>,
    ) -> impl AsMut<[GenericNodeFieldMut]>;
    fn outputs_mut(
        &mut self,
        registry: &ETypesRegistry,
        external_state: &Self::State<'_>,
    ) -> impl AsMut<[GenericNodeFieldMut]>;

    /// Writes node state to json
    fn write_json(
        &self,
        registry: &ETypesRegistry,
        external_state: Self::State<'_>,
    ) -> miette::Result<JsonValue> {
        let _ = (registry,);
        write_generic_json_fields(
            self,
            |n| n.inputs(registry, &external_state),
            |n| n.outputs(registry, &external_state),
        )
    }

    /// Loads node state from json
    fn parse_json(
        &mut self,
        registry: &ETypesRegistry,
        external_state: Self::State<'_>,
        value: &mut JsonValue,
    ) -> miette::Result<()> {
        let _ = (registry,);
        parse_generic_json_fields(value)?
            .inputs(self, |n| n.inputs_mut(registry, &external_state))?
            .outputs(self, |n| n.outputs_mut(registry, &external_state))?
            .done()?;

        Ok(())
    }

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

    /// Called after one of the node's inputs types has changed, allowing the
    /// node to update state of its pair
    fn types_changed(
        &mut self,
        context: NodeContext,
        external_state: Self::State<'_>,
        node: NodeId,
        commands: &mut SnarlCommands,
    ) {
        let _ = (context, external_state, node, commands);
    }

    fn should_execute(
        &self,
        context: NodeContext,
        external_state: Self::State<'_>,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<bool> {
        let _ = (context, external_state, variables);
        Ok(true)
    }

    fn external_state_changed(&mut self, context: NodeContext, external_state: Self::State<'_>) {
        let _ = (context, external_state);
    }

    fn has_side_effects(&self, external_state: Self::State<'_>) -> bool {
        let _ = (external_state,);
        false
    }

    fn execute(
        &self,
        context: NodeContext,
        external_state: Self::State<'_>,
        inputs: &[EValue],
        outputs: &mut Vec<EValue>,
        extras: &mut ExecutionExtras,
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

impl<T: GenericStatefulNode> JsonSerde for T {
    type State<'a> = <T as GenericStatefulNode>::State<'a>;

    fn write_json(
        &self,
        _registry: &ETypesRegistry,
        external_state: Self::State<'_>,
    ) -> miette::Result<JsonValue> {
        <T as GenericStatefulNode>::write_json(self, _registry, external_state)
    }

    fn parse_json(
        &mut self,
        _registry: &ETypesRegistry,
        external_state: Self::State<'_>,
        value: &mut JsonValue,
    ) -> miette::Result<()> {
        <T as GenericStatefulNode>::parse_json(self, _registry, external_state, value)
    }
}

impl<T: GenericStatefulNode> StatefulNode for T {
    type State<'a> = <T as GenericStatefulNode>::State<'a>;
    fn id() -> Ustr {
        T::id()
    }

    fn has_editable_state(&self, external_state: Self::State<'_>) -> bool {
        <T as GenericStatefulNode>::has_editable_state(self, external_state)
    }

    fn editable_state(&self, external_state: Self::State<'_>) -> EditableState {
        <T as GenericStatefulNode>::editable_state(self, external_state)
    }

    fn apply_editable_state(
        &mut self,
        context: NodeContext,
        external_state: Self::State<'_>,
        state: EditableState,
        commands: &mut SnarlCommands,
        node_id: NodeId,
    ) -> miette::Result<()> {
        <T as GenericStatefulNode>::apply_editable_state(
            self,
            context,
            external_state,
            state,
            commands,
            node_id,
        )
    }

    fn inputs_count(&self, context: NodeContext, external_state: Self::State<'_>) -> usize {
        self.inputs(context.registry, &external_state)
            .as_ref()
            .len()
    }

    fn outputs_count(&self, context: NodeContext, external_state: Self::State<'_>) -> usize {
        self.outputs(context.registry, &external_state)
            .as_ref()
            .len()
    }

    fn input_unchecked(
        &self,
        context: NodeContext,
        external_state: Self::State<'_>,
        input: usize,
    ) -> miette::Result<InputData> {
        let inputs = self.inputs(context.registry, &external_state);
        let Some(ty) = inputs.as_ref().get(input) else {
            bail!("Invalid input index: {}", input);
        };

        Ok(InputData::new(
            if ty.is_specific() {
                EItemInfo::simple_type(ty.ty(context.registry)).into()
            } else {
                NodePortType::BasedOnSource
            },
            self.input_names(&external_state)[input].into(),
        ))
    }

    fn output_unchecked(
        &self,
        context: NodeContext,
        external_state: Self::State<'_>,
        output: usize,
    ) -> miette::Result<OutputData> {
        let outputs = self.outputs(context.registry, &external_state);
        let Some(ty) = outputs.as_ref().get(output) else {
            bail!("Invalid output index: {}", output);
        };

        Ok(OutputData::new(
            if ty.is_specific() {
                EItemInfo::simple_type(ty.ty(context.registry)).into()
            } else {
                NodePortType::BasedOnTarget
            },
            self.output_names(&external_state)[output].into(),
        ))
    }

    fn try_connect(
        &mut self,
        context: NodeContext,
        external_state: Self::State<'_>,
        commands: &mut SnarlCommands,
        _from: &OutPin,
        to: &InPin,
        incoming_type: &NodePortType,
    ) -> miette::Result<ControlFlow<bool>> {
        let changed = match generic_try_connect(
            context,
            to.id.input,
            incoming_type,
            self.inputs_mut(context.registry, &external_state).as_mut(),
        )? {
            ControlFlow::Break(_) => return Ok(ControlFlow::Break(false)),
            ControlFlow::Continue(changed) => changed,
        };

        if changed {
            self.types_changed(context, external_state, to.id.node, commands);
        }

        Ok(ControlFlow::Continue(()))
    }

    fn can_output_to(
        &self,
        context: NodeContext,
        external_state: Self::State<'_>,
        from: &OutPin,
        _to: &InPin,
        target_type: &NodePortType,
    ) -> miette::Result<bool> {
        generic_can_output_to(
            context,
            from.id.output,
            target_type,
            self.outputs(context.registry, &external_state).as_ref(),
        )
    }

    fn connected_to_output(
        &mut self,
        context: NodeContext,
        external_state: Self::State<'_>,
        commands: &mut SnarlCommands,
        from: &OutPin,
        _to: &InPin,
        incoming_type: &NodePortType,
    ) -> miette::Result<()> {
        if generic_connected_to_output(
            context,
            from.id.output,
            incoming_type,
            self.outputs_mut(context.registry, &external_state).as_mut(),
        )? {
            self.types_changed(context, external_state, from.id.node, commands);
        }

        Ok(())
    }

    fn external_state_changed(&mut self, context: NodeContext, external_state: Self::State<'_>) {
        <T as GenericStatefulNode>::external_state_changed(self, context, external_state)
    }

    fn has_side_effects(&self, external_state: Self::State<'_>) -> bool {
        <T as GenericStatefulNode>::has_side_effects(self, external_state)
    }

    fn should_execute(
        &self,
        context: NodeContext,
        external_state: Self::State<'_>,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<bool> {
        <T as GenericStatefulNode>::should_execute(self, context, external_state, variables)
    }

    fn execute(
        &self,
        context: NodeContext,
        external_state: Self::State<'_>,
        inputs: &[EValue],
        outputs: &mut Vec<EValue>,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<ExecutionResult> {
        <T as GenericStatefulNode>::execute(
            self,
            context,
            external_state,
            inputs,
            outputs,
            variables,
        )
    }

    fn categories() -> &'static [&'static str] {
        <T as GenericStatefulNode>::categories()
    }

    fn create() -> Self {
        <T as GenericStatefulNode>::create()
    }

    fn output_port_for(ty: EDataType, registry: &ETypesRegistry) -> Option<usize> {
        <T as GenericStatefulNode>::output_port_for(ty, registry)
    }

    fn input_port_for(ty: EDataType, registry: &ETypesRegistry) -> Option<usize> {
        <T as GenericStatefulNode>::input_port_for(ty, registry)
    }
}
