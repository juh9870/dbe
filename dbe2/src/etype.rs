use crate::etype::default::DefaultEValue;
use crate::etype::econst::ETypeConst;
use crate::json_utils::{json_expected, json_kind, JsonValue};
use crate::m_try;
use crate::registry::ETypesRegistry;
use crate::value::id::{EListId, EMapId, ETypeId};
use crate::value::EValue;
use miette::{bail, miette, Context};
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;
use strum::EnumIs;

pub mod default;
pub mod econst;
pub mod eenum;
pub mod eitem;
pub mod estruct;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, EnumIs)]
pub enum EDataType {
    /// Primitive boolean type
    Boolean,
    /// Primitive numeric type
    Number,
    /// Primitive string type
    String,
    /// Inline object, enum, or list type
    Object {
        ident: ETypeId,
    },
    /// Primitive constant type
    Const {
        value: ETypeConst,
    },
    List {
        id: EListId,
    },
    Map {
        id: EMapId,
    },
}

impl EDataType {
    pub fn default_value(&self, reg: &ETypesRegistry) -> DefaultEValue {
        match self {
            EDataType::Boolean => EValue::Boolean { value: false },
            EDataType::Number => EValue::Number { value: 0.0.into() },
            EDataType::String => EValue::String {
                value: Default::default(),
            },
            EDataType::Object { ident } => return reg.default_value_inner(ident),
            EDataType::Const { value } => value.default_value(),
            EDataType::List { id } => EValue::List {
                id: *id,
                values: vec![],
            },
            EDataType::Map { id } => EValue::Map {
                id: *id,
                values: Default::default(),
            },
        }
        .into()
    }

    pub const fn null() -> EDataType {
        EDataType::Const {
            value: ETypeConst::Null,
        }
    }

    pub fn name(&self) -> Cow<'_, str> {
        match self {
            EDataType::Boolean => "boolean".into(),
            EDataType::Number => "number".into(),
            EDataType::String => "string".into(),
            EDataType::Object { ident } => ident.to_string().into(),
            EDataType::Const { value } => value.to_string().into(),
            EDataType::List { id: ty } => ty.to_string().into(),
            EDataType::Map { id: ty } => ty.to_string().into(),
        }
    }

    pub fn parse_json(
        &self,
        registry: &ETypesRegistry,
        data: &mut JsonValue,
        inline: bool,
    ) -> miette::Result<EValue> {
        match self {
            EDataType::Boolean => json_expected(data.as_bool(), data, "bool").map(EValue::from),
            EDataType::Number => json_expected(data.as_number(), data, "number")
                .map(|num| OrderedFloat(num.as_f64().unwrap()).into()),
            EDataType::String => {
                json_expected(data.as_str(), data, "string").map(|s| s.to_string().into())
            }
            EDataType::Object { ident } => {
                let obj = registry.get_object(ident).ok_or_else(|| {
                    miette!(
                        "!!INTERNAL ERROR!! object id was not present in registry: `{}`",
                        ident
                    )
                })?;

                obj.parse_json(registry, data, inline)
            }
            EDataType::Const { value } => {
                let m = value.matches_json(data);

                if !m.by_type {
                    bail!(
                        "invalid data type. Expected {} but got {}",
                        value,
                        json_kind(data)
                    )
                }

                if !m.by_value {
                    bail!("invalid constant. Expected {} but got {}", value, data)
                }

                Ok(value.default_value())
            }
            EDataType::List { id } => {
                let list = registry.get_list(id).ok_or_else(|| {
                    miette!(
                        "!!INTERNAL ERROR!! list id was not present in registry: `{}`",
                        id
                    )
                })?;

                let JsonValue::Array(items) = data else {
                    bail!(
                        "invalid data type. Expected list but got {}",
                        json_kind(data)
                    )
                };

                let mut list_items = vec![];
                for (i, x) in items.iter_mut().enumerate() {
                    list_items.push(
                        list.value_type
                            .parse_json(registry, x, false)
                            .with_context(|| format!("at index {}", i))?,
                    )
                }

                Ok(EValue::List {
                    id: *id,
                    values: list_items,
                })
            }
            EDataType::Map { id } => {
                let map = registry.get_map(id).ok_or_else(|| {
                    miette!(
                        "!!INTERNAL ERROR!! map id was not present in registry: `{}`",
                        id
                    )
                })?;

                let JsonValue::Object(obj) = data else {
                    bail!(
                        "invalid data type. Expected map but got {}",
                        json_kind(data)
                    )
                };

                let mut entries = BTreeMap::new();

                for (k, v) in obj {
                    let key_name = k.clone();
                    let (k, v) = m_try(|| {
                        let k = map.key_type.parse_json(
                            registry,
                            &mut JsonValue::String(k.clone()),
                            false,
                        )?;
                        let v = map.value_type.parse_json(registry, v, false)?;
                        Ok((k, v))
                    })
                    .with_context(|| format!("in entry with key `{}`", key_name))?;

                    entries.insert(k, v);
                }

                Ok(EValue::Map {
                    id: *id,
                    values: entries,
                })
            }
        }
    }
}