use crate::etype::eitem::EItemInfo;
use crate::etype::EDataType;
use crate::graph::node::commands::{SnarlCommand, SnarlCommands};
use crate::graph::node::ports::NodePortType;
use crate::graph::node::{
    ExecutionExtras, ExecutionResult, InputData, Node, NodeContext, NodeFactory, OutputData,
};
use crate::json_utils::JsonValue;
use crate::registry::ETypesRegistry;
use crate::value::EValue;
use egui_snarl::{InPin, OutPin, OutPinId};
use miette::IntoDiagnostic;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct ListNode {
    item: Option<EDataType>,
    #[serde(default)]
    items_count: usize,
    #[serde(default)]
    has_connection: Vec<bool>,
}

impl ListNode {
    pub fn new() -> Self {
        Self {
            item: None,
            items_count: 0,
            has_connection: vec![],
        }
    }

    pub fn of_type(ty: EDataType) -> Self {
        Self {
            item: Some(ty),
            items_count: 0,
            has_connection: vec![],
        }
    }
}

impl Default for ListNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for ListNode {
    fn write_json(&self, _registry: &ETypesRegistry) -> miette::Result<JsonValue> {
        serde_json::value::to_value(self).into_diagnostic()
    }
    fn parse_json(
        &mut self,
        _registry: &ETypesRegistry,
        value: &mut JsonValue,
    ) -> miette::Result<()> {
        Self::deserialize(value.take())
            .map(|node| *self = node)
            .into_diagnostic()?;
        self.has_connection.resize_with(self.items_count, || false);
        Ok(())
    }

    fn id(&self) -> Ustr {
        ListNodeFactory.id()
    }

    fn has_inline_values(&self, _input: usize) -> bool {
        false
    }

    fn inputs_count(&self, _context: NodeContext) -> usize {
        self.items_count + 1
    }

    fn input_unchecked(&self, _context: NodeContext, input: usize) -> miette::Result<InputData> {
        Ok(InputData::new(
            if let Some(ty) = self.item.as_ref() {
                EItemInfo::simple_type(*ty).into()
            } else {
                NodePortType::BasedOnSource
            },
            if input == self.items_count {
                "+".into()
            } else {
                input.to_string().into()
            },
        ))
    }

    fn outputs_count(&self, _context: NodeContext) -> usize {
        1
    }

    fn output_unchecked(&self, context: NodeContext, output: usize) -> miette::Result<OutputData> {
        Ok(OutputData::new(
            EItemInfo::simple_type(
                context
                    .registry
                    .list_of(self.item.unwrap_or(EDataType::Unknown)),
            )
            .into(),
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
        if incoming_type.is_specific() && self.item.is_none() {
            self.item = Some(incoming_type.ty());
            commands.push(SnarlCommand::ReconnectOutput {
                id: OutPinId {
                    node: to.id.node,
                    output: 0,
                },
            })
        }

        if self._default_try_connect(context, commands, from, to, incoming_type)? {
            if to.id.input == self.items_count {
                self.items_count += 1;
                self.has_connection.push(true);
            } else {
                self.has_connection[to.id.input] = true;
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn try_disconnect(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        from: &OutPin,
        to: &InPin,
    ) -> miette::Result<()> {
        self._default_try_disconnect(context, commands, from, to)?;
        self.has_connection[to.id.input] = false;
        while self.items_count > 0 && !self.has_connection[self.items_count - 1] {
            self.items_count -= 1;
            self.has_connection.pop();
        }

        Ok(())
    }

    fn execute(
        &self,
        context: NodeContext,
        inputs: &[EValue],
        outputs: &mut Vec<EValue>,
        _variables: &mut ExecutionExtras,
    ) -> miette::Result<ExecutionResult> {
        let mut values = vec![];
        // TODO: check for inputs count to match items_count
        for input in inputs.iter().take(self.items_count).cloned() {
            values.push(input);
        }
        outputs.clear();
        outputs.push(EValue::List {
            id: context
                .registry
                .list_id_of(self.item.unwrap_or(EDataType::Unknown)),
            values,
        });

        Ok(ExecutionResult::Done)
    }
}

#[derive(Debug, Clone)]
pub struct ListNodeFactory;

impl NodeFactory for ListNodeFactory {
    fn id(&self) -> Ustr {
        "list".into()
    }

    fn categories(&self) -> &'static [&'static str] {
        &["list"]
    }

    fn create(&self) -> Box<dyn Node> {
        Box::new(ListNode::default())
    }
}
