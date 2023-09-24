use crate::value::etype::registry::serialization::parse_type;
use crate::value::etype::registry::{EObjectType, ETypesRegistry, ETypetId};
use crate::value::etype::{EDataType, ETypeConst};
use crate::value::{EValue, JsonValue};
use anyhow::{anyhow, bail, Context};
use itertools::Itertools;
use serde_json::Value;
use std::fmt::{Display, Formatter};
use ustr::Ustr;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
enum EnumPattern {
    StructField(Ustr, ETypeConst),
    Boolean,
    Scalar,
    Vec2,
    String,
    Const(ETypeConst),
}

impl EnumPattern {
    fn deserialize_struct_field(data: &JsonValue) -> anyhow::Result<Self> {
        match data {
            Value::Object(fields) => {
                let (k, v) = fields
                    .iter()
                    .exactly_one()
                    .map_err(|_| anyhow!("Exactly one field was expected in pattern definition"))?;
                let k = Ustr::from(k);
                let v = parse_type(v)?;

                let EDataType::Const { value } = v else {
                    bail!("Patterns only support constant values")
                };
                Ok(Self::StructField(k, value))
            }
            _ => bail!("Non-object patterns are not supported"),
        }
    }
}

impl Display for EnumPattern {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            EnumPattern::StructField(field, ty) => write!(f, "{{\"{field}\": \"{ty}\"}}"),
            EnumPattern::Boolean => write!(f, "{{boolean}}"),
            EnumPattern::Scalar => write!(f, "\"number\""),
            EnumPattern::Vec2 => write!(f, "\"vec2\""),
            EnumPattern::String => write!(f, "\"string\""),
            EnumPattern::Const(ty) => write!(f, "\"{ty}\""),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EEnumVariant {
    pat: EnumPattern,
    data: EDataType,
}

impl EEnumVariant {
    pub fn default_value(&self, registry: &ETypesRegistry) -> EValue {
        self.data.default_value(registry)
    }

    fn boolean() -> EEnumVariant {
        Self {
            data: EDataType::Boolean,
            pat: EnumPattern::Boolean,
        }
    }

    fn scalar() -> EEnumVariant {
        Self {
            data: EDataType::Scalar,
            pat: EnumPattern::Scalar,
        }
    }

    fn vec2() -> EEnumVariant {
        Self {
            data: EDataType::Vec2,
            pat: EnumPattern::Vec2,
        }
    }

    fn string() -> EEnumVariant {
        Self {
            data: EDataType::String,
            pat: EnumPattern::String,
        }
    }

    fn econst(data: ETypeConst) -> EEnumVariant {
        Self {
            data: EDataType::Const { value: data },
            pat: EnumPattern::Const(data),
        }
    }

    pub fn deserialize(
        registry: &mut ETypesRegistry,
        item: &JsonValue,
    ) -> anyhow::Result<EEnumVariant> {
        let (target_type, pat) = {
            match item {
                Value::String(ty) => (ty.as_str(), None),
                Value::Object(obj) => {
                    let target_type = obj
                        .get("type")
                        .context("Mandatory field `type` is missing")?
                        .as_str()
                        .context("`type` field must be a string")?;

                    (target_type, obj.get("pattern"))
                }
                _ => {
                    bail!("Expected enum definition item to be an object or a string")
                }
            }
        };

        let target_id = match parse_type(&JsonValue::String(target_type.to_string()))
            .with_context(|| format!("While parsing enum item type string `{target_type}`"))?
        {
            // Early return on "simple" types
            EDataType::Boolean => {
                return Ok(Self::boolean());
            }
            EDataType::Scalar => {
                return Ok(Self::scalar());
            }
            EDataType::Vec2 => {
                return Ok(Self::vec2());
            }
            EDataType::String => {
                return Ok(Self::string());
            }
            EDataType::Const { value } => {
                return Ok(Self::econst(value));
            }
            // Continue on object
            EDataType::Object { ident } => ident,
            EDataType::Id { .. } => bail!("ID types are not supported as enum variants"),
            EDataType::Ref { .. } => {
                bail!("Reference types are not yet supported as enum variants")
            }
        };

        registry.assert_defined(&target_id)?;

        let pat = if let Some(pattern) = pat {
            EnumPattern::deserialize_struct_field(pattern).context("While parsing pattern")?
        } else {
            let target_type = registry
                .fetch_or_deserialize(target_id)
                .context("Error during automatic pattern detection\n> If you see recursion error at the top of this log, consider specifying `pattern` field manually")?;

            match target_type {
                EObjectType::Enum(_) => bail!("Automatic pattern detection only works with struct targets, but `{target_id}` is an enum. Please specify `pattern` manually"),
                EObjectType::Struct(data) => {
                    let pat = data.fields.iter().filter_map(|f| {
                        match f.ty {
                            EDataType::Const { value } => {
                                Some((f.name, value))
                            }
                            _ => None,
                        }
                    }).exactly_one().map_err(|_| anyhow!("Target struct `{target_id}` contains multiple constant fields. Please specify `pattern` manually"))?;

                    EnumPattern::StructField(pat.0, pat.1)
                }
            }
        };
        Ok(EEnumVariant {
            pat,
            data: EDataType::Object { ident: target_id },
        })
    }
}

#[derive(Debug, Clone)]
pub struct EEnumData {
    pub ident: ETypetId,
    pub variants: Vec<EEnumVariant>,
}

impl EEnumData {
    pub fn default_value(&self, registry: &ETypesRegistry) -> EValue {
        let default_variant = self.variants.first().expect("Expect enum to not be empty");
        EValue::Enum {
            ident: EEnumVariantId {
                ident: self.ident,
                variant: default_variant.pat,
            },
            data: Box::new(default_variant.default_value(registry)),
        }
    }

    pub fn deserialize(
        registry: &mut ETypesRegistry,
        id: ETypetId,
        data: &Vec<JsonValue>,
    ) -> anyhow::Result<EEnumData> {
        anyhow::ensure!(!data.is_empty(), "Enum must have at least one variant");
        let mut items = Vec::with_capacity(data.len());
        for (i, item) in data.iter().enumerate() {
            let item = EEnumVariant::deserialize(registry, item)
                .with_context(|| format!("Parsing enum item at position {i}"))?;
            items.push(item);
        }
        let duplicates = items
            .iter()
            .enumerate()
            .group_by(|e| e.1.pat)
            .into_iter()
            .filter_map(|(pat, group)| {
                let groups = group.map(|e| e.0).map(|e| e.to_string()).collect_vec();
                if groups.len() == 1 {
                    return None;
                }
                Some(format!("Pattern {pat} in items {}", groups.join(", ")))
            })
            .collect_vec();

        if !duplicates.is_empty() {
            bail!(
                "Enum definition contains duplicate patterns:\n{}",
                duplicates.join("\n")
            )
        }
        Ok(EEnumData {
            variants: items,
            ident: id,
        })
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct EEnumVariantId {
    ident: ETypetId,
    // Data types are currently unique
    variant: EnumPattern,
}

impl Display for EEnumVariantId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.ident, self.variant)
    }
}