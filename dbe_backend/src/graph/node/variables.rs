use crate::etype::EDataType;
use crate::graph::inputs::GraphIoData;
use crate::graph::node::commands::SnarlCommands;
use crate::graph::node::editable_state::EditableState;
use crate::graph::node::extras::ExecutionExtras;
use crate::graph::node::generic::{
    generic_can_output_to, generic_connected_to_output, generic_try_connect, GenericNodeField,
    GenericNodeFieldMut,
};
use crate::graph::node::groups::utils::{
    get_graph_io_field, get_graph_io_field_index, sync_fields,
};
use crate::graph::node::ports::fields::IoDirection;
use crate::graph::node::ports::{InputData, NodePortType, OutputData};
use crate::graph::node::stateful::StatefulNode;
use crate::graph::node::{ExecutionResult, Node, NodeContext, NodeFactory};
use crate::json_utils::json_serde::JsonSerde;
use crate::json_utils::JsonValue;
use crate::registry::ETypesRegistry;
use crate::value::EValue;
use bitflags::bitflags;
use egui_snarl::{InPin, NodeId, OutPin};
use miette::{miette, IntoDiagnostic};
use serde::{Deserialize, Serialize};
use serde_json::json;
use smallvec::SmallVec;
use std::fmt::Debug;
use std::hash::Hash;
use std::marker::PhantomData;
use std::ops::{ControlFlow, Deref};
use ustr::Ustr;
use uuid::Uuid;

pub mod coalesce;

bitflags! {
    pub struct VariableSide: u8 {
        const IN  = 0b0001;
        const OUT = 0b0010;
    }
}

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct NodeVariable {
    pub ty: Option<EDataType>,
    pub id: Uuid,
    pub name: String,
}

impl GraphIoData for NodeVariable {
    fn id(&self) -> &Uuid {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn name_mut(&mut self) -> &mut String {
        &mut self.name
    }

    fn ty(&self) -> Option<EDataType> {
        self.ty
    }
}

type VariablesStore = SmallVec<[NodeVariable; 2]>;

#[derive(Debug)]
pub enum VariablesState<'a> {
    Im(&'a [NodeVariable]),
    Mut(&'a mut VariablesStore),
}

impl<'a> VariablesState<'a> {
    pub fn as_mut(&mut self) -> Option<&mut VariablesStore> {
        match self {
            VariablesState::Im(_) => None,
            VariablesState::Mut(v) => Some(v),
        }
    }
}

impl<'a> Deref for VariablesState<'a> {
    type Target = [NodeVariable];

    fn deref(&self) -> &Self::Target {
        match self {
            VariablesState::Im(v) => v,
            VariablesState::Mut(v) => v,
        }
    }
}

#[derive(Debug, Clone, Hash)]
pub struct VariablesNode<T: VariablesTrait> {
    node: T,
    ids: SmallVec<[Uuid; 2]>,
    variables: VariablesStore,
}

impl<T: VariablesTrait> VariablesNode<T> {
    fn get_variable(&self, index: usize) -> Option<(&NodeVariable, usize)> {
        let idx = get_graph_io_field_index(&self.variables, &self.ids, index)?;
        let variable = self.variables.get(idx)?;

        Some((variable, idx))
    }

    fn allow_input_variables(&self) -> bool {
        T::allow_variables(VariablesState::Im(&self.variables)).contains(VariableSide::IN)
    }

    fn allow_output_variables(&self) -> bool {
        T::allow_variables(VariablesState::Im(&self.variables)).contains(VariableSide::OUT)
    }

    fn input_variables_length(&self) -> usize {
        if self.allow_input_variables() {
            self.ids.len()
        } else {
            0
        }
    }

    fn output_variables_length(&self) -> usize {
        if self.allow_output_variables() {
            self.ids.len()
        } else {
            0
        }
    }
}

impl<T: VariablesTrait> Node for VariablesNode<T> {
    fn write_json(&self, registry: &ETypesRegistry) -> miette::Result<JsonValue> {
        let node = self
            .node
            .write_json(registry, VariablesState::Im(&self.variables))?;

        Ok(json!({
            "node": node,
            "ids": self.ids.clone(),
            "variables": self.variables.clone(),
        }))
    }

    fn parse_json(
        &mut self,
        registry: &ETypesRegistry,
        value: &mut JsonValue,
    ) -> miette::Result<()> {
        #[derive(Debug, Serialize, Deserialize)]
        struct Packed {
            node: JsonValue,
            ids: SmallVec<[Uuid; 2]>,
            variables: VariablesStore,
        }

        let mut packed: Packed = serde_json::from_value(value.take()).into_diagnostic()?;
        self.ids = packed.ids;
        self.variables = packed.variables;
        self.node.parse_json(
            registry,
            VariablesState::Mut(&mut self.variables),
            &mut packed.node,
        )?;

        Ok(())
    }

    fn id(&self) -> Ustr {
        T::id()
    }

    fn update_state(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        id: NodeId,
    ) -> miette::Result<()> {
        let allow_in = self.allow_input_variables();
        let allow_out = self.allow_output_variables();
        if !allow_in && !allow_out {
            return Ok(());
        }

        sync_fields(
            commands,
            &self.variables,
            &mut self.ids,
            None,
            id,
            if allow_in && allow_out {
                IoDirection::Both {
                    input_offset: self
                        .node
                        .inputs_count(context, VariablesState::Im(&self.variables)),
                    output_offset: self
                        .node
                        .outputs_count(context, VariablesState::Im(&self.variables)),
                }
            } else if allow_in {
                IoDirection::Input(
                    self.node
                        .inputs_count(context, VariablesState::Im(&self.variables)),
                )
            } else {
                IoDirection::Output(
                    self.node
                        .outputs_count(context, VariablesState::Im(&self.variables)),
                )
            },
        );

        Ok(())
    }

    fn has_editable_state(&self) -> bool {
        self.node
            .has_editable_state(VariablesState::Im(&self.variables))
    }

    fn editable_state(&self) -> EditableState {
        self.node
            .editable_state(VariablesState::Im(&self.variables))
    }

    fn apply_editable_state(
        &mut self,
        context: NodeContext,
        state: EditableState,
        commands: &mut SnarlCommands,
        node_id: NodeId,
    ) -> miette::Result<()> {
        self.node.apply_editable_state(
            context,
            VariablesState::Mut(&mut self.variables),
            state,
            commands,
            node_id,
        )
    }

    fn inputs_count(&self, context: NodeContext) -> usize {
        self.input_variables_length()
            + self
                .node
                .inputs_count(context, VariablesState::Im(&self.variables))
            + if self.allow_input_variables() { 1 } else { 0 }
    }

    fn input_unchecked(&self, context: NodeContext, input: usize) -> miette::Result<InputData> {
        let native_in_count = self
            .node
            .inputs_count(context, VariablesState::Im(&self.variables));
        if input
            < self
                .node
                .inputs_count(context, VariablesState::Im(&self.variables))
        {
            return self
                .node
                .input_unchecked(context, VariablesState::Im(&self.variables), input);
        }

        if !self.allow_input_variables() {
            return Ok(InputData::invalid("unknown input"));
        }

        if input == self.ids.len() + native_in_count {
            // special "new" input
            Ok(self
                .node
                .new_variable_port(context, VariablesState::Im(&self.variables)))
        } else {
            let Some(field) =
                get_graph_io_field(&self.variables, &self.ids, input - native_in_count)
            else {
                return Ok(InputData::invalid("unknown input"));
            };
            Ok(self
                .node
                .input_variable_type(context, VariablesState::Im(&self.variables), &field.ty)
                .as_input_ty(context, field.name.clone()))
        }
    }

    fn outputs_count(&self, context: NodeContext) -> usize {
        self.output_variables_length()
            + self
                .node
                .outputs_count(context, VariablesState::Im(&self.variables))
    }

    fn output_unchecked(&self, context: NodeContext, output: usize) -> miette::Result<OutputData> {
        let native_out_count = self
            .node
            .outputs_count(context, VariablesState::Im(&self.variables));
        if output < native_out_count {
            return self.node.output_unchecked(
                context,
                VariablesState::Im(&self.variables),
                output,
            );
        }

        if !self.allow_output_variables() {
            return Ok(OutputData::invalid("!!unknown input!!"));
        }

        let Some(field) = get_graph_io_field(&self.variables, &self.ids, output - native_out_count)
        else {
            return Ok(OutputData::invalid("unknown input"));
        };
        Ok(self
            .node
            .output_variable_type(context, VariablesState::Im(&self.variables), &field.ty)
            .as_output_ty(context, field.name.clone()))
    }

    fn try_connect(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        from: &OutPin,
        to: &InPin,
        incoming_type: &NodePortType,
    ) -> miette::Result<bool> {
        if to.id.input
            < self
                .node
                .inputs_count(context, VariablesState::Im(&self.variables))
        {
            if let ControlFlow::Break(value) = self.node.try_connect(
                context,
                VariablesState::Mut(&mut self.variables),
                commands,
                from,
                to,
                incoming_type,
            )? {
                return Ok(value);
            };
        } else if self.allow_input_variables() {
            if to.id.input == self.inputs_count(context) - 1 {
                let mut ty = None;
                let mut inputs = [self.node.input_variable_type_mut(
                    context,
                    VariablesState::Im(&self.variables),
                    &mut ty,
                )];
                let specific = inputs[0].as_ref().is_specific();
                match generic_try_connect(context, 0, incoming_type, inputs.as_mut_slice())? {
                    ControlFlow::Continue(_) => {}
                    ControlFlow::Break(_) => return Ok(false),
                };
                let Some(ty) = ty else {
                    if specific {
                        panic!("if specific type was created, it must also change the input type");
                    } else {
                        panic!("type should be present after generic_try_connect on a non-specific type");
                    }
                };
                let new_pin_id = Uuid::new_v4();
                self.variables.push(NodeVariable {
                    ty: Some(ty),
                    id: new_pin_id,
                    name: "value".to_string(),
                });
                self.node
                    .external_state_changed(context, VariablesState::Mut(&mut self.variables));
            } else {
                let input = to.id.input
                    - self
                        .node
                        .inputs_count(context, VariablesState::Im(&self.variables));

                let (field, field_index) = self
                    .get_variable(input)
                    .ok_or_else(|| miette!("Variable {} is missing", input))?;

                if field.ty.is_none() {
                    let mut ty = field.ty;
                    let mut inputs = [self.node.input_variable_type_mut(
                        context,
                        VariablesState::Im(&self.variables),
                        &mut ty,
                    )];
                    let changed = match generic_try_connect(
                        context,
                        0,
                        incoming_type,
                        inputs.as_mut_slice(),
                    )? {
                        ControlFlow::Continue(changed) => changed,
                        ControlFlow::Break(_) => return Ok(false),
                    };

                    if let Some(ty) = ty {
                        self.variables[field_index].ty = Some(ty);
                        self.node.external_state_changed(
                            context,
                            VariablesState::Mut(&mut self.variables),
                        );
                    } else {
                        assert!(
                            !changed,
                            "ty should be Some if generic_try_connect returned true"
                        );
                    }
                }
            }
        }

        self._default_try_connect(context, commands, from, to, incoming_type)
    }

    fn can_output_to(
        &self,
        context: NodeContext,
        from: &OutPin,
        to: &InPin,
        target_type: &NodePortType,
    ) -> miette::Result<bool> {
        if from.id.output
            < self
                .node
                .outputs_count(context, VariablesState::Im(&self.variables))
        {
            return self.node.can_output_to(
                context,
                VariablesState::Im(&self.variables),
                from,
                to,
                target_type,
            );
        }

        if !self.allow_output_variables() {
            return Ok(false);
        }

        let Some((field, _)) = self.get_variable(
            from.id.output
                - self
                    .node
                    .outputs_count(context, VariablesState::Im(&self.variables)),
        ) else {
            return Ok(false);
        };

        let ty = field.ty();

        let outputs =
            [self
                .node
                .output_variable_type(context, VariablesState::Im(&self.variables), &ty)];
        generic_can_output_to(context, 0, target_type, &outputs)
    }

    fn connected_to_output(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        from: &OutPin,
        to: &InPin,
        incoming_type: &NodePortType,
    ) -> miette::Result<()> {
        // do NOT sync fields in this method, rearrangement of the fields might cause issues with pending connection commands

        if from.id.output
            < self
                .node
                .outputs_count(context, VariablesState::Im(&self.variables))
        {
            return self.node.connected_to_output(
                context,
                VariablesState::Mut(&mut self.variables),
                commands,
                from,
                to,
                incoming_type,
            );
        }

        let output = from.id.output
            - self
                .node
                .outputs_count(context, VariablesState::Im(&self.variables));

        let (field, field_index) = self
            .get_variable(output)
            .expect("variable should exist, because `can_output_to` succeeded");

        let mut ty = field.ty();
        let mut outputs = [self.node.output_variable_type_mut(
            context,
            VariablesState::Im(&self.variables),
            &mut ty,
        )];
        let changed =
            generic_connected_to_output(context, 0, incoming_type, outputs.as_mut_slice())?;

        if let Some(ty) = ty {
            self.variables[field_index].ty = Some(ty);
            self.node
                .external_state_changed(context, VariablesState::Mut(&mut self.variables));
            Ok(())
        } else {
            assert!(
                !changed,
                "ty should be Some if generic_connected_to_output returned true"
            );
            Ok(())
        }
    }

    fn region_source(&self) -> Option<Uuid> {
        None
    }

    fn region_end(&self) -> Option<Uuid> {
        None
    }

    fn has_side_effects(&self) -> bool {
        self.node
            .has_side_effects(VariablesState::Im(&self.variables))
    }

    fn execute(
        &self,
        context: NodeContext,
        inputs: &[EValue],
        outputs: &mut Vec<EValue>,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<ExecutionResult> {
        self.node.execute(
            context,
            VariablesState::Im(&self.variables),
            inputs,
            outputs,
            variables,
        )
    }
}

#[derive(Debug)]
pub struct VariablesNodeFactory<T: VariablesTrait>(PhantomData<T>);

impl<T: VariablesTrait> VariablesNodeFactory<T> {
    pub const INSTANCE: Self = Self(PhantomData);
}

impl<T: VariablesTrait> NodeFactory for VariablesNodeFactory<T> {
    fn id(&self) -> Ustr {
        T::id()
    }

    fn categories(&self) -> &'static [&'static str] {
        T::categories()
    }

    fn create(&self) -> Box<dyn Node> {
        Box::new(VariablesNode {
            node: T::create(),
            ids: Default::default(),
            variables: Default::default(),
        })
    }

    fn output_port_for(&self, ty: EDataType, registry: &ETypesRegistry) -> Option<usize> {
        T::output_port_for(ty, registry)
    }

    fn input_port_for(&self, ty: EDataType, registry: &ETypesRegistry) -> Option<usize> {
        T::input_port_for(ty, registry)
    }
}

pub trait VariablesTrait:
    for<'a> StatefulNode<State<'a> = VariablesState<'a>>
    + for<'a> JsonSerde<State<'a> = VariablesState<'a>>
    + for<'a> NodeWithVariables<State<'a> = VariablesState<'a>>
{
}

impl<
        T: for<'a> StatefulNode<State<'a> = VariablesState<'a>>
            + for<'a> JsonSerde<State<'a> = VariablesState<'a>>
            + for<'a> NodeWithVariables<State<'a> = VariablesState<'a>>,
    > VariablesTrait for T
{
}

pub trait NodeWithVariables {
    type State<'a>;
    /// Indicates at which sides the node can have variables
    fn allow_variables(external_state: Self::State<'_>) -> VariableSide {
        let _ = (external_state,);
        VariableSide::all()
    }

    fn input_variable_type<'a>(
        &self,
        context: NodeContext,
        external_state: Self::State<'_>,
        ty: &'a Option<EDataType>,
    ) -> GenericNodeField<'a> {
        let _ = (context, external_state);
        GenericNodeField::Value(ty)
    }

    fn output_variable_type<'a>(
        &self,
        context: NodeContext,
        external_state: Self::State<'_>,
        ty: &'a Option<EDataType>,
    ) -> GenericNodeField<'a> {
        let _ = (context, external_state);
        GenericNodeField::Value(ty)
    }

    fn input_variable_type_mut<'a>(
        &self,
        context: NodeContext,
        external_state: Self::State<'_>,
        ty: &'a mut Option<EDataType>,
    ) -> GenericNodeFieldMut<'a> {
        let _ = (context, external_state);
        GenericNodeFieldMut::Value(ty)
    }

    fn output_variable_type_mut<'a>(
        &self,
        context: NodeContext,
        external_state: Self::State<'_>,
        ty: &'a mut Option<EDataType>,
    ) -> GenericNodeFieldMut<'a> {
        let _ = (context, external_state);
        GenericNodeFieldMut::Value(ty)
    }

    fn new_variable_port(
        &self,
        context: NodeContext,
        external_state: Self::State<'_>,
    ) -> InputData {
        let _ = (context, external_state);
        InputData::new(NodePortType::BasedOnSource, "".into())
    }
}

pub fn remember_variables(
    state_values: &mut Option<Vec<EValue>>,
    inputs: &[EValue],
    outputs: &mut Vec<EValue>,
) {
    if let Some(values) = state_values.take() {
        outputs.extend(values);
    } else {
        outputs.extend(inputs.iter().cloned());
    }
}

/// Syncs all variables in the state to the same type
pub fn sync_variable_types(master_type: &mut Option<EDataType>, mut state: VariablesState) {
    let variables = state
        .as_mut()
        .expect("sync_variable_types should only be called in the mutable state context");
    let ty = if let Some(ty) = master_type {
        *ty
    } else {
        let Some(ty) = variables.iter().filter_map(|x| x.ty).next() else {
            return;
        };
        *master_type = Some(ty);
        ty
    };

    for var in variables {
        var.ty = Some(ty);
    }
}
