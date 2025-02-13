use crate::graph::editing::GraphEditingContext;
use crate::graph::node::commands::SnarlCommands;
use crate::graph::node::get_node_factory;
use crate::value::id::{EListId, ETypeId};
use egui_snarl::{InPinId, NodeId, OutPinId};
use emath::Pos2;
use miette::{bail, miette};
use smallvec::SmallVec;
use std::borrow::Cow;
use ustr::Ustr;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum NodeCombo {
    Factory(Ustr),
    Subgraph(Uuid, String),
    Object(ETypeId, String),
    List(EListId),
}

impl NodeCombo {
    /// Formats the node for display in search results
    pub fn display_search(&self) -> Cow<str> {
        match self {
            NodeCombo::Object(id, title) => format!("{} ({})", title, id.as_raw().unwrap()).into(),
            NodeCombo::Subgraph(_, name) => name.into(),
            _ => self.display_title(),
        }
    }

    /// Formats the node for display in the file tree or other places where ID
    /// is unnecessary
    pub fn display_title(&self) -> Cow<str> {
        match self {
            NodeCombo::Factory(id) => id.as_str().into(),
            NodeCombo::Object(_, title) => title.into(),
            NodeCombo::Subgraph(_, name) => name.into(),
            NodeCombo::List(id) => id.as_raw().unwrap().into(),
        }
    }

    pub fn create(
        &self,
        ctx: &mut GraphEditingContext,
        pos: Pos2,
    ) -> miette::Result<SmallVec<[NodeId; 2]>> {
        match self {
            NodeCombo::Factory(id) => ctx.create_node(*id, pos),
            NodeCombo::Object(id, _) => ctx.create_object_node(*id, pos, None),
            NodeCombo::Subgraph(id, _) => ctx.create_subgraph_node(*id, pos),
            NodeCombo::List(id) => ctx.create_list_node(*id, pos),
        }
    }

    pub fn create_from_input_pin(
        &self,
        ctx: &mut GraphEditingContext,
        pos: Pos2,
        pin: &InPinId,
        commands: &mut SnarlCommands,
    ) -> miette::Result<SmallVec<[NodeId; 2]>> {
        let in_pin = ctx.snarl.in_pin(*pin);
        let mut port_id = 0;
        let nodes = match self {
            NodeCombo::Factory(id) => {
                let factory = get_node_factory(id).unwrap();
                let in_data = ctx.snarl[pin.node].try_input(ctx.as_node_context(), pin.input)?;
                port_id = factory
                    .input_port_for(in_data.ty.ty(), ctx.registry)
                    .ok_or_else(|| {
                        miette!(
                            "Node {} does not have an output port for type {}",
                            id,
                            in_data.ty.ty().name()
                        )
                    })?;
                ctx.create_node(*id, pos)?
            }
            NodeCombo::Subgraph(id, _) => ctx.create_subgraph_node(*id, pos)?,
            NodeCombo::Object(ident, _) => {
                let inline_value = ctx.inline_values.remove(pin);
                ctx.create_object_node(*ident, pos, inline_value)?
            }
            NodeCombo::List(id) => ctx.create_list_node(*id, pos)?,
        };
        if let Some(node_id) = nodes.last() {
            let out_pin = ctx.snarl.out_pin(OutPinId {
                node: *node_id,
                output: port_id,
            });
            if !ctx.connect(&out_pin, &in_pin, commands)? {
                bail!("Failed to connect dragged-out pins");
            }
        }

        Ok(nodes)
    }
    pub fn create_from_output_pin(
        &self,
        ctx: &mut GraphEditingContext,
        pos: Pos2,
        pin: &OutPinId,
        commands: &mut SnarlCommands,
    ) -> miette::Result<SmallVec<[NodeId; 2]>> {
        let out_pin = ctx.snarl.out_pin(*pin);
        let mut port_id = 0;
        let nodes = match self {
            NodeCombo::Factory(id) => {
                let factory = get_node_factory(id).unwrap();
                let out_data = ctx.snarl[pin.node].try_output(ctx.as_node_context(), pin.output)?;
                port_id = factory
                    .input_port_for(out_data.ty.ty(), ctx.registry)
                    .ok_or_else(|| {
                        miette!(
                            "Node {} does not have an input port for type {}",
                            id,
                            out_data.ty.ty().name()
                        )
                    })?;
                ctx.create_node(*id, pos)?
            }
            NodeCombo::Subgraph(id, _) => ctx.create_subgraph_node(*id, pos)?,
            NodeCombo::Object(ident, _) => ctx.create_object_node(*ident, pos, None)?,
            NodeCombo::List(id) => ctx.create_list_node(*id, pos)?,
        };
        if let Some(node_id) = nodes.first() {
            let in_pin = &ctx.snarl.in_pin(InPinId {
                node: *node_id,
                input: port_id,
            });
            if !ctx.connect(&out_pin, in_pin, commands)? {
                bail!("Failed to connect dragged-out pins");
            }
        }

        Ok(nodes)
    }
}
