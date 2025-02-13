use crate::etype::eitem::EItemInfo;
use crate::etype::EDataType;
use crate::graph::node::commands::SnarlCommands;
use crate::graph::node::editable_state::EditableState;
use crate::graph::node::extras::ExecutionExtras;
use crate::graph::node::ports::fields::{map_inputs, sync_fields, FieldMapper, IoDirection};
use crate::graph::node::ports::{InputData, OutputData};
use crate::graph::node::{ExecutionResult, Node, NodeContext, NodeFactory};
use crate::registry::ETypesRegistry;
use crate::value::EValue;
use egui_snarl::NodeId;
use exmex::{Express, FlatEx};
use miette::{bail, IntoDiagnostic};
use serde::{Deserialize, Serialize};
use smallvec::smallvec;
use std::hash::{Hash, Hasher};
use ustr::{ustr, Ustr};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpressionNode {
    expression: String,
    variables: Vec<Ustr>,
    #[serde(skip)]
    expr: Option<FlatEx<f64>>,
}

impl Hash for ExpressionNode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.expression.hash(state);
        self.variables.hash(state);
    }
}

impl ExpressionNode {
    fn sync_fmt(&mut self) -> miette::Result<()> {
        match FlatEx::parse(&self.expression).into_diagnostic() {
            Ok(data) => {
                self.expr = Some(data);
            }
            Err(err) => {
                self.expr = None;
                return Err(err);
            }
        }
        Ok(())
    }

    fn sync_fields(&mut self, commands: &mut SnarlCommands, id: NodeId) -> miette::Result<()> {
        if self.expr.is_none() {
            self.sync_fmt()?;
        }
        let fmt = self
            .expr
            .as_ref()
            .expect("Expression should either be parsed or error should be returned");

        let fields = fmt.var_names();

        sync_fields(
            &ExpressionMapper,
            commands,
            fields,
            &mut self.variables,
            id,
            IoDirection::Input(0),
        );

        Ok(())
    }
}

struct ExpressionMapper;

impl FieldMapper for ExpressionMapper {
    type Field = String;
    type Local = Ustr;
    type Type = EDataType;

    fn matches(&self, field: &Self::Field, local: &Self::Local) -> bool {
        local == field
    }

    fn to_local(&self, field: &Self::Field) -> Self::Local {
        ustr(field)
    }

    fn field_type(&self, _field: &Self::Field) -> Self::Type {
        EDataType::Number
    }

    fn default_value(&self, _field: &Self::Field, _registry: &ETypesRegistry) -> EValue {
        0f64.into()
    }
}

impl Node for ExpressionNode {
    fn id(&self) -> Ustr {
        ExpressionNodeFactory.id()
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
            "expression".into(),
            EValue::String {
                value: self.expression.clone()
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

        self.expression = value;
        self.sync_fmt()?;
        self.sync_fields(commands, node_id)?;
        Ok(())
    }

    fn inputs_count(&self, _context: NodeContext) -> usize {
        self.variables.len()
    }

    fn input_unchecked(&self, _context: NodeContext, input: usize) -> miette::Result<InputData> {
        Ok(InputData::new(
            EItemInfo::simple_type(EDataType::Number).into(),
            self.variables[input],
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
            EItemInfo::simple_type(EDataType::Number).into(),
            "result".into(),
        ))
    }

    fn execute(
        &self,
        context: NodeContext,
        inputs: &[EValue],
        outputs: &mut Vec<EValue>,
        _variables: &mut ExecutionExtras,
    ) -> miette::Result<ExecutionResult> {
        let Some(expr) = &self.expr else {
            bail!("Expression not parsed");
        };

        let mut expr_inputs = Vec::with_capacity(expr.var_names().len());

        map_inputs(
            &ExpressionMapper,
            context.registry,
            expr.var_names(),
            &self.variables,
            inputs,
            &mut expr_inputs,
        )?;

        let result = expr
            .eval_iter(
                expr_inputs
                    .into_iter()
                    .map(|expr| expr.try_into_number().unwrap().0),
            )
            .into_diagnostic()?;

        outputs.clear();
        outputs.push(result.into());

        Ok(ExecutionResult::Done)
    }
}

#[derive(Debug, Clone)]
pub struct ExpressionNodeFactory;

impl NodeFactory for ExpressionNodeFactory {
    fn id(&self) -> Ustr {
        "expression".into()
    }

    fn categories(&self) -> &'static [&'static str] {
        &["math"]
    }

    fn create(&self) -> Box<dyn Node> {
        Box::new(ExpressionNode {
            expression: "".to_string(),
            variables: vec![],
            expr: None,
        })
    }

    fn output_port_for(&self, ty: EDataType, _registry: &ETypesRegistry) -> Option<usize> {
        ty.is_number().then_some(0)
    }
}
