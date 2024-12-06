use crate::etype::default::DefaultEValue;
use crate::etype::eenum::variant::{EEnumVariant, EEnumVariantId};
use crate::etype::eitem::EItemInfo;
use crate::etype::property::default_properties::{
    PROP_OBJECT_GRAPH_AUTOCONVERT, PROP_OBJECT_GRAPH_AUTOCONVERT_RECURSIVE,
    PROP_OBJECT_GRAPH_AUTOCONVERT_VARIANT,
};
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
                return repr.convert_to(registry, &from, to, value);
            }
        }

        if let Some(repr) = to.repr(registry) {
            if repr.is_convertible_from(registry, to, &from) {
                return repr.convert_from(registry, to, &from, value);
            }
        }

        if let Some(value) = convert_enum(registry, &from, to, value)? {
            return Ok(value);
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

    enum_assignable(registry, from, to)
}

fn enum_assignable(registry: &ETypesRegistry, from: &EItemInfo, to: &EItemInfo) -> bool {
    let target_ty = to.ty();

    let EDataType::Object { ident } = target_ty else {
        return false;
    };

    let Some(enum_data) = registry.get_enum(&ident) else {
        return false;
    };

    let autoconvert = PROP_OBJECT_GRAPH_AUTOCONVERT.get(&enum_data.extra_properties, false);
    if !autoconvert {
        return false;
    }

    let recursive_convert =
        PROP_OBJECT_GRAPH_AUTOCONVERT_RECURSIVE.get(&enum_data.extra_properties, false);
    let autoconvert_variant =
        PROP_OBJECT_GRAPH_AUTOCONVERT_VARIANT.try_get(&enum_data.extra_properties);

    fn check_variant(
        registry: &ETypesRegistry,
        variant: &EEnumVariant,
        from: &EItemInfo,
        recursive_convert: bool,
    ) -> bool {
        if variant.data.ty() == from.ty() {
            return true;
        }

        if recursive_convert {
            let inner_info = &variant.data;
            if let Some(inner_repr) = inner_info.repr(registry) {
                if inner_repr.is_convertible_from(registry, inner_info, from) {
                    return true;
                }
            };
            if from
                .repr(registry)
                .is_some_and(|r| r.is_convertible_to(registry, from, inner_info))
            {
                return true;
            }
        }

        false
    }

    if let Some(autoconvert_variant) = autoconvert_variant {
        for variant in enum_data.variants() {
            if variant.name == autoconvert_variant {
                return check_variant(registry, variant, from, recursive_convert);
            }
        }
    }

    for variant in enum_data.variants() {
        if check_variant(registry, variant, from, recursive_convert) {
            return true;
        }
    }

    false
}

fn convert_enum(
    registry: &ETypesRegistry,
    from: &EItemInfo,
    to: &EItemInfo,
    value: EValue,
) -> miette::Result<Option<EValue>> {
    let target_ty = to.ty();

    let EDataType::Object { ident } = target_ty else {
        return Ok(None);
    };

    let Some(enum_data) = registry.get_enum(&ident) else {
        return Ok(None);
    };

    let autoconvert = PROP_OBJECT_GRAPH_AUTOCONVERT.get(&enum_data.extra_properties, false);
    if !autoconvert {
        return Ok(None);
    }

    let recursive_convert =
        PROP_OBJECT_GRAPH_AUTOCONVERT_RECURSIVE.get(&enum_data.extra_properties, false);
    let autoconvert_variant =
        PROP_OBJECT_GRAPH_AUTOCONVERT_VARIANT.try_get(&enum_data.extra_properties);

    fn make_enum(variant: &EEnumVariantId, value: EValue) -> EValue {
        EValue::Enum {
            variant: *variant,
            data: Box::new(value),
        }
    }

    for variant in enum_data.variants_with_ids() {
        let (variant, variant_id) = variant;

        if autoconvert_variant.is_some_and(|v| v != variant.name) {
            continue;
        }

        if variant.data.ty() == from.ty() {
            return Ok(Some(make_enum(variant_id, value)));
        }

        if recursive_convert {
            let inner_info = &variant.data;
            if let Some(inner_repr) = inner_info.repr(registry) {
                if inner_repr.is_convertible_from(registry, inner_info, from) {
                    return Ok(Some(make_enum(
                        variant_id,
                        inner_repr.convert_from(registry, inner_info, from, value)?,
                    )));
                }
            }
            if let Some(repr) = from.repr(registry) {
                if repr.is_convertible_to(registry, from, inner_info) {
                    return Ok(Some(make_enum(
                        variant_id,
                        repr.convert_to(registry, from, inner_info, value)?,
                    )));
                }
            }
        }
    }

    Ok(None)
}
