use crate::etype::EDataType;
use crate::graph::editing::GraphEditingContext;
use crate::graph::region::{RegionInfo, RegionVariable};
use egui_snarl::{InPinId, NodeId, OutPinId};
use itertools::Itertools;
use smallvec::SmallVec;
use tracing::debug;
use utils::map::HashMap;
use utils::vec_utils::VecOperation;
use uuid::Uuid;

#[derive(derive_more::Debug)]
pub enum SnarlCommand {
    /// Connects an output pin to an input pin
    Connect { from: OutPinId, to: InPinId },
    /// Disconnects an output pin from an input pin
    Disconnect { from: OutPinId, to: InPinId },
    /// Disconnects all connections coming from the pin
    DropOutputs { from: OutPinId },
    /// Disconnects all connections to the pin
    DropInputs { to: InPinId },
    /// Disconnects all connections coming from the node
    DropNodeOutputs { from: NodeId },
    /// Disconnects all connections to the node
    DropNodeInputs { to: NodeId },
    /// Deletes a node, running disconnection logic
    DeleteNode { node: NodeId },
    /// Disconnects and reconnects an output pin to an input pin, running the connected nodes' logic
    ///
    /// The most likely use case for this is when a node's output pin type
    /// changes, this allows to propagate the change and clean up the now invalid connections
    ReconnectOutput { id: OutPinId },
    /// Disconnects and reconnects all connection to the input pin, running the connected nodes' logic
    ///
    /// The most likely use case for this is when a node's input pin type
    /// changes, this allows to propagate the change and clean up the now invalid connections
    ReconnectInput { id: InPinId },
    /// Marks regions graph as dirty, requiring a rebuild
    RequireRegionRebuild,
    /// Removes the inline input value of the pin
    DeletePinValue { pin: InPinId },
    /// Connects an output pin to an input pin
    ///
    /// This command bypasses node logic, and should only be used by the node
    /// itself, after the connect logic has been done
    ConnectRaw { from: OutPinId, to: InPinId },
    /// Disconnects an output pin from an input pin
    ///
    /// This command bypasses node logic, and should only be used by the node
    /// itself, after the disconnect logic has been done
    DisconnectRaw { from: OutPinId, to: InPinId },
    /// Disconnects all connections to the pin
    ///
    /// This command bypasses node logic, and should only be used by the node
    /// itself, after the disconnect logic has been done
    DropInputsRaw { to: InPinId },
    // Is this ever a good idea?
    // /// Disconnects all connections coming from the pin
    // ///
    // /// This command bypasses node logic, and should only be used by the node
    // /// itself, after the disconnect logic has been done
    // DropOutputsRaw { from: OutPinId },
    /// Changes all connections to the input pin to point at the new pin
    ///
    /// This command bypasses node logic, and should only be used with caution
    InputMovedRaw { from: InPinId, to: InPinId },
    /// Changes all connections from the output pin to point at the new pin
    ///
    /// This command bypasses node logic, and should only be used with caution
    OutputMovedRaw { from: OutPinId, to: OutPinId },
    /// Changes all connections to the input pin to point at the new pins according to the new indices
    ///
    /// This command bypasses node logic, and should only be used with caution
    InputsRearrangedRaw {
        node: NodeId,
        indices: RearrangeIndices,
        offset: usize,
    },
    /// Changes all connections from the output pin to point according to the new indices
    ///
    /// This command bypasses node logic, and should only be used with caution
    OutputsRearrangedRaw {
        node: NodeId,
        indices: RearrangeIndices,
        offset: usize,
    },
    /// Sets the group input type. The command will panic if the input already has a type
    SetGroupInputType { id: Uuid, ty: EDataType },
    /// Sets the group output type. The command will panic if the output already has a type
    SetGroupOutputType { id: Uuid, ty: EDataType },
    /// Edits region variables using the provided operation
    EditRegionVariables {
        region: Uuid,
        operation: VecOperation<RegionVariable>,
    },
    /// Runs a custom callback on the graph and execution context
    Custom {
        #[debug("fn()")]
        cb: CustomCommand,
    },
}

pub type RearrangeIndices = SmallVec<[usize; 4]>;

type CustomCommand = Box<dyn FnOnce(&mut GraphEditingContext) -> miette::Result<()>>;

impl SnarlCommand {
    pub fn execute(
        self,
        ctx: &mut GraphEditingContext,
        commands: &mut SnarlCommands,
    ) -> miette::Result<()> {
        match self {
            SnarlCommand::DisconnectRaw { from, to } => {
                ctx.snarl.disconnect(from, to);
                ctx.mark_dirty();
            }
            SnarlCommand::ConnectRaw { from, to } => {
                ctx.snarl.connect(from, to);
                ctx.mark_dirty();
            }
            SnarlCommand::InputMovedRaw { from, to } => {
                // debug!("Moving input from {:?} to {:?}", from, to);
                if let Some(value) = ctx.inline_values.remove(&from) {
                    ctx.inline_values.insert(to, value);
                } else {
                    ctx.inline_values.remove(&to);
                }
                let pin = ctx.snarl.in_pin(from);
                ctx.snarl.drop_inputs(from);
                for remote in pin.remotes {
                    ctx.snarl.connect(remote, to);
                }
                if from.node != to.node {
                    ctx.mark_dirty();
                }
            }
            SnarlCommand::OutputMovedRaw { from, to } => {
                let pin = ctx.snarl.out_pin(from);
                ctx.snarl.drop_outputs(from);
                for remote in pin.remotes {
                    ctx.snarl.connect(to, remote);
                }
                if from.node != to.node {
                    ctx.mark_dirty();
                }
            }
            SnarlCommand::InputsRearrangedRaw {
                node,
                indices,
                offset,
            } => {
                // debug!("Rearranging inputs for node {:?}: {:#?}", node, indices);
                let node_pins = ctx
                    .snarl
                    .wires()
                    .filter(|(_, in_pin)| in_pin.node == node)
                    .collect_vec();

                let mut inline_values = ctx
                    .inline_values
                    .iter()
                    .filter(|(pin, _)| pin.node == node)
                    .map(|(pin, value)| (*pin, value.clone()))
                    .collect::<HashMap<_, _>>();

                for (i, target) in indices.iter().copied().enumerate() {
                    if target == i {
                        continue;
                    }

                    let old_pin = InPinId {
                        node,
                        input: i + offset,
                    };

                    ctx.inline_values.remove(&old_pin);

                    ctx.snarl.drop_inputs(old_pin);
                }

                for (i, target) in indices.into_iter().enumerate() {
                    if target == i {
                        continue;
                    }

                    let old_pin = InPinId {
                        node,
                        input: i + offset,
                    };
                    let new_pin = InPinId {
                        node,
                        input: target + offset,
                    };

                    for (source, _) in node_pins.iter().filter(|(_, i)| i == &old_pin) {
                        ctx.snarl.connect(*source, new_pin);
                    }

                    if let Some(inline) = inline_values.remove(&old_pin) {
                        ctx.inline_values.insert(new_pin, inline);
                    } else {
                        ctx.inline_values.remove(&new_pin);
                    }
                }
            }
            SnarlCommand::OutputsRearrangedRaw {
                node,
                indices,
                offset,
            } => {
                let node_pins = ctx
                    .snarl
                    .wires()
                    .filter(|(out_pin, _)| out_pin.node == node)
                    .collect_vec();

                for (i, target) in indices.iter().copied().enumerate() {
                    if target == i {
                        continue;
                    }

                    ctx.snarl.drop_outputs(OutPinId {
                        node,
                        output: i + offset,
                    });
                }

                for (i, target) in indices.into_iter().enumerate() {
                    if target == i {
                        continue;
                    }

                    let old_pin = OutPinId {
                        node,
                        output: i + offset,
                    };
                    let new_pin = OutPinId {
                        node,
                        output: target + offset,
                    };

                    for (_, target_pin) in node_pins.iter().filter(|(i, _)| i == &old_pin) {
                        ctx.snarl.connect(new_pin, *target_pin);
                    }
                }
            }
            SnarlCommand::Custom { cb } => {
                cb(ctx)?;
            }
            SnarlCommand::ReconnectOutput { id } => {
                for pin in ctx.snarl.out_pin(id).remotes {
                    SnarlCommand::Disconnect { from: id, to: pin }.execute(ctx, commands)?;
                    SnarlCommand::Connect { from: id, to: pin }.execute(ctx, commands)?;
                }
            }
            SnarlCommand::ReconnectInput { id } => {
                for pin in ctx.snarl.in_pin(id).remotes {
                    SnarlCommand::Disconnect { from: pin, to: id }.execute(ctx, commands)?;
                    SnarlCommand::Connect { from: pin, to: id }.execute(ctx, commands)?;
                }
            }
            SnarlCommand::Connect { from, to } => {
                let from = ctx.snarl.out_pin(from);
                let to = ctx.snarl.in_pin(to);
                ctx.connect(&from, &to, commands)?;
            }
            SnarlCommand::Disconnect { from, to } => {
                let from = ctx.snarl.out_pin(from);
                let to = ctx.snarl.in_pin(to);
                ctx.disconnect(&from, &to, commands)?;
            }
            SnarlCommand::DropInputsRaw { to } => {
                ctx.snarl.drop_inputs(to);
                ctx.mark_dirty();
            }
            // SnarlCommand::DropOutputsRaw { from } => {
            //     for pin in ctx.snarl.out_pin(from).remotes {
            //         ctx.mark_dirty(pin.node);
            //     }
            //     ctx.snarl.drop_outputs(from);
            // }
            SnarlCommand::DeletePinValue { pin } => {
                ctx.inline_values.remove(&pin);
            }
            SnarlCommand::DropOutputs { from } => {
                for pin in ctx.snarl.out_pin(from).remotes {
                    SnarlCommand::Disconnect { from, to: pin }.execute(ctx, commands)?;
                }
            }
            SnarlCommand::DropInputs { to } => {
                for pin in ctx.snarl.in_pin(to).remotes {
                    SnarlCommand::Disconnect { from: pin, to }.execute(ctx, commands)?;
                }
            }
            SnarlCommand::DropNodeOutputs { from } => {
                for pin in ctx.snarl.wires().filter(|w| w.0.node == from).collect_vec() {
                    SnarlCommand::Disconnect {
                        from: pin.0,
                        to: pin.1,
                    }
                    .execute(ctx, commands)?;
                }
            }
            SnarlCommand::DropNodeInputs { to } => {
                for pin in ctx.snarl.wires().filter(|w| w.1.node == to).collect_vec() {
                    SnarlCommand::Disconnect {
                        from: pin.0,
                        to: pin.1,
                    }
                    .execute(ctx, commands)?;
                }
            }
            SnarlCommand::DeleteNode { node } => {
                ctx.inline_values.retain(|pin, _| pin.node != node);
                // Disconnect all outputs
                for (out_pin, in_pin) in
                    ctx.snarl.wires().filter(|w| w.0.node == node).collect_vec()
                {
                    SnarlCommand::Disconnect {
                        from: out_pin,
                        to: in_pin,
                    }
                    .execute(ctx, commands)?;
                }
                // Disconnect all inputs
                for (out_pin, in_pin) in
                    ctx.snarl.wires().filter(|w| w.1.node == node).collect_vec()
                {
                    SnarlCommand::Disconnect {
                        from: out_pin,
                        to: in_pin,
                    }
                    .execute(ctx, commands)?;
                }
                ctx.snarl.remove_node(node);
                ctx.mark_dirty();
            }
            SnarlCommand::SetGroupInputType { ty, id } => {
                let Some(input) = ctx.inputs.iter_mut().find(|i| i.id == id) else {
                    panic!("Input {} not found", id);
                };

                if input.ty.is_some() {
                    panic!("Input `{}` ({}) already has a type", input.name, id);
                }

                input.ty = Some(ty);
            }
            SnarlCommand::SetGroupOutputType { ty, id } => {
                let Some(output) = ctx.outputs.iter_mut().find(|o| o.id == id) else {
                    panic!("Output {} not found", id);
                };

                if output.ty.is_some() {
                    panic!("Output `{}` ({}) already has a type", output.name, id);
                }

                output.ty = Some(ty);
            }
            SnarlCommand::RequireRegionRebuild => {
                ctx.mark_dirty();
            }
            SnarlCommand::EditRegionVariables { region, operation } => {
                let vars = &mut ctx
                    .regions
                    .entry(region)
                    .or_insert_with(|| RegionInfo::new(region))
                    .variables;

                operation.apply(vars);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct SnarlCommands {
    commands: Vec<SnarlCommand>,
}

impl SnarlCommands {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, command: SnarlCommand) {
        self.commands.push(command);
    }

    pub fn execute(&mut self, ctx: &mut GraphEditingContext) -> miette::Result<()> {
        let mut iter = 0;
        while !self.commands.is_empty() {
            iter += 1;
            if iter > 1000 {
                panic!("Node commands formed an infinite loop");
            }
            let mut commands = std::mem::take(&mut self.commands);
            for command in commands.drain(..) {
                command.execute(ctx, self)?;
            }
            if self.commands.is_empty() {
                self.commands = commands;
            }
        }
        Ok(())
    }
}
