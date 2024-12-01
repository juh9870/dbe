use crate::etype::default::DefaultEValue;
use crate::etype::eitem::EItemInfo;
use crate::etype::EDataType;
use crate::json_utils::repr::JsonRepr;
use crate::registry::ETypesRegistry;
use crate::value::EValue;
use miette::bail;
use std::borrow::Cow;
use strum::EnumIs;
use ustr::Ustr;

#[derive(Debug, Clone)]
pub struct InputData {
    pub ty: NodePortType,
    pub name: Ustr,
}

#[derive(Debug, Clone)]
pub struct OutputData {
    pub ty: NodePortType,
    pub name: Ustr,
}

#[derive(Debug, Clone, EnumIs)]
pub enum NodePortType {
    /// Port that accepts any connection
    Any,
    /// Port that accepts only connections of the specific type
    Specific(EItemInfo),
}

impl NodePortType {
    pub fn default_value(&self, registry: &ETypesRegistry) -> DefaultEValue {
        match self {
            NodePortType::Any => EValue::Null.into(),
            NodePortType::Specific(info) => info.default_value(registry),
        }
    }

    pub fn item_info(&self) -> Option<&EItemInfo> {
        match self {
            NodePortType::Any => None,
            NodePortType::Specific(info) => Some(info),
        }
    }

    pub fn item_info_or_null(&self) -> Cow<EItemInfo> {
        self.item_info()
            .map(Cow::Borrowed)
            .unwrap_or_else(|| Cow::Owned(EItemInfo::simple_type(EDataType::null())))
    }

    pub fn ty(&self) -> EDataType {
        match self {
            NodePortType::Any => EDataType::null(),
            NodePortType::Specific(info) => info.ty(),
        }
    }

    pub fn compatible(registry: &ETypesRegistry, from: &NodePortType, to: &NodePortType) -> bool {
        match (from, to) {
            (NodePortType::Any, _) => false, // Any can't be converted to anything
            (_, NodePortType::Any) => true,
            (NodePortType::Specific(from), NodePortType::Specific(to)) => {
                types_compatible(registry, from, to)
            }
        }
    }

    pub fn convert_value(
        registry: &ETypesRegistry,
        from: &NodePortType,
        to: &NodePortType,
        value: EValue,
    ) -> miette::Result<EValue> {
        if from.ty() == to.ty() {
            return Ok(value);
        }

        let NodePortType::Specific(to) = to else {
            // When target type is Any, anything goes
            return Ok(value);
        };

        let from = from.item_info_or_null();

        if let Some(repr) = from.repr(registry) {
            if repr.is_convertible_to(registry, &from, to) {
                return repr.convert_to(registry, to, value);
            }
        }

        if let Some(repr) = to.repr(registry) {
            if repr.is_convertible_from(registry, to, &from) {
                return repr.convert_from(registry, to, value);
            }
        }

        bail!("conversion not supported")
    }
}

impl From<EItemInfo> for NodePortType {
    fn from(info: EItemInfo) -> Self {
        NodePortType::Specific(info)
    }
}

fn types_compatible(registry: &ETypesRegistry, from: &EItemInfo, to: &EItemInfo) -> bool {
    let source_ty = from.ty();
    let target_ty = to.ty();
    if source_ty == target_ty {
        return true;
    }

    if from
        .repr(registry)
        .is_some_and(|r| r.is_convertible_to(registry, from, to))
    {
        return true;
    }

    if to
        .repr(registry)
        .is_some_and(|r| r.is_convertible_from(registry, to, from))
    {
        return true;
    }

    false
}
