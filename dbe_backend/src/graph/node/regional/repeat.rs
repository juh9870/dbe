use crate::etype::eitem::EItemInfo;
use crate::etype::EDataType;
use crate::graph::node::extras::ExecutionExtras;
use crate::graph::node::ports::{InputData, OutputData};
use crate::graph::node::regional::{NodeWithVariables, RegionIoData};
use crate::graph::node::stateful::StatefulNode;
use crate::graph::node::variables::remember_variables;
use crate::graph::node::{ExecutionResult, NodeContext};
use crate::graph::region::RegionExecutionData;
use crate::json_utils::json_serde::JsonSerde;
use crate::registry::ETypesRegistry;
use crate::value::EValue;
use miette::bail;
use ustr::Ustr;

#[derive(Debug, Clone, Hash)]
pub struct RepeatNode;

impl NodeWithVariables for RepeatNode {
    type State<'a> = &'a RegionIoData;
}

impl JsonSerde for RepeatNode {
    type State<'a> = &'a RegionIoData;
}

impl StatefulNode for RepeatNode {
    type State<'a> = &'a RegionIoData;

    fn id() -> Ustr {
        "repeat".into()
    }

    fn inputs_count(&self, _context: NodeContext, data: &RegionIoData) -> usize {
        if data.is_start() {
            1
        } else {
            0
        }
    }

    fn outputs_count(&self, _context: NodeContext, data: &RegionIoData) -> usize {
        if data.is_start() {
            1
        } else {
            0
        }
    }

    fn input_unchecked(
        &self,
        _context: NodeContext,
        data: &RegionIoData,
        _input: usize,
    ) -> miette::Result<InputData> {
        if data.is_start() {
            Ok(InputData::new(
                EItemInfo::simple_type(EDataType::Number).into(),
                "iterations".into(),
            ))
        } else {
            panic!("Repeat node has no inputs on the end")
        }
    }

    fn output_unchecked(
        &self,
        _context: NodeContext,
        data: &RegionIoData,
        _output: usize,
    ) -> miette::Result<OutputData> {
        if data.is_start() {
            Ok(OutputData::new(
                EItemInfo::simple_type(EDataType::Number).into(),
                "iteration".into(),
            ))
        } else {
            panic!("Repeat node has no outputs on the end")
        }
    }

    fn should_execute(
        &self,
        _context: NodeContext,
        data: &RegionIoData,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<bool> {
        let Some(state) = variables.get_region_data::<RepeatNodeState>(data.region) else {
            bail!("End of repeat node without start")
        };

        Ok(state.current < state.repeats)
    }

    fn execute(
        &self,
        _context: NodeContext,
        data: &RegionIoData,
        inputs: &[EValue],
        outputs: &mut Vec<EValue>,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<ExecutionResult> {
        if data.is_start() {
            let n_repeats = inputs[0].try_as_number()?;
            let state = variables.get_or_init_region_data(data.region, |_| RepeatNodeState {
                repeats: n_repeats.0 as u32,
                current: 0,
                values: None,
            });

            outputs.clear();
            outputs.push(EValue::from(state.current as f64));

            remember_variables(&mut state.values, &inputs[1..], outputs);

            Ok(ExecutionResult::Done)
        } else {
            let Some(state) = variables.get_region_data::<RepeatNodeState>(data.region) else {
                bail!("End of repeat node without start")
            };

            state.current += 1;

            if state.current >= state.repeats {
                outputs.clear();
                outputs.extend(inputs.iter().cloned());
                variables.remove_region_data(data.region);
                Ok(ExecutionResult::Done)
            } else {
                state.values = Some(inputs.to_vec());
                Ok(ExecutionResult::RerunRegion {
                    region: data.region,
                })
            }
        }
    }

    fn categories() -> &'static [&'static str] {
        &["utility", "utility.iterators"]
    }

    fn create() -> Self {
        Self
    }

    fn input_port_for(ty: EDataType, _registry: &ETypesRegistry) -> Option<usize> {
        ty.is_number().then_some(0)
    }
}

#[derive(Debug)]
struct RepeatNodeState {
    repeats: u32,
    current: u32,
    values: Option<Vec<EValue>>,
}

impl RegionExecutionData for RepeatNodeState {}
