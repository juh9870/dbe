use crate::value::etype::registry::eitem::{EItemConst, EItemType, EItemTypeTrait};
use crate::value::etype::registry::{ETypeId, ETypesRegistry};
use crate::value::etype::{EDataType, ETypeConst};
use crate::value::EValue;
use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use ustr::{Ustr, UstrMap};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub(super) enum EnumPattern {
    StructField(Ustr, ETypeConst),
    Boolean,
    Number,
    String,
    Const(ETypeConst),
}
impl Display for EnumPattern {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            EnumPattern::StructField(field, ty) => write!(f, "{{\"{field}\": \"{ty}\"}}"),
            EnumPattern::Boolean => write!(f, "{{boolean}}"),
            EnumPattern::Number => write!(f, "{{number}}"),
            EnumPattern::String => write!(f, "{{string}}"),
            EnumPattern::Const(ty) => write!(f, "{{{ty}}}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EEnumVariant {
    pat: EnumPattern,
    data: EItemType,
    name: String,
}

impl EEnumVariant {
    pub fn default_value(&self, registry: &ETypesRegistry) -> EValue {
        self.data.default_value(registry)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn null() -> EEnumVariant {
        EEnumVariant {
            pat: EnumPattern::Const(ETypeConst::Null),
            data: EItemType::Const(EItemConst {
                value: ETypeConst::Null,
            }),
            name: "null".to_string(),
        }
    }

    pub(super) fn new(name: String, pat: EnumPattern, data: EItemType) -> Self {
        Self { pat, data, name }
    }
}

#[derive(Debug, Clone)]
pub struct EEnumData {
    pub generic_arguments: Vec<Ustr>,
    pub ident: ETypeId,
    pub variants: Vec<EEnumVariant>,
}

impl EEnumData {
    pub fn default_value(&self, registry: &ETypesRegistry) -> EValue {
        let default_variant = self.variants.first().expect("Expect enum to not be empty");
        EValue::Enum {
            variant: EEnumVariantId {
                ident: self.ident,
                variant: default_variant.pat,
            },
            data: Box::new(default_variant.default_value(registry)),
        }
    }

    pub fn apply_generics(&self, arguments: &UstrMap<EItemType>) -> anyhow::Result<Self> {
        let mut cloned = self.clone();
        for x in &mut cloned.variants {
            if let EItemType::Generic(g) = &x.data {
                let item = arguments.get(&g.argument_name).with_context(|| {
                    format!("Generic argument `{}` is not provided", g.argument_name)
                })?;
                x.data = item.clone();
            }
        }

        Ok(cloned)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct EEnumVariantId {
    ident: ETypeId,
    // Data types are currently unique
    variant: EnumPattern,
}

impl Display for EEnumVariantId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.ident, self.variant)
    }
}
