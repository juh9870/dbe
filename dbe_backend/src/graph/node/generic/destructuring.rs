use crate::etype::eobject::EObject;
use crate::etype::EDataType;
use crate::graph::node::commands::{SnarlCommand, SnarlCommands};
use crate::graph::node::extras::ExecutionExtras;
use crate::graph::node::generic::{generic_try_connect, GenericNodeField, GenericNodeFieldMut};
use crate::graph::node::ports::fields::{get_field, sync_fields, IoDirection};
use crate::graph::node::ports::{InputData, NodePortType, OutputData};
use crate::graph::node::serde_node::impl_serde_node;
use crate::graph::node::struct_node::StructNodeFieldMapper;
use crate::graph::node::{ExecutionResult, Node, NodeContext, NodeFactory};
use crate::project::docs::DocsRef;
use crate::registry::ETypesRegistry;
use crate::value::id::ETypeId;
use crate::value::EValue;
use egui_snarl::{InPin, NodeId, OutPin, OutPinId};
use miette::{bail, miette};
use serde::{Deserialize, Serialize};
use std::ops::ControlFlow;
use ustr::Ustr;

#[derive(Debug, Clone, Hash, Default, Serialize, Deserialize)]
pub struct DestructuringNode {
    pub id: Option<ETypeId>,
    pub fields: Vec<Ustr>,
}

impl Node for DestructuringNode {
    impl_serde_node!();

    fn id(&self) -> Ustr {
        DestructuringNodeFactory.id()
    }

    fn title(&self, context: NodeContext) -> String {
        let Some(id) = self.id else {
            return "Destructuring".into();
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
            IoDirection::Output(0),
        );

        Ok(())
    }

    fn has_inline_values(&self, _input: usize) -> bool {
        false
    }

    fn inputs_count(&self, _context: NodeContext) -> usize {
        1
    }

    fn input_unchecked(&self, context: NodeContext, input: usize) -> miette::Result<InputData> {
        if input != 0 {
            bail!("Destructuring only has one input")
        }
        Ok(GenericNodeField::Object(&self.id).as_input_ty(context, "input"))
    }

    fn outputs_count(&self, context: NodeContext) -> usize {
        let Some(_) = self.id.and_then(|id| context.registry.get_struct(&id)) else {
            return 0;
        };

        self.fields.len()
    }

    fn output_unchecked(&self, context: NodeContext, output: usize) -> miette::Result<OutputData> {
        let Some(id) = self.id else {
            bail!("Struct id is not set");
        };
        let Some(data) = context.registry.get_struct(&id) else {
            bail!("Unknown struct `{}`", id);
        };

        let field = get_field(&StructNodeFieldMapper, &data.fields, &self.fields, output);
        if let Some(field) = field {
            Ok(OutputData::new(field.ty.clone().into(), field.name)
                .with_custom_docs(DocsRef::TypeField(id, field.name)))
        } else {
            Ok(OutputData::new(NodePortType::Invalid, self.fields[output]))
        }
    }

    fn try_connect(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        from: &OutPin,
        to: &InPin,
        incoming_type: &NodePortType,
    ) -> miette::Result<bool> {
        let changed = match generic_try_connect(
            context,
            to.id.input,
            incoming_type,
            &mut [GenericNodeFieldMut::Object(&mut self.id)],
        )? {
            ControlFlow::Continue(changed) => changed,
            ControlFlow::Break(_) => return Ok(false),
        };
        if changed {
            for (idx, _) in self.fields.iter().enumerate() {
                commands.push(SnarlCommand::DropOutputs {
                    from: OutPinId {
                        node: to.id.node,
                        output: idx,
                    },
                });
            }
            self.update_state(context, commands, to.id.node)?;
        }

        self._default_try_connect(context, commands, from, to, incoming_type)
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

        let EValue::Struct { ident, fields } = &inputs[0] else {
            bail!("expected struct, got `{}`", inputs[0].ty().name());
        };

        if *ident != id {
            bail!("expected struct `{}`, got `{}`", id, ident);
        }

        let data = context
            .registry
            .get_struct(&id)
            .ok_or_else(|| miette!("unknown struct `{}`", id))?;

        for field in &self.fields {
            let value = if let Some(value) = fields.get(field).cloned() {
                value
            } else if let Some(field_data) = data.fields.iter().find(|f| f.name == *field) {
                field_data.ty.default_value(context.registry).into_owned()
            } else {
                bail!("missing field `{}`", field);
            };

            outputs.push(value);
        }

        Ok(ExecutionResult::Done)
    }
}

#[derive(Debug, Clone)]
pub struct DestructuringNodeFactory;

impl NodeFactory for DestructuringNodeFactory {
    fn id(&self) -> Ustr {
        "destructuring".into()
    }

    fn categories(&self) -> &'static [&'static str] {
        &["objects"]
    }

    fn create(&self) -> Box<dyn Node> {
        Box::new(DestructuringNode::default())
    }

    fn input_port_for(&self, ty: EDataType, registry: &ETypesRegistry) -> Option<usize> {
        let EDataType::Object { ident } = ty else {
            return None;
        };
        registry.get_struct(&ident).is_some().then_some(0)
    }
}
