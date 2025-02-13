use crate::etype::EDataType;
use crate::graph::node::commands::SnarlCommands;
use crate::graph::node::extras::ExecutionExtras;
use crate::graph::node::generic::macros::generic_node_io;
use crate::graph::node::generic::{GenericNodeField, GenericNodeFieldMut};
use crate::graph::node::stateful::generic::GenericStatefulNode;
use crate::graph::node::variables::{
    sync_variable_types, NodeWithVariables, VariableSide, VariablesState,
};
use crate::graph::node::{ExecutionResult, NodeContext};
use crate::registry::optional_helpers::{is_type_option, unwrap_optional_value, wrap_in_option};
use crate::registry::ETypesRegistry;
use crate::value::EValue;
use egui_snarl::NodeId;
use miette::Context;
use ustr::Ustr;

pub type CoalesceNode = Coalesce<false>;
pub type CoalesceOrDefaultNode = Coalesce<true>;

#[derive(Debug, Clone, Hash)]
pub struct Coalesce<const DEFAULT: bool> {
    output_ty: Option<EDataType>,
}

impl<const DEFAULT: bool> NodeWithVariables for Coalesce<DEFAULT> {
    type State<'a> = VariablesState<'a>;

    fn allow_variables(_external_state: Self::State<'_>) -> VariableSide {
        VariableSide::IN
    }

    fn input_variable_type<'a>(
        &self,
        context: NodeContext,
        _external_state: Self::State<'_>,
        ty: &'a Option<EDataType>,
    ) -> GenericNodeField<'a> {
        if let Some(ty) = self.output_ty {
            GenericNodeField::Fixed(EDataType::Object {
                ident: context.registry.option_id_of(ty),
            })
        } else {
            GenericNodeField::Option(ty)
        }
    }

    fn input_variable_type_mut<'a>(
        &self,
        _context: NodeContext,
        _external_state: Self::State<'_>,
        ty: &'a mut Option<EDataType>,
    ) -> GenericNodeFieldMut<'a> {
        if let Some(output_ty) = self.output_ty {
            *ty = Some(output_ty);
        }

        GenericNodeFieldMut::Option(ty)
    }
}

impl<const DEFAULT: bool> GenericStatefulNode for Coalesce<DEFAULT> {
    type State<'a> = VariablesState<'a>;

    fn id() -> Ustr {
        if DEFAULT {
            "coalesce_or_default".into()
        } else {
            "coalesce".into()
        }
    }

    fn input_names(&self, _external_state: &Self::State<'_>) -> &[&str] {
        &[]
    }

    fn output_names(&self, _external_state: &Self::State<'_>) -> &[&str] {
        &["value"]
    }

    generic_node_io! {
        inputs(&Self::State<'_>) {
            []
        }
    }

    fn outputs(
        &self,
        _registry: &ETypesRegistry,
        _external_state: &Self::State<'_>,
    ) -> impl AsRef<[GenericNodeField]> {
        if DEFAULT {
            [GenericNodeField::Value(&self.output_ty)]
        } else {
            [GenericNodeField::Option(&self.output_ty)]
        }
    }
    fn outputs_mut(
        &mut self,
        _registry: &ETypesRegistry,
        _external_state: &Self::State<'_>,
    ) -> impl AsMut<[GenericNodeFieldMut]> {
        if DEFAULT {
            [GenericNodeFieldMut::Value(&mut self.output_ty)]
        } else {
            [GenericNodeFieldMut::Option(&mut self.output_ty)]
        }
    }

    fn types_changed(
        &mut self,
        _context: NodeContext,
        external_state: Self::State<'_>,
        _node: NodeId,
        _commands: &mut SnarlCommands,
    ) {
        sync_variable_types(&mut self.output_ty, external_state);
    }

    fn external_state_changed(&mut self, _context: NodeContext, external_state: Self::State<'_>) {
        sync_variable_types(&mut self.output_ty, external_state);
    }

    fn execute(
        &self,
        context: NodeContext,
        variables: Self::State<'_>,
        inputs: &[EValue],
        outputs: &mut Vec<EValue>,
        _extras: &mut ExecutionExtras,
    ) -> miette::Result<ExecutionResult> {
        let ty = self.output_ty.unwrap_or_else(EDataType::null);
        let value = inputs
            .iter()
            .take(variables.len())
            .enumerate()
            .filter_map(|(idx, value)| {
                unwrap_optional_value(context.registry, value)
                    .with_context(|| format!("failed to read input #{}", idx))
                    .transpose()
            })
            .next()
            .transpose()?;

        let value = if DEFAULT {
            if let Some(value) = value {
                value.clone()
            } else {
                ty.default_value(context.registry).into_owned()
            }
        } else {
            wrap_in_option(context.registry, ty, value.cloned())
        };

        outputs.push(value);

        Ok(ExecutionResult::Done)
    }

    fn categories() -> &'static [&'static str] {
        &["optional"]
    }

    fn create() -> Self {
        Self { output_ty: None }
    }

    fn output_port_for(ty: EDataType, _registry: &ETypesRegistry) -> Option<usize> {
        if DEFAULT {
            None
        } else {
            is_type_option(ty).then_some(0)
        }
    }

    fn input_port_for(ty: EDataType, _registry: &ETypesRegistry) -> Option<usize> {
        is_type_option(ty).then_some(0)
    }
}
