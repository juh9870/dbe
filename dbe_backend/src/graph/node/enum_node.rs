use crate::etype::eenum::variant::{EEnumVariant, EEnumVariantId};
use crate::etype::eenum::EEnumData;
use crate::etype::eitem::EItemInfo;
use crate::etype::eobject::EObject;
use crate::etype::EDataType;
use crate::graph::node::commands::{SnarlCommand, SnarlCommands};
use crate::graph::node::editable_state::{EditableState, EditableStateValue};
use crate::graph::node::extras::ExecutionExtras;
use crate::graph::node::ports::NodePortType;
use crate::graph::node::serde_node::impl_serde_node;
use crate::graph::node::{ExecutionResult, InputData, Node, NodeContext, NodeFactory, OutputData};
use crate::project::docs::DocsRef;
use crate::registry::ETypesRegistry;
use crate::value::EValue;
use egui_snarl::{InPin, InPinId, NodeId, OutPin};
use miette::bail;
use serde::{Deserialize, Serialize};
use smallvec::smallvec;
use ustr::{ustr, Ustr};
use utils::whatever_ref::{WhateverRef, WhateverRefMap};

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct EnumNode {
    variant: Option<EEnumVariantId>,
}

impl EnumNode {
    pub fn new(variant: EEnumVariantId) -> Self {
        Self {
            variant: Some(variant),
        }
    }

    pub fn from_value(value: &EValue) -> miette::Result<Self> {
        let EValue::Enum { variant, .. } = value else {
            bail!("Expected enum value");
        };

        Ok(Self {
            variant: Some(*variant),
        })
    }

    fn get_data<'a>(
        &self,
        registry: &'a ETypesRegistry,
    ) -> Option<(
        WhateverRef<'a, EEnumData>,
        WhateverRefMap<'a, EEnumData, EEnumVariant>,
    )> {
        let variant = self.variant.as_ref()?;

        variant.enum_variant(registry)
    }

    pub fn variant(&self) -> EEnumVariantId {
        self.variant
            .expect("Variant should be set before using EnumNode")
    }

    pub fn set_variant(
        &mut self,
        commands: &mut SnarlCommands,
        node: NodeId,
        variant: EEnumVariantId,
    ) -> miette::Result<()> {
        if variant == self.variant() {
            return Ok(());
        }

        self.variant = Some(variant);
        commands.push(SnarlCommand::DropInputsRaw {
            to: InPinId { node, input: 0 },
        });
        commands.push(SnarlCommand::DeletePinValue {
            pin: InPinId { node, input: 0 },
        });

        Ok(())
    }
}

impl Node for EnumNode {
    impl_serde_node!();
    fn id(&self) -> Ustr {
        EnumNodeFactory.id()
    }

    fn title(&self, context: NodeContext) -> String {
        let Some((data, _variant)) = self.get_data(context.registry) else {
            return "Unknown enum variant".into();
        };

        data.title(context.registry)
    }

    fn has_editable_state(&self) -> bool {
        true
    }

    fn editable_state(&self) -> EditableState {
        smallvec![(
            ustr("variant"),
            EditableStateValue::EnumVariant(self.variant())
        )]
    }

    fn apply_editable_state(
        &mut self,
        _context: NodeContext,
        state: EditableState,
        commands: &mut SnarlCommands,
        node_id: NodeId,
    ) -> miette::Result<()> {
        let value = state[0].1.try_as_enum_variant_ref().unwrap();
        self.set_variant(commands, node_id, *value)
    }

    fn inputs_count(&self, context: NodeContext) -> usize {
        let Some((_, _)) = self.get_data(context.registry) else {
            return 0;
        };
        1
    }

    fn input_unchecked(&self, context: NodeContext, input: usize) -> miette::Result<InputData> {
        let Some((enum_data, variant)) = self.get_data(context.registry) else {
            panic!("Unknown enum variant");
        };
        if input != 0 {
            panic!("Enum only has one input");
        }
        Ok(InputData::new(variant.data.clone().into(), variant.name)
            .with_custom_docs(DocsRef::EnumVariant(enum_data.ident, variant.name)))
    }

    fn outputs_count(&self, context: NodeContext) -> usize {
        let Some((_, _)) = self.get_data(context.registry) else {
            return 0;
        };
        1
    }

    fn output_unchecked(&self, context: NodeContext, output: usize) -> miette::Result<OutputData> {
        let Some((data, _)) = self.get_data(context.registry) else {
            panic!("Unknown enum variant");
        };

        if output != 0 {
            panic!("Enum only has one output");
        }

        Ok(OutputData::new(
            EItemInfo::simple_type(EDataType::Object { ident: data.ident }).into(),
            "output".into(),
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
        let Some((data, _)) = self.get_data(context.registry) else {
            panic!("Unknown enum variant");
        };

        if to.id.input != 0 {
            panic!("Enum only has one input");
        }

        for (variant, id) in data.variants_with_ids() {
            if variant.data.ty() == incoming_type.ty() {
                self.set_variant(commands, to.id.node, *id)?;
                break;
            }
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
        let Some((_, _)) = self.get_data(context.registry) else {
            panic!("Unknown enum variant");
        };

        assert_eq!(inputs.len(), 1);
        // assert_eq!(inputs[0].ty(), variant.data.ty());

        outputs.clear();
        outputs.push(EValue::Enum {
            variant: self.variant(),
            data: Box::new(inputs[0].clone()),
        });

        Ok(ExecutionResult::Done)
    }
}

#[derive(Debug, Clone)]
pub struct EnumNodeFactory;

impl NodeFactory for EnumNodeFactory {
    fn id(&self) -> Ustr {
        "enum_node".into()
    }

    fn categories(&self) -> &'static [&'static str] {
        &[]
    }

    fn create(&self) -> Box<dyn Node> {
        Box::new(EnumNode { variant: None })
    }
}
