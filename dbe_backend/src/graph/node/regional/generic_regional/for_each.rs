use crate::etype::EDataType;
use crate::graph::node::commands::{SnarlCommand, SnarlCommands};
use crate::graph::node::extras::ExecutionExtras;
use crate::graph::node::generic::{GenericNodeField, GenericNodeFieldMut};
use crate::graph::node::regional::{NodeWithVariables, RegionIONode, RegionIoData, RegionIoKind};
use crate::graph::node::stateful::generic::GenericStatefulNode;
use crate::graph::node::variables::remember_variables;
use crate::graph::node::{ExecutionResult, NodeContext};
use crate::graph::region::{get_region_execution_data, RegionExecutionData};
use crate::registry::ETypesRegistry;
use crate::value::{ENumber, EValue};
use egui_snarl::NodeId;
use itertools::Itertools;
use miette::bail;
use smallvec::smallvec;
use ustr::Ustr;
use utils::smallvec_n;

pub type ListForEachNode = ForEachLikeRegionalNode<{ ForEachKind::ForEach as u8 }>;
pub type ListMapNode = ForEachLikeRegionalNode<{ ForEachKind::Map as u8 }>;
pub type ListFilterNode = ForEachLikeRegionalNode<{ ForEachKind::Filter as u8 }>;
pub type ListFilterMapNode = ForEachLikeRegionalNode<{ ForEachKind::FilterMap as u8 }>;
pub type ListFlatMapNode = ForEachLikeRegionalNode<{ ForEachKind::FlatMap as u8 }>;

#[derive(Debug, Clone, Hash)]
pub struct ForEachLikeRegionalNode<const KIND: u8> {
    input_ty: Option<EDataType>,
    output_ty: Option<EDataType>,
}

impl<const KIND: u8> ForEachLikeRegionalNode<KIND> {
    #[inline(always)]
    const fn kind(&self) -> ForEachKind {
        ForEachKind::of(KIND)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
#[repr(u8)]
enum ForEachKind {
    ForEach = 0,
    Map,
    Filter,
    FilterMap,
    FlatMap,
}

impl ForEachKind {
    const fn of(kind: u8) -> Self {
        match kind {
            0 => Self::ForEach,
            1 => Self::Map,
            2 => Self::Filter,
            3 => Self::FilterMap,
            4 => Self::FlatMap,
            _ => unreachable!(),
        }
    }
}

fn is_map(kind: u8) -> bool {
    match ForEachKind::of(kind) {
        ForEachKind::Map | ForEachKind::FilterMap | ForEachKind::FlatMap => true,
        ForEachKind::ForEach | ForEachKind::Filter => false,
    }
}

fn is_filter(kind: u8) -> bool {
    kind == ForEachKind::Filter as u8 || kind == ForEachKind::FilterMap as u8
}

impl PartialEq<u8> for ForEachKind {
    fn eq(&self, other: &u8) -> bool {
        *self as u8 == *other
    }
}

impl PartialEq<ForEachKind> for u8 {
    fn eq(&self, other: &ForEachKind) -> bool {
        *self == *other as u8
    }
}

impl<const KIND: u8> NodeWithVariables for ForEachLikeRegionalNode<KIND> {
    type State<'a> = &'a RegionIoData;
}
impl<const KIND: u8> GenericStatefulNode for ForEachLikeRegionalNode<KIND> {
    type State<'a> = &'a RegionIoData;

    fn id() -> Ustr {
        match ForEachKind::of(KIND) {
            ForEachKind::ForEach => "for_each".into(),
            ForEachKind::Map => "list_map".into(),
            ForEachKind::Filter => "list_filter".into(),
            ForEachKind::FilterMap => "list_filter_map".into(),
            ForEachKind::FlatMap => "flat_map".into(),
        }
    }

    fn input_names(&self, data: &Self::State<'_>) -> &[&str] {
        match data.kind {
            RegionIoKind::Start => &["values"],
            RegionIoKind::End => match self.kind() {
                ForEachKind::ForEach => &[],
                ForEachKind::Map => &["value"],
                ForEachKind::Filter => &["condition"],
                ForEachKind::FilterMap => &["value", "condition"],
                ForEachKind::FlatMap => &["values"],
            },
        }
    }

    fn output_names(&self, data: &Self::State<'_>) -> &[&str] {
        match data.kind {
            RegionIoKind::Start => &["value", "index", "length"],
            RegionIoKind::End => {
                if is_map(KIND) || is_filter(KIND) {
                    &["values"]
                } else {
                    &[]
                }
            }
        }
    }

    // array_op_io! {
    //     inputs {
    //         Start => [List(self.input_ty)],
    //         End => [Value(self.output_ty)]
    //     }
    // }

    // array_op_io! {
    //     outputs {
    //         Start => [2; Value(self.input_ty), Fixed(EDataType::Number)],
    //         End => [2; List(self.output_ty)]
    //     }
    // }

    fn inputs(
        &self,
        _registry: &ETypesRegistry,
        external_state: &Self::State<'_>,
    ) -> impl AsRef<[GenericNodeField]> {
        match external_state.kind {
            RegionIoKind::Start => smallvec_n![2;GenericNodeField::List(&self.input_ty)],
            RegionIoKind::End => match self.kind() {
                ForEachKind::ForEach => smallvec![],
                ForEachKind::Map => smallvec![GenericNodeField::Value(&self.output_ty)],
                ForEachKind::Filter => smallvec![GenericNodeField::Fixed(EDataType::Boolean)],
                ForEachKind::FilterMap => smallvec![
                    GenericNodeField::Value(&self.output_ty),
                    GenericNodeField::Fixed(EDataType::Boolean)
                ],
                ForEachKind::FlatMap => smallvec![GenericNodeField::List(&self.output_ty)],
            },
        }
    }

    fn outputs(
        &self,
        _registry: &ETypesRegistry,
        external_state: &Self::State<'_>,
    ) -> impl AsRef<[GenericNodeField]> {
        match external_state.kind {
            RegionIoKind::Start => {
                smallvec_n![2;GenericNodeField::Value(&self.input_ty), GenericNodeField::Fixed(EDataType::Number), GenericNodeField::Fixed(EDataType::Number)]
            }
            RegionIoKind::End => {
                if is_map(KIND) {
                    smallvec![GenericNodeField::List(&self.output_ty)]
                } else if KIND == ForEachKind::Filter {
                    smallvec![GenericNodeField::List(&self.input_ty)]
                } else {
                    smallvec![]
                }
            }
        }
    }

    fn inputs_mut(
        &mut self,
        _registry: &ETypesRegistry,
        external_state: &Self::State<'_>,
    ) -> impl AsMut<[GenericNodeFieldMut]> {
        match external_state.kind {
            RegionIoKind::Start => smallvec_n![2;GenericNodeFieldMut::List(&mut self.input_ty)],
            RegionIoKind::End => match self.kind() {
                ForEachKind::ForEach => smallvec![],
                ForEachKind::Map => smallvec![GenericNodeFieldMut::Value(&mut self.output_ty)],
                ForEachKind::Filter => smallvec![GenericNodeFieldMut::Fixed(EDataType::Boolean)],
                ForEachKind::FilterMap => smallvec![
                    GenericNodeFieldMut::Value(&mut self.output_ty),
                    GenericNodeFieldMut::Fixed(EDataType::Boolean)
                ],
                ForEachKind::FlatMap => smallvec![GenericNodeFieldMut::List(&mut self.output_ty)],
            },
        }
    }

    fn outputs_mut(
        &mut self,
        _registry: &ETypesRegistry,
        external_state: &Self::State<'_>,
    ) -> impl AsMut<[GenericNodeFieldMut]> {
        match external_state.kind {
            RegionIoKind::Start => {
                smallvec_n![2;GenericNodeFieldMut::Value(&mut self.input_ty), GenericNodeFieldMut::Fixed(EDataType::Number), GenericNodeFieldMut::Fixed(EDataType::Number)]
            }
            RegionIoKind::End => {
                if is_map(KIND) {
                    smallvec![GenericNodeFieldMut::List(&mut self.output_ty)]
                } else if KIND == ForEachKind::Filter {
                    smallvec![GenericNodeFieldMut::List(&mut self.input_ty)]
                } else {
                    smallvec![]
                }
            }
        }
    }

    fn types_changed(
        &mut self,
        context: NodeContext,
        region_data: &RegionIoData,
        _node: NodeId,
        commands: &mut SnarlCommands,
    ) {
        if KIND != ForEachKind::Filter {
            return;
        }

        let Some(ty) = self.input_ty else {
            return;
        };

        let Ok(data) = context.region_graph.try_as_data() else {
            self.input_ty = None;
            return;
        };

        let other_id = if region_data.is_start() {
            data.region_data(&region_data.region).end_node
        } else {
            data.region_data(&region_data.region).start_node
        };

        commands.push(SnarlCommand::Custom {
            cb: Box::new(move |ctx| {
                let other = ctx.snarl[other_id]
                    .downcast_mut::<RegionIONode<Self>>()
                    .unwrap();
                other.node.input_ty = Some(ty);
                Ok(())
            }),
        })
    }

    fn should_execute(
        &self,
        _context: NodeContext,
        region: &RegionIoData,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<bool> {
        let state = get_region_execution_data::<ForEachNodeState>(region.region, variables)?;

        Ok(state.index < state.length)
    }

    fn execute(
        &self,
        context: NodeContext,
        region: &RegionIoData,
        inputs: &[EValue],
        outputs: &mut Vec<EValue>,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<ExecutionResult> {
        if region.is_start() {
            let EValue::List { values, .. } = &inputs[0] else {
                bail!("Expected list input, got: {}", inputs[0].ty().name());
            };
            let state = variables.get_or_init_region_data(region.region, |_| ForEachNodeState {
                index: 0,
                length: values.len(),
                output: Vec::with_capacity(values.len()),
                input_value: None,
                values: None,
            });

            outputs.clear();
            if !values.is_empty() {
                outputs.push(values[state.index].clone());
                if KIND == ForEachKind::Filter {
                    state.input_value = Some(values[state.index].clone());
                }
            }
            outputs.push(ENumber::from(state.index as f64).into());
            outputs.push(ENumber::from(state.length as f64).into());

            remember_variables(&mut state.values, &inputs[1..], outputs);

            // if !MAP && !FILTER {
            //     debug!(?outputs, state.index, state.length, "ForEachNode start");
            // }

            Ok(ExecutionResult::Done)
        } else {
            let state = get_region_execution_data::<ForEachNodeState>(region.region, variables)?;

            if state.length > 0 {
                match self.kind() {
                    ForEachKind::ForEach => {}
                    ForEachKind::Map => {
                        state.output.push(inputs[0].clone());
                    }
                    ForEachKind::Filter => {
                        if *inputs[0].try_as_boolean()? {
                            state.output.push(
                                state
                                    .input_value
                                    .take()
                                    .expect("Filter node should have input value"),
                            );
                        }
                    }
                    ForEachKind::FilterMap => {
                        if *inputs[1].try_as_boolean()? {
                            state.output.push(inputs[0].clone());
                        }
                    }
                    ForEachKind::FlatMap => {
                        let EValue::List { values, .. } = &inputs[0] else {
                            bail!("Expected list input, got: {}", inputs[0].ty().name());
                        };
                        state.output.extend(values.iter().cloned());
                    }
                }
            }
            state.index += 1;

            let skip_n = match self.kind() {
                ForEachKind::ForEach => 0,
                ForEachKind::Map | ForEachKind::Filter | ForEachKind::FlatMap => 1,
                ForEachKind::FilterMap => 2,
            };
            if state.index >= state.length {
                outputs.clear();
                if is_map(KIND) {
                    outputs.push(EValue::List {
                        values: std::mem::take(&mut state.output),
                        id: context
                            .registry
                            .list_id_of(self.output_ty.unwrap_or_else(EDataType::null)),
                    });
                } else if is_filter(KIND) {
                    outputs.push(EValue::List {
                        values: std::mem::take(&mut state.output),
                        id: context
                            .registry
                            .list_id_of(self.input_ty.unwrap_or_else(EDataType::null)),
                    });
                }
                outputs.extend(inputs.iter().skip(skip_n).cloned());
                variables.remove_region_data(region.region);
                Ok(ExecutionResult::Done)
            } else {
                state.values = Some(inputs.iter().skip(skip_n).cloned().collect_vec());
                Ok(ExecutionResult::RerunRegion {
                    region: region.region,
                })
            }
        }
    }

    fn categories() -> &'static [&'static str] {
        &["list", "utility.iterators"]
    }

    fn create() -> Self {
        Self {
            input_ty: None,
            output_ty: None,
        }
    }

    fn output_port_for(ty: EDataType, _registry: &ETypesRegistry) -> Option<usize> {
        match ForEachKind::of(KIND) {
            ForEachKind::ForEach => None,
            ForEachKind::Map
            | ForEachKind::Filter
            | ForEachKind::FilterMap
            | ForEachKind::FlatMap => ty.is_list().then_some(0),
        }
    }

    fn input_port_for(ty: EDataType, _registry: &ETypesRegistry) -> Option<usize> {
        ty.is_list().then_some(0)
    }
}

#[derive(Debug)]
struct ForEachNodeState {
    index: usize,
    length: usize,
    output: Vec<EValue>,
    input_value: Option<EValue>,
    values: Option<Vec<EValue>>,
}

impl RegionExecutionData for ForEachNodeState {}
