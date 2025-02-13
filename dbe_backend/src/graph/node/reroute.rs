use crate::etype::eitem::EItemInfo;
use crate::etype::EDataType;
use crate::graph::node::commands::{SnarlCommand, SnarlCommands};
use crate::graph::node::ports::NodePortType;
use crate::graph::node::{
    ExecutionExtras, ExecutionResult, InputData, Node, NodeContext, NodeFactory, OutputData,
};
use crate::value::EValue;
use egui_snarl::{InPin, InPinId, OutPin, OutPinId};
use ustr::Ustr;

#[derive(Debug, Clone, Hash, Default)]
pub struct RerouteNode {
    inputs: Vec<EDataType>,
}

impl Node for RerouteNode {
    fn id(&self) -> Ustr {
        RerouteFactory.id()
    }

    fn has_inline_values(&self, _input: usize) -> bool {
        false
    }

    fn inputs_count(&self, _context: NodeContext) -> usize {
        self.inputs.len() + 1
    }

    fn input_unchecked(&self, _context: NodeContext, input: usize) -> miette::Result<InputData> {
        if input == self.inputs.len() {
            return Ok(InputData::new(NodePortType::BasedOnSource, "".into()));
        }
        Ok(InputData::new(
            EItemInfo::simple_type(self.inputs[input]).into(),
            input.to_string().into(),
        ))
    }

    fn outputs_count(&self, _context: NodeContext) -> usize {
        self.inputs.len()
    }

    fn output_unchecked(&self, _context: NodeContext, output: usize) -> miette::Result<OutputData> {
        Ok(OutputData::new(
            EItemInfo::simple_type(self.inputs[output]).into(),
            output.to_string().into(),
        ))
    }

    fn try_connect(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        from: &OutPin,
        to: &InPin,
        incoming_type: &NodePortType,
    ) -> miette::Result<bool> {
        let Some(info) = incoming_type.item_info() else {
            return Ok(false);
        };

        let i = to.id.input;
        if i == self.inputs.len() {
            self.inputs.push(info.ty());
        } else if self.inputs[i] != incoming_type.ty() {
            self.inputs[i] = info.ty();
            // Reconnect the corresponding output pin to propagate type
            // changes and clear invalid connections
            commands.push(SnarlCommand::ReconnectOutput {
                id: OutPinId {
                    node: to.id.node,
                    output: i,
                },
            })
        }

        self._default_try_connect(context, commands, from, to, incoming_type)
    }

    fn try_disconnect(
        &mut self,
        _context: NodeContext,
        commands: &mut SnarlCommands,
        from: &OutPin,
        to: &InPin,
    ) -> miette::Result<()> {
        commands.push(SnarlCommand::DisconnectRaw {
            from: from.id,
            to: to.id,
        });
        commands.push(SnarlCommand::DropOutputs {
            from: OutPinId {
                node: to.id.node,
                output: to.id.input,
            },
        });
        for i in to.id.input..(self.inputs.len() - 1) {
            self.inputs.as_mut_slice().swap(i, i + 1);
            commands.push(SnarlCommand::InputMovedRaw {
                from: InPinId {
                    node: to.id.node,
                    input: i + 1,
                },
                to: InPinId {
                    node: to.id.node,
                    input: i,
                },
            });
            commands.push(SnarlCommand::OutputMovedRaw {
                from: OutPinId {
                    node: to.id.node,
                    output: i + 1,
                },
                to: OutPinId {
                    node: to.id.node,
                    output: i,
                },
            });
        }

        self.inputs.pop();
        commands.push(SnarlCommand::DeletePinValue {
            pin: InPinId {
                node: to.id.node,
                input: self.inputs.len(),
            },
        });

        Ok(())
    }

    fn execute(
        &self,
        _context: NodeContext,
        inputs: &[EValue],
        outputs: &mut Vec<EValue>,
        _variables: &mut ExecutionExtras,
    ) -> miette::Result<ExecutionResult> {
        for input in inputs.iter() {
            outputs.push(input.clone());
        }

        Ok(ExecutionResult::Done)
    }
}

#[derive(Debug, Clone)]
pub struct RerouteFactory;

impl NodeFactory for RerouteFactory {
    fn id(&self) -> Ustr {
        "reroute".into()
    }

    fn categories(&self) -> &'static [&'static str] {
        &["utility"]
    }

    fn create(&self) -> Box<dyn Node> {
        Box::new(RerouteNode::default())
    }
}
