use crate::etype::EDataType;
use crate::graph::node::extras::ExecutionExtras;
use crate::graph::node::generic::macros::generic_node_io;
use crate::graph::node::regional::{NodeWithVariables, RegionIoData, RegionIoKind};
use crate::graph::node::stateful::generic::GenericStatefulNode;
use crate::graph::node::variables::remember_variables;
use crate::graph::node::{ExecutionResult, NodeContext};
use crate::graph::region::{get_region_execution_data, RegionExecutionData};
use crate::registry::ETypesRegistry;
use crate::value::EValue;
use ustr::Ustr;

#[derive(Debug, Clone, Hash)]
pub struct ConstructListNode {
    output_ty: Option<EDataType>,
}

impl NodeWithVariables for ConstructListNode {
    type State<'a> = &'a RegionIoData;
}

impl GenericStatefulNode for ConstructListNode {
    type State<'a> = &'a RegionIoData;

    fn id() -> Ustr {
        "construct_list".into()
    }

    fn input_names(&self, data: &Self::State<'_>) -> &[&str] {
        match data.kind {
            RegionIoKind::Start => &["length"],
            RegionIoKind::End => &["value"],
        }
    }

    fn output_names(&self, data: &Self::State<'_>) -> &[&str] {
        match data.kind {
            RegionIoKind::Start => &["index"],
            RegionIoKind::End => &["values"],
        }
    }

    generic_node_io! {
        inputs {
            Start => [Fixed(EDataType::Number)],
            End => [Value(self.output_ty)]
        }
    }

    generic_node_io! {
        outputs {
            Start => [Fixed(EDataType::Number)],
            End => [List(self.output_ty)]
        }
    }

    fn should_execute(
        &self,
        _context: NodeContext,
        region: &RegionIoData,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<bool> {
        let state = get_region_execution_data::<ConstructNodeState>(region.region, variables)?;

        Ok(state.current < state.repeats)
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
            let n_repeats = inputs[0].try_as_number()?;
            let state = variables.get_or_init_region_data(region.region, |_| ConstructNodeState {
                current: 0,
                repeats: n_repeats.0 as usize,
                output: Vec::with_capacity(n_repeats.0 as usize),
                values: None,
            });

            outputs.clear();
            outputs.push(EValue::from(state.current as f64));

            remember_variables(&mut state.values, &inputs[1..], outputs);

            Ok(ExecutionResult::Done)
        } else {
            let state = get_region_execution_data::<ConstructNodeState>(region.region, variables)?;

            if state.repeats > 0 {
                state.output.push(inputs[0].clone());
            }
            state.current += 1;

            if state.current >= state.repeats {
                outputs.clear();
                outputs.push(EValue::List {
                    values: std::mem::take(&mut state.output),
                    id: context
                        .registry
                        .list_id_of(self.output_ty.unwrap_or_else(EDataType::null)),
                });
                outputs.extend(inputs.iter().skip(1).cloned());
                variables.remove_region_data(region.region);
                Ok(ExecutionResult::Done)
            } else {
                state.values = Some(inputs[1..].to_vec());
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
        Self { output_ty: None }
    }

    fn output_port_for(ty: EDataType, _registry: &ETypesRegistry) -> Option<usize> {
        ty.is_list().then_some(0)
    }

    fn input_port_for(ty: EDataType, _registry: &ETypesRegistry) -> Option<usize> {
        ty.is_number().then_some(0)
    }
}

#[derive(Debug)]
struct ConstructNodeState {
    current: usize,
    repeats: usize,
    output: Vec<EValue>,
    values: Option<Vec<EValue>>,
}

impl RegionExecutionData for ConstructNodeState {}
