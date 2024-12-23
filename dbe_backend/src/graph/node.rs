use crate::etype::default::DefaultEValue;
use crate::graph::inputs::{GraphInput, GraphOutput};
use crate::graph::node::commands::{SnarlCommand, SnarlCommands};
use crate::graph::node::enum_node::EnumNodeFactory;
use crate::graph::node::functional::functional_nodes;
use crate::graph::node::list::ListNodeFactory;
use crate::graph::node::ports::{InputData, NodePortType, OutputData};
use crate::graph::node::reroute::RerouteFactory;
use crate::graph::node::saving_node::SavingNodeFactory;
use crate::graph::node::struct_node::StructNodeFactory;
use crate::json_utils::JsonValue;
use crate::project::project_graph::ProjectGraphs;
use crate::registry::ETypesRegistry;
use crate::value::EValue;
use atomic_refcell::{AtomicRef, AtomicRefCell};
use downcast_rs::{impl_downcast, Downcast};
use dyn_clone::DynClone;
use egui_snarl::{InPin, NodeId, OutPin};
use miette::bail;
use smallvec::SmallVec;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::sync::{Arc, LazyLock};
use ustr::{Ustr, UstrMap};

pub mod commands;
pub mod enum_node;
pub mod functional;
pub mod groups;
pub mod list;
pub mod ports;
pub mod reroute;
pub mod saving_node;
pub mod struct_node;
pub mod variables;

static NODE_FACTORIES: LazyLock<AtomicRefCell<UstrMap<Arc<dyn NodeFactory>>>> =
    LazyLock::new(|| AtomicRefCell::new(default_nodes().collect()));

type FactoriesByCategory = BTreeMap<&'static str, Vec<Arc<dyn NodeFactory>>>;
static NODE_FACTORIES_BY_CATEGORY: LazyLock<AtomicRefCell<FactoriesByCategory>> =
    LazyLock::new(|| {
        AtomicRefCell::new({
            let mut map: BTreeMap<&str, Vec<Arc<dyn NodeFactory>>> = BTreeMap::new();
            for (_, fac) in default_nodes() {
                for cat in fac.categories() {
                    map.entry(*cat).or_default().push(fac.clone());
                }
            }
            map
        })
    });

fn default_nodes() -> impl Iterator<Item = (Ustr, Arc<dyn NodeFactory>)> {
    let mut v: Vec<Arc<dyn NodeFactory>> = functional_nodes();
    v.push(Arc::new(RerouteFactory));
    v.push(Arc::new(StructNodeFactory));
    v.push(Arc::new(EnumNodeFactory));
    v.push(Arc::new(SavingNodeFactory));
    v.push(Arc::new(ListNodeFactory));
    v.push(Arc::new(GroupOutputNodeFactory));
    v.push(Arc::new(GroupInputNodeFactory));
    v.push(Arc::new(SubgraphNodeFactory));
    v.into_iter().map(|item| (Ustr::from(&item.id()), item))
}

pub fn get_snarl_node(id: &Ustr) -> Option<SnarlNode> {
    NODE_FACTORIES.borrow().get(id).map(|f| f.create())
}

pub fn all_node_factories() -> AtomicRef<'static, UstrMap<Arc<dyn NodeFactory>>> {
    NODE_FACTORIES.borrow()
}

pub fn node_factories_by_category() -> AtomicRef<'static, FactoriesByCategory> {
    NODE_FACTORIES_BY_CATEGORY.borrow()
}

pub trait NodeFactory: Send + Sync + Debug + 'static {
    fn id(&self) -> Ustr;
    fn categories(&self) -> &'static [&'static str];
    fn create(&self) -> SnarlNode;
}

#[derive(Debug, Copy, Clone)]
pub struct NodeContext<'a> {
    pub registry: &'a ETypesRegistry,
    pub inputs: &'a SmallVec<[GraphInput; 1]>,
    pub outputs: &'a SmallVec<[GraphOutput; 1]>,
    pub graphs: Option<&'a ProjectGraphs>,
}

pub type SnarlNode = Box<dyn Node>;

pub trait Node: DynClone + Debug + Send + Sync + Downcast + 'static {
    /// Writes node state to json
    fn write_json(&self, registry: &ETypesRegistry) -> miette::Result<JsonValue> {
        let _ = (registry,);
        Ok(JsonValue::Null)
    }
    /// Loads node state from json
    fn parse_json(
        &mut self,
        registry: &ETypesRegistry,
        value: &mut JsonValue,
    ) -> miette::Result<()> {
        let _ = (registry, value);
        Ok(())
    }

    fn id(&self) -> Ustr;

    fn default_input_value(
        &self,
        context: NodeContext,
        input: usize,
    ) -> miette::Result<DefaultEValue> {
        let input = self.try_input(context, input)?;
        Ok(input.ty.default_value(context.registry))
    }

    fn title(&self, context: NodeContext) -> String {
        let _ = (context,);
        self.id().to_string()
    }

    /// Updates internal state of the node
    ///
    /// Nodes should not depend on this method ever getting called and expected
    /// to work without it
    ///
    /// Nodes should generally should only use this method for optimization or
    /// user presentation reasons
    fn update_state(&mut self, context: NodeContext, commands: &mut SnarlCommands, id: NodeId) {
        let _ = (context, commands, id);
    }

    /// Determines if the node has inline editable values
    fn has_inline_values(&self) -> miette::Result<bool> {
        Ok(true)
    }

    /// Node inputs
    fn inputs_count(&self, context: NodeContext) -> usize;

    /// Returns the type of the input pin
    /// # Panics
    /// This method panics if the input index is out of bounds
    fn input_unchecked(&self, context: NodeContext, input: usize) -> miette::Result<InputData>;

    /// Node outputs
    fn outputs_count(&self, context: NodeContext) -> usize;

    /// Returns the type of the output pin
    /// # Panics
    /// This method panics if the input index is out of bounds
    fn output_unchecked(&self, context: NodeContext, output: usize) -> miette::Result<OutputData>;

    fn try_input(&self, context: NodeContext, input: usize) -> miette::Result<InputData> {
        let count = self.inputs_count(context);
        if input >= count {
            bail!("input index {} out of bounds (length {})", input, count)
        } else {
            self.input_unchecked(context, input)
        }
    }

    fn try_output(&self, context: NodeContext, output: usize) -> miette::Result<OutputData> {
        let count = self.outputs_count(context);
        if output >= count {
            bail!("output index {} out of bounds (length {})", output, count)
        } else {
            self.output_unchecked(context, output)
        }
    }

    /// Attempts to create a connection to the input pin of the node
    /// Returns true if the connection can be made
    ///
    /// On success, boolean value should be true and the connection was established
    ///
    /// Nodes may mutate their internal state when a connection is made
    fn try_connect(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        from: &OutPin,
        to: &InPin,
        incoming_type: &NodePortType,
    ) -> miette::Result<bool> {
        self._default_try_connect(context, commands, from, to, incoming_type)
    }

    /// Disconnect the input pin of the node
    ///
    /// Note that the output type of the `from` pin is not guaranteed to be
    /// compatible with the input type of the `to` pin. For example, the drop
    /// might be caused by the source node changing its output type, which
    /// happens before the disconnection is processed.
    ///
    /// On success, the provided connection should no longer exist after executing emitted commands
    fn try_disconnect(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        from: &OutPin,
        to: &InPin,
    ) -> miette::Result<()> {
        self._default_try_disconnect(context, commands, from, to)
    }

    /// Custom logic for checking if the node can output to the given port
    ///
    /// Only called if the corresponding output has type [NodePortType::BasedOnTarget]
    fn can_output_to(
        &self,
        context: NodeContext,
        from: &OutPin,
        to: &InPin,
        target_type: &NodePortType,
    ) -> miette::Result<bool> {
        let _ = (context, from, to, target_type);
        unimplemented!("Node::can_output_to")
    }

    /// Custom logic to be run after the output is connected to some input
    ///
    /// Only called if the corresponding output has type [NodePortType::BasedOnTarget]
    fn connected_to_output(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        from: &OutPin,
        to: &InPin,
        incoming_type: &NodePortType,
    ) -> miette::Result<()> {
        let _ = (context, commands, from, to, incoming_type);
        unimplemented!("Node::can_output_to")
    }

    /// Whenever the node has side effects and must be executed
    fn has_side_effects(&self) -> bool {
        false
    }

    /// Execute the node
    fn execute(
        &self,
        context: NodeContext,
        inputs: &[EValue],
        outputs: &mut Vec<EValue>,
        variables: &mut ExecutionExtras,
    ) -> miette::Result<()>;

    fn _default_try_connect(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        from: &OutPin,
        to: &InPin,
        incoming_type: &NodePortType,
    ) -> miette::Result<bool> {
        let ty = self.try_input(context, to.id.input)?;
        if NodePortType::compatible(context.registry, incoming_type, &ty.ty) {
            // TODO: support for multi-connect ports
            if !to.remotes.is_empty() {
                commands.push(SnarlCommand::DropInputsRaw { to: to.id });
            }

            commands.push(SnarlCommand::ConnectRaw {
                from: from.id,
                to: to.id,
            });

            return Ok(true);
        }

        Ok(false)
    }

    fn _default_try_disconnect(
        &mut self,
        context: NodeContext,
        commands: &mut SnarlCommands,
        from: &OutPin,
        to: &InPin,
    ) -> miette::Result<()> {
        let _ = (context,);
        commands.push(SnarlCommand::DisconnectRaw {
            from: from.id,
            to: to.id,
        });
        Ok(())
    }
}

impl_downcast!(Node);

/// Implements write_json and parse_json for the node by serializing whole node struct via serde
macro_rules! impl_serde_node {
    () => {
        fn write_json(
            &self,
            _registry: &ETypesRegistry,
        ) -> miette::Result<$crate::json_utils::JsonValue> {
            miette::IntoDiagnostic::into_diagnostic(serde_json::value::to_value(&self))
        }

        fn parse_json(
            &mut self,
            _registry: &ETypesRegistry,
            value: &mut $crate::json_utils::JsonValue,
        ) -> miette::Result<()> {
            miette::IntoDiagnostic::into_diagnostic(Self::deserialize(value.take()))
                .map(|node| *self = node)
        }
    };
}

use crate::graph::node::groups::input::GroupInputNodeFactory;
use crate::graph::node::groups::output::GroupOutputNodeFactory;
use crate::graph::node::groups::subgraph::SubgraphNodeFactory;
use crate::graph::node::variables::ExecutionExtras;
pub(crate) use impl_serde_node;
