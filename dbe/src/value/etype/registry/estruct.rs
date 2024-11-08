use crate::graph::port_shapes::PortShape;
use crate::value::etype::registry::eitem::{EItemObjectId, EItemType, EItemTypeTrait};
use crate::value::etype::registry::{ETypeId, ETypesRegistry};
use crate::value::EValue;
use anyhow::{bail, Context};
use egui::Color32;
use itertools::Itertools;
use ustr::{Ustr, UstrMap};

#[derive(Debug, Clone)]
pub struct EStructField {
    pub name: Ustr,
    pub ty: EItemType,
}

#[derive(Debug, Clone)]
pub struct EStructData {
    pub generic_arguments: Vec<Ustr>,
    pub ident: ETypeId,
    pub fields: Vec<EStructField>,
    pub id_field: Option<usize>,
    pub default_editor: Option<String>,
    pub color: Option<Color32>,
    pub port_shape: Option<PortShape>,
}

impl EStructData {
    pub fn new(
        ident: ETypeId,
        generic_arguments: Vec<Ustr>,
        default_editor: Option<String>,
        color: Option<Color32>,
        port_shape: Option<PortShape>,
    ) -> EStructData {
        Self {
            generic_arguments,
            fields: Default::default(),
            ident,
            id_field: None,
            default_editor,
            color,
            port_shape,
        }
    }

    pub fn default_value(&self, registry: &ETypesRegistry) -> EValue {
        EValue::Struct {
            ident: self.ident,
            fields: self
                .fields
                .iter()
                .map(|f| (f.name.as_str().into(), f.ty.default_value(registry)))
                .collect(),
        }
    }

    pub fn apply_generics(
        mut self,
        arguments: &UstrMap<EItemType>,
        new_id: ETypeId,
        registry: &mut ETypesRegistry,
    ) -> anyhow::Result<Self> {
        self.ident = new_id;
        for x in &mut self.fields {
            if let EItemType::Generic(g) = &x.ty {
                let item = arguments.get(&g.argument_name).with_context(|| {
                    format!("Generic argument `{}` is not provided", g.argument_name)
                })?;
                x.ty = item.clone();
            }
        }

        if let Ok((_, item)) = arguments.iter().exactly_one() {
            if self.color.is_none() {
                self.color = Some(item.ty().color(registry));
            }
        }

        self.generic_arguments = vec![];

        Ok(self)
    }

    pub fn id_field(&self) -> Option<&EStructField> {
        self.id_field.map(|i| &self.fields[i])
    }

    pub fn id_field_data(&self) -> Option<&EItemObjectId> {
        self.id_field().map(|e| {
            if let EItemType::ObjectId(id) = &e.ty {
                id
            } else {
                panic!("Bad struct state")
            }
        })
    }

    pub(super) fn add_field(&mut self, field: EStructField) -> anyhow::Result<()> {
        if let EItemType::ObjectId(id) = &field.ty {
            if self.id_field.is_some() {
                bail!("Struct already has an ID field");
            }
            if id.ty != self.ident {
                bail!("Struct can't have an ID field with different type")
            }
            self.id_field = Some(self.fields.len());
        }
        self.fields.push(field);

        Ok(())
    }
}
