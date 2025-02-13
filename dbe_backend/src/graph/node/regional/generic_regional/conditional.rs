use crate::etype::EDataType;
use crate::graph::node::extras::ExecutionExtras;
use crate::graph::node::generic::{GenericNodeField, GenericNodeFieldMut};
use crate::graph::node::regional::{NodeWithVariables, RegionIoData, RegionIoKind, VariableSide};
use crate::graph::node::stateful::generic::GenericStatefulNode;
use crate::graph::node::{ExecutionResult, NodeContext};
use crate::graph::region::{get_region_execution_data, RegionExecutionData};
use crate::registry::optional_helpers::{none_of_type, unwrap_optional_value, wrap_in_some};
use crate::registry::ETypesRegistry;
use crate::value::EValue;
use smallvec::smallvec;
use ustr::Ustr;
use utils::smallvec_n;

pub type ConditionalIfNode = ConditionalNode<{ ConditionalKind::If as u8 }>;
pub type ConditionalMapNode = ConditionalNode<{ ConditionalKind::Map as u8 }>;

#[derive(Debug, Clone, Hash)]
pub struct ConditionalNode<const KIND: u8> {
    input_ty: Option<EDataType>,
}

impl<const KIND: u8> ConditionalNode<KIND> {
    #[inline(always)]
    const fn kind(&self) -> ConditionalKind {
        ConditionalKind::of(KIND)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
#[repr(u8)]
enum ConditionalKind {
    If = 0,
    Map = 1,
}

impl ConditionalKind {
    const fn of(kind: u8) -> Self {
        match kind {
            0 => Self::If,
            1 => Self::Map,
            _ => unreachable!(),
        }
    }
}
impl<const KIND: u8> NodeWithVariables for ConditionalNode<KIND> {
    type State<'a> = &'a RegionIoData;

    fn allow_variables(data: &RegionIoData) -> VariableSide {
        if data.is_start() {
            VariableSide::empty()
        } else {
            VariableSide::all()
        }
    }

    fn output_variable_type<'a>(
        &self,
        _context: NodeContext,
        data: &RegionIoData,
        ty: &'a Option<EDataType>,
    ) -> GenericNodeField<'a> {
        if !data.is_end() {
            panic!("Repeat node has no variables on the start")
        }

        GenericNodeField::Option(ty)
    }

    fn output_variable_type_mut<'a>(
        &self,
        _context: NodeContext,
        data: &RegionIoData,
        ty: &'a mut Option<EDataType>,
    ) -> GenericNodeFieldMut<'a> {
        if !data.is_end() {
            panic!("Repeat node has no variables on the start")
        }

        GenericNodeFieldMut::Option(ty)
    }
}

impl<const KIND: u8> GenericStatefulNode for ConditionalNode<KIND> {
    type State<'a> = &'a RegionIoData;

    fn id() -> Ustr {
        match ConditionalKind::of(KIND) {
            ConditionalKind::If => "conditional".into(),
            ConditionalKind::Map => "option_map".into(),
        }
    }

    fn input_names(&self, data: &Self::State<'_>) -> &[&str] {
        if !data.is_start() {
            panic!("Conditional node has no inputs on the end")
        }

        match self.kind() {
            ConditionalKind::If => &["condition"],
            ConditionalKind::Map => &["option"],
        }
    }

    fn output_names(&self, data: &Self::State<'_>) -> &[&str] {
        if !data.is_start() {
            panic!("Conditional node has no outputs on the end")
        }
        match self.kind() {
            ConditionalKind::If => &[],
            ConditionalKind::Map => &["value"],
        }
    }

    fn inputs(
        &self,
        _registry: &ETypesRegistry,
        external_state: &Self::State<'_>,
    ) -> impl AsRef<[GenericNodeField]> {
        match external_state.kind {
            RegionIoKind::Start => match self.kind() {
                ConditionalKind::If => smallvec_n![1;GenericNodeField::Fixed(EDataType::Boolean)],
                ConditionalKind::Map => smallvec![GenericNodeField::Option(&self.input_ty)],
            },
            RegionIoKind::End => smallvec![],
        }
    }

    fn outputs(
        &self,
        _registry: &ETypesRegistry,
        external_state: &Self::State<'_>,
    ) -> impl AsRef<[GenericNodeField]> {
        match external_state.kind {
            RegionIoKind::Start => match self.kind() {
                ConditionalKind::If => smallvec![],
                ConditionalKind::Map => smallvec_n![1;GenericNodeField::Value(&self.input_ty)],
            },
            RegionIoKind::End => smallvec![],
        }
    }

    fn inputs_mut(
        &mut self,
        _registry: &ETypesRegistry,
        external_state: &Self::State<'_>,
    ) -> impl AsMut<[GenericNodeFieldMut]> {
        match external_state.kind {
            RegionIoKind::Start => match self.kind() {
                ConditionalKind::If => {
                    smallvec_n![1;GenericNodeFieldMut::Fixed(EDataType::Boolean)]
                }
                ConditionalKind::Map => smallvec![GenericNodeFieldMut::Option(&mut self.input_ty)],
            },
            RegionIoKind::End => smallvec![],
        }
    }

    fn outputs_mut(
        &mut self,
        _registry: &ETypesRegistry,
        external_state: &Self::State<'_>,
    ) -> impl AsMut<[GenericNodeFieldMut]> {
        match external_state.kind {
            RegionIoKind::Start => match self.kind() {
                ConditionalKind::If => smallvec![],
                ConditionalKind::Map => {
                    smallvec_n![1;GenericNodeFieldMut::Value(&mut self.input_ty)]
                }
            },
            RegionIoKind::End => smallvec![],
        }
    }

    fn should_execute(
        &self,
        _context: NodeContext,
        region: &RegionIoData,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<bool> {
        let state = get_region_execution_data::<ConditionalNodeState>(region.region, variables)?;

        Ok(state.condition)
    }

    fn execute(
        &self,
        context: NodeContext,
        region: &RegionIoData,
        inputs: &[EValue],
        outputs: &mut Vec<EValue>,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<ExecutionResult> {
        outputs.clear();

        if region.is_start() {
            match ConditionalKind::of(KIND) {
                ConditionalKind::If => {
                    let condition = *inputs[0].try_as_boolean()?;
                    variables.get_or_init_region_data(region.region, |_| ConditionalNodeState {
                        condition,
                    });
                }
                ConditionalKind::Map => {
                    let value = unwrap_optional_value(context.registry, &inputs[0])?;
                    let condition = value.is_some();
                    variables.get_or_init_region_data(region.region, |_| ConditionalNodeState {
                        condition,
                    });
                    if let Some(value) = value {
                        outputs.push(value.clone())
                    }
                }
            }
            Ok(ExecutionResult::Done)
        } else {
            let state =
                get_region_execution_data::<ConditionalNodeState>(region.region, variables)?;

            if state.condition {
                outputs.extend(
                    inputs
                        .iter()
                        .map(|value| wrap_in_some(context.registry, value.clone())),
                );
            } else {
                outputs.extend(
                    inputs
                        .iter()
                        .map(|value| none_of_type(context.registry, value.ty())),
                );
            }

            variables.remove_region_data(region.region);
            Ok(ExecutionResult::Done)
        }
    }

    fn categories() -> &'static [&'static str] {
        &["optional"]
    }

    fn create() -> Self {
        Self { input_ty: None }
    }

    fn input_port_for(ty: EDataType, _registry: &ETypesRegistry) -> Option<usize> {
        ty.is_boolean().then_some(0)
    }
}

#[derive(Debug)]
struct ConditionalNodeState {
    condition: bool,
}

impl RegionExecutionData for ConditionalNodeState {}
