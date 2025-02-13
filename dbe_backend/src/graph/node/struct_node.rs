use crate::etype::eobject::EObject;
use crate::etype::estruct::EStructField;
use crate::graph::node::commands::{SnarlCommand, SnarlCommands};
use crate::graph::node::generic::{
    generic_can_output_to, generic_connected_to_output, GenericNodeField, GenericNodeFieldMut,
};
use crate::graph::node::ports::fields::{
    get_field, map_inputs, sync_fields, FieldMapper, IoDirection,
};
use crate::graph::node::ports::NodePortType;
use crate::graph::node::serde_node::impl_serde_node;
use crate::graph::node::{
    ExecutionExtras, ExecutionResult, InputData, Node, NodeContext, NodeFactory, OutputData,
};
use crate::project::docs::DocsRef;
use crate::value::id::ETypeId;
use crate::value::EValue;
use egui_snarl::{InPin, InPinId, NodeId, OutPin};
use miette::{bail, miette};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ustr::Ustr;

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct StructNode {
    pub id: Option<ETypeId>,
    #[serde(default)]
    pub fields: Vec<Ustr>,
}

pub struct StructNodeFieldMapper;

impl FieldMapper for StructNodeFieldMapper {
    type Field = EStructField;
    type Local = Ustr;
    type Type = ();

    fn matches(&self, field: &Self::Field, local: &Self::Local) -> bool {
        &field.name == local
    }

    fn to_local(&self, field: &Self::Field) -> Self::Local {
        field.name
    }
}

impl StructNode {
    pub fn new(id: ETypeId) -> Self {
        Self {
            id: Some(id),
            fields: vec![],
        }
    }

    pub fn from_value(value: &EValue) -> miette::Result<Self> {
        let EValue::Struct { fields, ident } = value else {
            bail!("Expected struct value");
        };

        Ok(Self {
            id: Some(*ident),
            fields: fields.keys().cloned().collect(),
        })
    }
}

impl Node for StructNode {
    impl_serde_node!();

    fn id(&self) -> Ustr {
        "struct_node".into()
    }

    fn title(&self, context: NodeContext) -> String {
        let Some(id) = self.id else {
            return "Struct".into();
        };
        let Some(data) = context.registry.get_struct(&id) else {
            return format!("Unknown struct `{}`", id);
        };

        data.title(context.registry)
    }

    fn update_state(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        id: NodeId,
    ) -> miette::Result<()> {
        let Some(struct_id) = self.id else {
            return Ok(());
        };
        let Some(data) = context.registry.get_struct(&struct_id) else {
            return Ok(());
        };

        sync_fields(
            &StructNodeFieldMapper,
            commands,
            &data.fields,
            &mut self.fields,
            id,
            IoDirection::Input(0),
        );

        Ok(())
    }

    fn inputs_count(&self, context: NodeContext) -> usize {
        let Some(_) = self.id.and_then(|id| context.registry.get_struct(&id)) else {
            return 0;
        };

        self.fields.len()
    }

    fn input_unchecked(&self, context: NodeContext, input: usize) -> miette::Result<InputData> {
        let Some(id) = self.id else {
            bail!("Struct id is not set");
        };
        let Some(data) = context.registry.get_struct(&id) else {
            bail!("Unknown struct `{}`", id);
        };

        let field = get_field(&StructNodeFieldMapper, &data.fields, &self.fields, input);
        if let Some(field) = field {
            Ok(InputData::new(field.ty.clone().into(), field.name)
                .with_custom_docs(DocsRef::TypeField(id, field.name)))
        } else {
            Ok(InputData::new(NodePortType::Invalid, self.fields[input]))
        }
    }

    fn outputs_count(&self, _context: NodeContext) -> usize {
        1
    }

    fn output_unchecked(&self, context: NodeContext, output: usize) -> miette::Result<OutputData> {
        if output != 0 {
            bail!("Destructuring only has one output")
        }
        Ok(GenericNodeField::Object(&self.id).as_output_ty(context, "output"))
    }

    fn can_output_to(
        &self,
        context: NodeContext,
        from: &OutPin,
        _to: &InPin,
        target_type: &NodePortType,
    ) -> miette::Result<bool> {
        generic_can_output_to(
            context,
            from.id.output,
            target_type,
            &[GenericNodeField::Object(&self.id)],
        )
    }

    fn connected_to_output(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        from: &OutPin,
        _to: &InPin,
        incoming_type: &NodePortType,
    ) -> miette::Result<()> {
        let changed = generic_connected_to_output(
            context,
            from.id.output,
            incoming_type,
            &mut [GenericNodeFieldMut::Object(&mut self.id)],
        )?;

        if changed {
            for (idx, _) in self.fields.iter().enumerate() {
                commands.push(SnarlCommand::DropInputs {
                    to: InPinId {
                        node: from.id.node,
                        input: idx,
                    },
                });
            }
            self.update_state(context, commands, from.id.node)?;
        };

        Ok(())
    }

    fn execute(
        &self,
        context: NodeContext,
        inputs: &[EValue],
        outputs: &mut Vec<EValue>,
        _variables: &mut ExecutionExtras,
    ) -> miette::Result<ExecutionResult> {
        let Some(id) = self.id else {
            return Ok(ExecutionResult::Done);
        };
        let data = context
            .registry
            .get_struct(&id)
            .ok_or_else(|| miette!("unknown struct `{}`", id))?;

        let mut struct_fields = vec![];

        map_inputs(
            &StructNodeFieldMapper,
            context.registry,
            &data.fields,
            &self.fields,
            inputs,
            &mut struct_fields,
        )?;

        let mut fields = BTreeMap::default();

        for (i, field) in data.fields.iter().enumerate() {
            // assert_eq!(inputs[i].ty(), field.ty.ty());
            fields.insert(field.name, struct_fields[i].clone());
        }

        outputs.clear();
        outputs.push(EValue::Struct { fields, ident: id });

        Ok(ExecutionResult::Done)
    }
}

#[derive(Debug, Clone)]
pub struct StructNodeFactory;

impl NodeFactory for StructNodeFactory {
    fn id(&self) -> Ustr {
        "struct_node".into()
    }

    fn categories(&self) -> &'static [&'static str] {
        &["objects"]
    }

    fn create(&self) -> Box<dyn Node> {
        Box::new(StructNode {
            id: None,
            fields: vec![],
        })
    }
}
