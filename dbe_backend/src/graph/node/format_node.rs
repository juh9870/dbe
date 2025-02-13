use crate::etype::eitem::EItemInfo;
use crate::etype::EDataType;
use crate::graph::node::commands::SnarlCommands;
use crate::graph::node::editable_state::EditableState;
use crate::graph::node::extras::ExecutionExtras;
use crate::graph::node::ports::fields::mappers::USTR_MAPPER;
use crate::graph::node::ports::fields::{sync_fields, IoDirection};
use crate::graph::node::ports::{InputData, OutputData};
use crate::graph::node::{ExecutionResult, Node, NodeContext, NodeFactory};
use crate::json_utils::JsonValue;
use crate::registry::ETypesRegistry;
use crate::value::EValue;
use egui_snarl::NodeId;
use miette::{bail, IntoDiagnostic, WrapErr};
use serde::{Deserialize, Serialize};
use smallvec::smallvec;
use squidfmt::formatting::{FormatKeyError, FormatKeys};
use squidfmt::PreparedFmt;
use std::fmt::Formatter;
use ustr::Ustr;

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct FormatNode {
    format: String,
    keys: Vec<Ustr>,
    #[serde(skip)]
    fmt: Option<PreparedFmt>,
}

impl FormatNode {
    fn sync_fmt(&mut self) -> miette::Result<()> {
        match PreparedFmt::parse(&self.format)
            .into_diagnostic()
            .context("failed to parse format string")
        {
            Ok(data) => {
                self.fmt = Some(data);
            }
            Err(err) => {
                self.fmt = None;
                return Err(err);
            }
        }
        Ok(())
    }

    fn sync_fields(&mut self, commands: &mut SnarlCommands, id: NodeId) -> miette::Result<()> {
        if self.fmt.is_none() {
            self.sync_fmt()?;
        }
        let fmt = self
            .fmt
            .as_ref()
            .expect("Format should either be parsed or error should be returned");
        let fields = fmt.keys();
        sync_fields(
            &USTR_MAPPER,
            commands,
            fields.as_slice(),
            &mut self.keys,
            id,
            IoDirection::Input(0),
        );

        Ok(())
    }
}

impl Node for FormatNode {
    fn write_json(&self, _registry: &ETypesRegistry) -> miette::Result<JsonValue> {
        serde_json::value::to_value(self).into_diagnostic()
    }
    fn parse_json(
        &mut self,
        _registry: &ETypesRegistry,
        value: &mut JsonValue,
    ) -> miette::Result<()> {
        miette::IntoDiagnostic::into_diagnostic(Self::deserialize(value.take()))
            .map(|node| *self = node)?;

        self.sync_fmt()?;

        Ok(())
    }

    fn id(&self) -> Ustr {
        FormatNodeFactory.id()
    }

    fn update_state(
        &mut self,
        _context: NodeContext,
        commands: &mut SnarlCommands,
        id: NodeId,
    ) -> miette::Result<()> {
        self.sync_fields(commands, id)?;
        Ok(())
    }

    fn has_editable_state(&self) -> bool {
        true
    }

    fn editable_state(&self) -> EditableState {
        smallvec![(
            "format".into(),
            EValue::String {
                value: self.format.clone()
            }
            .into()
        )]
    }

    fn apply_editable_state(
        &mut self,
        _context: NodeContext,
        state: EditableState,
        commands: &mut SnarlCommands,
        node_id: NodeId,
    ) -> miette::Result<()> {
        let EValue::String { value } = state.into_iter().nth(0).unwrap().1.try_as_value().unwrap()
        else {
            panic!("Expected string value");
        };

        self.format = value;
        self.sync_fmt()?;
        self.sync_fields(commands, node_id)?;
        Ok(())
    }

    fn inputs_count(&self, _context: NodeContext) -> usize {
        self.keys.len()
    }

    fn input_unchecked(&self, _context: NodeContext, input: usize) -> miette::Result<InputData> {
        Ok(InputData::new(
            EItemInfo::simple_type(EDataType::String).into(),
            self.keys[input],
        ))
    }

    fn outputs_count(&self, _context: NodeContext) -> usize {
        1
    }

    fn output_unchecked(
        &self,
        _context: NodeContext,
        _output: usize,
    ) -> miette::Result<OutputData> {
        Ok(OutputData::new(
            EItemInfo::simple_type(EDataType::String).into(),
            "formatted".into(),
        ))
    }

    fn execute(
        &self,
        _context: NodeContext,
        inputs: &[EValue],
        outputs: &mut Vec<EValue>,
        _variables: &mut ExecutionExtras,
    ) -> miette::Result<ExecutionResult> {
        let Some(fmt) = &self.fmt else {
            bail!("Format not parsed");
        };
        let fields = FmtFields {
            values: inputs,
            variables: &self.keys,
        };

        let formatted = fmt.format_to_string(&fields).into_diagnostic()?;

        outputs.clear();
        outputs.push(EValue::String { value: formatted });

        Ok(ExecutionResult::Done)
    }
}

pub fn format_evalue_for_graph(value: &EValue) -> String {
    match value {
        EValue::Null => "null".to_string(),
        EValue::Boolean { value } => value.to_string(),
        EValue::Number { value } => value.to_string(),
        EValue::String { value } => value.to_string(),
        value => value.to_string(), // fallback to default display impl
    }
}

struct FmtFields<'a> {
    values: &'a [EValue],
    variables: &'a [Ustr],
}

impl FormatKeys for FmtFields<'_> {
    fn fmt(&self, key: &str, f: &mut Formatter<'_>) -> Result<(), FormatKeyError> {
        let Some(idx) = self.variables.iter().position(|i| i.as_str() == key) else {
            return Err(FormatKeyError::UnknownKey);
        };

        let value = format_evalue_for_graph(&self.values[idx]);

        write!(f, "{}", value).map_err(FormatKeyError::Fmt)
    }
}

#[derive(Debug, Clone)]
pub struct FormatNodeFactory;

impl NodeFactory for FormatNodeFactory {
    fn id(&self) -> Ustr {
        "string_fmt".into()
    }

    fn categories(&self) -> &'static [&'static str] {
        &["string"]
    }

    fn create(&self) -> Box<dyn Node> {
        Box::new(FormatNode {
            format: "".into(),
            keys: vec![],
            fmt: None,
        })
    }

    fn output_port_for(&self, ty: EDataType, _registry: &ETypesRegistry) -> Option<usize> {
        ty.is_string().then_some(0)
    }
}
