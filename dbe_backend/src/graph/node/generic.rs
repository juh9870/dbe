#![forbid(clippy::unconditional_recursion)]

use crate::etype::default::DefaultEValue;
use crate::etype::eitem::EItemInfo;
use crate::etype::EDataType;
use crate::graph::node::commands::SnarlCommands;
use crate::graph::node::editable_state::EditableState;
use crate::graph::node::extras::ExecutionExtras;
use crate::graph::node::ports::{InputData, NodePortType, OutputData};
use crate::graph::node::{ExecutionResult, Node, NodeContext};
use crate::json_utils::JsonValue;
use crate::m_try;
use crate::project::docs::{Docs, DocsWindowRef};
use crate::registry::{ETypesRegistry, OPTIONAL_ID};
use crate::value::id::ETypeId;
use crate::value::EValue;
use downcast_rs::Downcast;
use dyn_clone::DynClone;
use dyn_hash::DynHash;
use egui_snarl::{InPin, NodeId, OutPin};
use itertools::Itertools;
use miette::{bail, miette, Context, IntoDiagnostic};
use std::fmt::Debug;
use std::ops::ControlFlow;
use ustr::Ustr;

pub mod destructuring;
pub mod macros;

#[derive(Debug)]
pub enum GenericNodeField<'a> {
    /// A type is the list's element type.
    List(&'a Option<EDataType>),
    /// A type is a specific type.
    Value(&'a Option<EDataType>),
    /// An optional (nullable) type
    Option(&'a Option<EDataType>),
    /// A type is a struct id.
    Object(&'a Option<ETypeId>),
    /// A type is fixed.
    Fixed(EDataType),
}

#[derive(Debug)]
pub enum GenericNodeFieldMut<'a> {
    List(&'a mut Option<EDataType>),
    Value(&'a mut Option<EDataType>),
    Option(&'a mut Option<EDataType>),
    Object(&'a mut Option<ETypeId>),
    Fixed(EDataType),
}

impl GenericNodeFieldMut<'_> {
    pub fn as_ref(&self) -> GenericNodeField {
        match self {
            GenericNodeFieldMut::List(ty) => GenericNodeField::List(ty),
            GenericNodeFieldMut::Value(ty) => GenericNodeField::Value(ty),
            GenericNodeFieldMut::Option(ty) => GenericNodeField::Option(ty),
            GenericNodeFieldMut::Object(id) => GenericNodeField::Object(id),
            GenericNodeFieldMut::Fixed(ty) => GenericNodeField::Fixed(*ty),
        }
    }

    pub fn specify_from(
        &mut self,
        registry: &ETypesRegistry,
        incoming: &NodePortType,
    ) -> miette::Result<bool> {
        if !self.as_ref().can_specify_from(registry, incoming)? {
            return Ok(false);
        }

        match self {
            Self::List(ty) => {
                if ty.is_some() {
                    bail!("List type already set");
                }
                let EDataType::List { id } = incoming.ty() else {
                    return Ok(false);
                };

                let list = registry
                    .get_list(&id)
                    .ok_or_else(|| miette!("Unknown list type: {}", id))?;

                **ty = Some(list.value_type);
            }
            Self::Value(ty) => {
                if ty.is_some() {
                    bail!("Value type already set");
                }
                **ty = Some(incoming.ty());
            }
            GenericNodeFieldMut::Object(id) => {
                if id.is_some() {
                    bail!("Struct type already set");
                }

                let EDataType::Object { ident } = incoming.ty() else {
                    return Ok(false);
                };

                **id = Some(ident);
            }
            GenericNodeFieldMut::Option(id) => {
                if id.is_some() {
                    bail!("Option type already set");
                }

                let EDataType::Object { ident } = incoming.ty() else {
                    return Ok(false);
                };

                let Some(data) = registry.get_enum(&ident) else {
                    return Ok(false);
                };

                if data.generic_parent_id != Some(*OPTIONAL_ID) {
                    return Ok(false);
                }

                let ty = data.generic_arguments_values[0].ty();

                **id = Some(ty);
            }
            GenericNodeFieldMut::Fixed(_) => {
                bail!("Fixed type cannot be changed");
            }
        }
        Ok(true)
    }

    /// Overwrites the fields with the incoming types
    fn load_from(&mut self, incoming: Option<EDataType>) -> miette::Result<()> {
        match self {
            GenericNodeFieldMut::List(ty) => {
                **ty = incoming;
            }
            GenericNodeFieldMut::Value(ty) => {
                **ty = incoming;
            }
            GenericNodeFieldMut::Option(ty) => {
                **ty = incoming;
            }
            GenericNodeFieldMut::Object(ident) => {
                let Some(incoming) = incoming else {
                    **ident = None;
                    return Ok(());
                };
                let EDataType::Object { ident: incoming } = incoming else {
                    bail!("Expected struct type");
                };
                **ident = Some(incoming);
            }
            GenericNodeFieldMut::Fixed(_) => {}
        }
        Ok(())
    }
}

impl GenericNodeField<'_> {
    pub fn is_specific(&self) -> bool {
        match self {
            GenericNodeField::List(ty) => ty.is_some(),
            GenericNodeField::Value(ty) => ty.is_some(),
            GenericNodeField::Option(ty) => ty.is_some(),
            GenericNodeField::Object(ident) => ident.is_some(),
            GenericNodeField::Fixed(_) => true,
        }
    }

    pub fn ty(&self, registry: &ETypesRegistry) -> EDataType {
        match self {
            GenericNodeField::List(ty) => registry.list_of(ty.unwrap_or_else(EDataType::null)),
            GenericNodeField::Value(ty) => ty.unwrap_or_else(EDataType::null),
            GenericNodeField::Option(ty) => EDataType::Object {
                ident: registry.option_id_of(ty.unwrap_or_else(EDataType::null)),
            },
            GenericNodeField::Object(ident) => ident
                .map(|ident| EDataType::Object { ident })
                .unwrap_or_else(EDataType::null),
            GenericNodeField::Fixed(ty) => *ty,
        }
    }

    pub fn can_specify_from(
        &self,
        registry: &ETypesRegistry,
        incoming: &NodePortType,
    ) -> miette::Result<bool> {
        match self {
            GenericNodeField::List(ty) => {
                if ty.is_some() {
                    bail!("List type already set");
                }
                Ok(incoming.ty().is_list())
            }
            &GenericNodeField::Option(ty) => {
                if ty.is_some() {
                    bail!("Option type already set");
                }

                let EDataType::Object { ident } = incoming.ty() else {
                    return Ok(false);
                };

                let Some(data) = registry.get_enum(&ident) else {
                    return Ok(false);
                };

                Ok(data.generic_parent_id == Some(*OPTIONAL_ID))
            }
            GenericNodeField::Value(ty) => {
                if ty.is_some() {
                    bail!("Value type already set");
                }
                Ok(true)
            }
            GenericNodeField::Object(ident) => {
                if ident.is_some() {
                    bail!("Struct type already set");
                }
                Ok(incoming.ty().is_object())
            }
            GenericNodeField::Fixed(_) => {
                bail!("Fixed type cannot be changed");
            }
        }
    }

    pub fn as_input_ty(&self, context: NodeContext, name: impl Into<Ustr>) -> InputData {
        InputData::new(
            if self.is_specific() {
                EItemInfo::simple_type(self.ty(context.registry)).into()
            } else {
                NodePortType::BasedOnSource
            },
            name.into(),
        )
    }

    pub fn as_output_ty(&self, context: NodeContext, name: impl Into<Ustr>) -> OutputData {
        OutputData::new(
            if self.is_specific() {
                EItemInfo::simple_type(self.ty(context.registry)).into()
            } else {
                NodePortType::BasedOnTarget
            },
            name.into(),
        )
    }

    fn save_type(&self) -> Option<EDataType> {
        match self {
            GenericNodeField::List(ty) => **ty,
            GenericNodeField::Value(ty) => **ty,
            GenericNodeField::Option(ty) => **ty,
            GenericNodeField::Fixed(ty) => Some(*ty),
            GenericNodeField::Object(ident) => ident.map(|ident| EDataType::Object { ident }),
        }
    }
}

pub trait GenericNode: DynClone + DynHash + Debug + Send + Sync + Downcast + 'static {
    fn write_json(&self, registry: &ETypesRegistry) -> miette::Result<JsonValue> {
        let _ = (registry,);
        write_generic_json_fields(self, |n| n.inputs(registry), |n| n.outputs(registry))
    }

    fn parse_json(
        &mut self,
        registry: &ETypesRegistry,
        value: &mut JsonValue,
    ) -> miette::Result<()> {
        let _ = (registry,);
        parse_generic_json_fields(value)?
            .inputs(self, |n| n.inputs_mut(registry))?
            .outputs(self, |n| n.outputs_mut(registry))?
            .done()?;
        Ok(())
    }

    fn id(&self) -> Ustr;
    fn input_names(&self) -> &[&str];
    fn output_names(&self) -> &[&str];

    fn default_input_value(
        &self,
        context: NodeContext,
        input: usize,
    ) -> miette::Result<DefaultEValue> {
        let inputs = self.inputs(context.registry);
        let input = inputs
            .as_ref()
            .get(input)
            .ok_or_else(|| miette!("Invalid input index: {}", input))?;
        Ok(input.ty(context.registry).default_value(context.registry))
    }

    fn inputs(&self, registry: &ETypesRegistry) -> impl AsRef<[GenericNodeField]>;
    fn outputs(&self, registry: &ETypesRegistry) -> impl AsRef<[GenericNodeField]>;
    fn inputs_mut(&mut self, registry: &ETypesRegistry) -> impl AsMut<[GenericNodeFieldMut]>;
    fn outputs_mut(&mut self, registry: &ETypesRegistry) -> impl AsMut<[GenericNodeFieldMut]>;

    /// See [Node::title]
    fn title(&self, context: NodeContext, docs: &Docs) -> String {
        let _ = (context, docs);
        DocsWindowRef::Node(self.id())
            .title(docs, context.registry)
            .to_string()
    }

    /// Called after one of the node's inputs/output types has changed,
    /// allowing the node to update its state
    fn types_changed(&mut self, context: NodeContext, node: NodeId, commands: &mut SnarlCommands) {
        let _ = (context, node, commands);
    }

    /// See [Node::update_state]
    fn update_state(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        id: NodeId,
    ) -> miette::Result<()> {
        let _ = (context, commands, id);
        Ok(())
    }

    /// See [Node::has_editable_state]
    fn has_editable_state(&self) -> bool {
        false
    }

    /// See [Node::editable_state]
    fn editable_state(&self) -> EditableState {
        assert!(
            self.has_editable_state(),
            "editable_state should only be called if has_editable_state returns true"
        );
        unimplemented!()
    }

    /// See [Node::apply_editable_state]
    fn apply_editable_state(
        &mut self,
        state: EditableState,
        commands: &mut SnarlCommands,
        node_id: NodeId,
    ) -> miette::Result<()> {
        let _ = (state, commands, node_id);
        assert!(
            self.has_editable_state(),
            "apply_editable_state should only be called if has_editable_state returns true"
        );
        unimplemented!()
    }

    /// See [Node::has_inline_values]
    fn has_inline_values(&self, input: usize) -> bool {
        let _ = (input,);
        true
    }

    /// See [Node::has_side_effects]
    fn has_side_effects(&self) -> bool {
        false
    }

    /// See [Node::should_execute_dependencies]
    fn should_execute_dependencies(
        &self,
        context: NodeContext,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<bool> {
        let _ = (context, variables);
        Ok(true)
    }

    /// See [Node::execute]
    fn execute(
        &self,
        context: NodeContext,
        inputs: &[EValue],
        outputs: &mut Vec<EValue>,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<ExecutionResult>;
}

pub fn write_generic_json_fields<
    'a,
    T: 'a + ?Sized,
    Inputs: FnOnce(&'a T) -> InputData,
    InputData: AsRef<[GenericNodeField<'a>]>,
    Outputs: FnOnce(&'a T) -> OutputData,
    OutputData: AsRef<[GenericNodeField<'a>]>,
>(
    node: &'a T,
    inputs: Inputs,
    outputs: Outputs,
) -> miette::Result<JsonValue> {
    let inputs = inputs(node)
        .as_ref()
        .iter()
        .map(|ty| ty.save_type())
        .collect_vec();
    let outputs = outputs(node)
        .as_ref()
        .iter()
        .map(|ty| ty.save_type())
        .collect_vec();

    serde_json::to_value((inputs, outputs)).into_diagnostic()
}

#[allow(clippy::type_complexity)]
pub fn parse_generic_json_fields(
    value: &mut JsonValue,
) -> miette::Result<ParsedGeneric<Vec<Option<EDataType>>, Vec<Option<EDataType>>>> {
    let (inputs, outputs) = if value.is_null() {
        Default::default()
    } else {
        serde_json::from_value(value.take()).into_diagnostic()?
    };
    Ok(ParsedGeneric { inputs, outputs })
}

#[must_use]
#[derive(Debug)]
pub struct ParsedGeneric<IN, OUT> {
    inputs: IN,
    outputs: OUT,
}

impl<IN> ParsedGeneric<IN, Vec<Option<EDataType>>> {
    pub(crate) fn outputs<
        'a,
        T: ?Sized,
        R: AsMut<[GenericNodeFieldMut<'a>]>,
        F: FnOnce(&'a mut T) -> R,
    >(
        self,
        node: &'a mut T,
        outputs: F,
    ) -> miette::Result<ParsedGeneric<IN, ()>> {
        for (ty, field) in self
            .outputs
            .into_iter()
            .zip(outputs(node).as_mut().iter_mut())
        {
            field.load_from(ty)?
        }
        Ok(ParsedGeneric {
            inputs: self.inputs,
            outputs: (),
        })
    }
}

impl<OUT> ParsedGeneric<Vec<Option<EDataType>>, OUT> {
    pub fn inputs<'a, T: ?Sized, R: AsMut<[GenericNodeFieldMut<'a>]>, F: FnOnce(&'a mut T) -> R>(
        self,
        node: &'a mut T,
        inputs: F,
    ) -> miette::Result<ParsedGeneric<(), OUT>> {
        for (ty, field) in self
            .inputs
            .into_iter()
            .zip(inputs(node).as_mut().iter_mut())
        {
            field.load_from(ty)?
        }
        Ok(ParsedGeneric {
            inputs: (),
            outputs: self.outputs,
        })
    }
}

impl ParsedGeneric<(), ()> {
    pub fn done(self) -> miette::Result<()> {
        Ok(())
    }
}

impl<T: GenericNode> Node for T {
    fn write_json(&self, registry: &ETypesRegistry) -> miette::Result<JsonValue> {
        T::write_json(self, registry)
    }

    fn parse_json(
        &mut self,
        registry: &ETypesRegistry,
        value: &mut JsonValue,
    ) -> miette::Result<()> {
        T::parse_json(self, registry, value)
    }

    fn id(&self) -> Ustr {
        T::id(self)
    }

    fn default_input_value(
        &self,
        context: NodeContext,
        input: usize,
    ) -> miette::Result<DefaultEValue> {
        T::default_input_value(self, context, input)
    }

    fn update_state(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        id: NodeId,
    ) -> miette::Result<()> {
        T::update_state(self, context, commands, id)
    }

    fn has_editable_state(&self) -> bool {
        T::has_editable_state(self)
    }

    fn editable_state(&self) -> EditableState {
        T::editable_state(self)
    }

    fn apply_editable_state(
        &mut self,
        _context: NodeContext,
        state: EditableState,
        commands: &mut SnarlCommands,
        node_id: NodeId,
    ) -> miette::Result<()> {
        T::apply_editable_state(self, state, commands, node_id)
    }

    fn has_inline_values(&self, input: usize) -> bool {
        T::has_inline_values(self, input)
    }

    fn inputs_count(&self, context: NodeContext) -> usize {
        self.inputs(context.registry).as_ref().len()
    }

    fn input_unchecked(&self, context: NodeContext, input: usize) -> miette::Result<InputData> {
        let inputs = self.inputs(context.registry);
        let Some(ty) = inputs.as_ref().get(input) else {
            bail!("Invalid input index: {}", input);
        };

        Ok(ty.as_input_ty(context, self.input_names()[input]))
    }

    fn outputs_count(&self, context: NodeContext) -> usize {
        self.outputs(context.registry).as_ref().len()
    }

    fn output_unchecked(&self, context: NodeContext, output: usize) -> miette::Result<OutputData> {
        let outputs = self.outputs(context.registry);
        let Some(ty) = outputs.as_ref().get(output) else {
            bail!("Invalid output index: {}", output);
        };

        Ok(ty.as_output_ty(context, self.output_names()[output]))
    }

    fn try_connect(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        from: &OutPin,
        to: &InPin,
        incoming_type: &NodePortType,
    ) -> miette::Result<bool> {
        let changed = match generic_try_connect(
            context,
            to.id.input,
            incoming_type,
            self.inputs_mut(context.registry).as_mut(),
        )? {
            ControlFlow::Break(_) => return Ok(false),
            ControlFlow::Continue(changed) => changed,
        };

        if changed {
            self.types_changed(context, to.id.node, commands);
        }

        self._default_try_connect(context, commands, from, to, incoming_type)
    }

    fn can_output_to(
        &self,
        context: NodeContext,
        from: &OutPin,
        _to: &InPin,
        target_type: &NodePortType,
    ) -> miette::Result<bool> {
        generic_can_output_to(
            context,
            from.id.output,
            target_type,
            self.outputs(context.registry).as_ref(),
        )
    }

    fn connected_to_output(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        from: &OutPin,
        _to: &InPin,
        incoming_type: &NodePortType,
    ) -> miette::Result<()> {
        if generic_connected_to_output(
            context,
            from.id.output,
            incoming_type,
            self.outputs_mut(context.registry).as_mut(),
        )? {
            self.types_changed(context, from.id.node, commands);
        }

        Ok(())
    }

    fn has_side_effects(&self) -> bool {
        T::has_side_effects(self)
    }

    fn should_execute_dependencies(
        &self,
        context: NodeContext,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<bool> {
        T::should_execute_dependencies(self, context, variables)
    }

    fn execute(
        &self,
        context: NodeContext,
        inputs: &[EValue],
        outputs: &mut Vec<EValue>,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<ExecutionResult> {
        T::execute(self, context, inputs, outputs, variables)
    }
}

/// Performs the generic part of the connection logic.
///
/// Returns [ControlFlow::Break] if the connection logic should finish early with a
/// specific result.
///
/// Returns [ControlFlow::Continue] if the connection logic should continue.
///
/// The payload of [ControlFlow::Continue] is a boolean indicating whether the
/// port type was changed.
pub fn generic_try_connect(
    context: NodeContext,
    input: usize,
    incoming_type: &NodePortType,
    inputs: &mut [GenericNodeFieldMut],
) -> miette::Result<ControlFlow<(), bool>> {
    let Some(ty) = inputs.get_mut(input) else {
        bail!("Invalid input index: {}", input);
    };

    if ty.as_ref().is_specific() {
        return Ok(ControlFlow::Continue(false));
    }

    if !ty
        .specify_from(context.registry, incoming_type)
        .context("Failed to process generic connection to output")?
    {
        return Ok(ControlFlow::Break(()));
    }
    Ok(ControlFlow::Continue(true))
}

pub fn generic_can_output_to(
    context: NodeContext,
    output: usize,
    target_type: &NodePortType,
    outputs: &[GenericNodeField],
) -> miette::Result<bool> {
    let Some(ty) = outputs.as_ref().get(output) else {
        bail!("Invalid output index: {}", output);
    };

    ty.can_specify_from(context.registry, target_type)
}

/// Performs the generic part of the reverse connection logic.
///
/// Returns [Ok(true)] if the port type was changed.
pub fn generic_connected_to_output(
    context: NodeContext,
    output: usize,
    incoming_type: &NodePortType,
    outputs: &mut [GenericNodeFieldMut],
) -> miette::Result<bool> {
    m_try(|| {
        let Some(ty) = outputs.as_mut().get_mut(output) else {
            bail!("Invalid output index: {}", output);
        };

        if !ty.specify_from(context.registry, incoming_type)? {
            bail!("Failed to specify type");
        }
        Ok(())
    })
    .context("Failed to process generic connection from output")?;

    Ok(true)
}
