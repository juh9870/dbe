use crate::etype::default::DefaultEValue;
use crate::etype::EDataType;
use crate::graph::node::commands::SnarlCommands;
use crate::graph::node::extras::ExecutionExtras;
use crate::graph::node::functional::generic::{sync_generic_state, MAX_FIELDS};
use crate::graph::node::functional::FunctionalNode;
use crate::graph::node::generic::{GenericNode, GenericNodeField, GenericNodeFieldMut};
use crate::graph::node::{ExecutionResult, Node, NodeContext, NodeFactory};
use crate::registry::optional_helpers::is_type_option;
use crate::registry::ETypesRegistry;
use crate::value::EValue;
use arrayvec::ArrayVec;
use egui_snarl::NodeId;
use miette::miette;
use ustr::Ustr;

#[derive(Debug, Clone, Hash)]
pub(super) struct FuncNodeState<T: FunctionalNode> {
    node: T,
    input_ty: [Option<EDataType>; MAX_FIELDS],
    output_ty: [Option<EDataType>; MAX_FIELDS],
}

impl<T: FunctionalNode> GenericNode for FuncNodeState<T> {
    fn id(&self) -> Ustr {
        self.node.id().into()
    }

    fn input_names(&self) -> &[&str] {
        self.node.input_names()
    }

    fn output_names(&self) -> &[&str] {
        self.node.output_names()
    }

    fn default_input_value(
        &self,
        context: NodeContext,
        input: usize,
    ) -> miette::Result<DefaultEValue> {
        if let Some(value) = self.node.custom_default_value(context, input)? {
            Ok(value)
        } else {
            let inputs = self.inputs(context.registry);
            let input = inputs
                .as_ref()
                .get(input)
                .ok_or_else(|| miette!("Invalid input index: {}", input))?;
            Ok(input.ty(context.registry).default_value(context.registry))
        }
    }

    fn inputs(&self, registry: &ETypesRegistry) -> impl AsRef<[GenericNodeField]> {
        let types: ArrayVec<GenericNodeField, { MAX_FIELDS }> = self
            .input_ty
            .iter()
            .take(T::inputs_count())
            .enumerate()
            .map(|(idx, ty)| T::input(registry, idx, ty))
            .collect();

        types
    }

    fn outputs(&self, registry: &ETypesRegistry) -> impl AsRef<[GenericNodeField]> {
        let types: ArrayVec<GenericNodeField, { MAX_FIELDS }> = self
            .output_ty
            .iter()
            .take(T::outputs_count())
            .enumerate()
            .map(|(idx, ty)| T::output(registry, idx, ty))
            .collect();

        types
    }

    fn inputs_mut(&mut self, registry: &ETypesRegistry) -> impl AsMut<[GenericNodeFieldMut]> {
        let types: ArrayVec<GenericNodeFieldMut, { MAX_FIELDS }> = self
            .input_ty
            .iter_mut()
            .take(T::inputs_count())
            .enumerate()
            .map(|(idx, ty)| T::input_mut(registry, idx, ty))
            .collect();

        types
    }

    fn outputs_mut(&mut self, registry: &ETypesRegistry) -> impl AsMut<[GenericNodeFieldMut]> {
        let types: ArrayVec<GenericNodeFieldMut, { MAX_FIELDS }> = self
            .output_ty
            .iter_mut()
            .take(T::outputs_count())
            .enumerate()
            .map(|(idx, ty)| T::output_mut(registry, idx, ty))
            .collect();

        types
    }

    fn types_changed(
        &mut self,
        _context: NodeContext,
        _node: NodeId,
        _commands: &mut SnarlCommands,
    ) {
        sync_generic_state(
            self.input_ty
                .iter_mut()
                .take(T::inputs_count())
                .chain(self.output_ty.iter_mut().take(T::outputs_count())),
            T::input_generic_indices()
                .into_iter()
                .chain(T::output_generic_indices()),
        );
    }

    fn has_side_effects(&self) -> bool {
        self.node.has_side_effects()
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
            &self.input_ty,
            &self.output_ty,
            variables,
            inputs,
            outputs,
        )?;

        Ok(ExecutionResult::Done)
    }
}

impl<T: FunctionalNode + Clone> NodeFactory for T {
    fn id(&self) -> Ustr {
        <Self as FunctionalNode>::id(self).into()
    }

    fn categories(&self) -> &'static [&'static str] {
        <Self as FunctionalNode>::categories(self)
    }

    fn create(&self) -> Box<dyn Node> {
        Box::new(FuncNodeState {
            node: self.clone(),
            input_ty: [None; MAX_FIELDS],
            output_ty: [None; MAX_FIELDS],
        })
    }

    fn output_port_for(&self, ty: EDataType, registry: &ETypesRegistry) -> Option<usize> {
        let count = T::outputs_count();
        (0..count).find(|&i| match T::output(registry, i, &None) {
            GenericNodeField::List(_) => ty.is_list(),
            GenericNodeField::Value(_) => true,
            GenericNodeField::Option(_) => is_type_option(ty),
            GenericNodeField::Object(_) => ty.is_object(),
            GenericNodeField::Fixed(fixed_ty) => ty == fixed_ty,
        })
    }

    fn input_port_for(&self, ty: EDataType, registry: &ETypesRegistry) -> Option<usize> {
        let count = T::inputs_count();
        (0..count).find(|&i| match T::input(registry, i, &None) {
            GenericNodeField::List(_) => ty.is_list(),
            GenericNodeField::Value(_) => true,
            GenericNodeField::Option(_) => is_type_option(ty),
            GenericNodeField::Object(_) => ty.is_object(),
            GenericNodeField::Fixed(fixed_ty) => fixed_ty.is_unknown() || ty == fixed_ty,
        })
    }
}
