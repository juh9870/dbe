use crate::etype::eobject::EObject;
use crate::etype::EDataType;
use crate::registry::{ETypesRegistry, OPTIONAL_ID, OPTIONAL_PREFIX};
use crate::value::EValue;
use miette::bail;

pub fn wrap_in_option(registry: &ETypesRegistry, ty: EDataType, value: Option<EValue>) -> EValue {
    let data = registry.option_data_of(ty);
    match value {
        None => {
            let none_variant = data.variant_ids()[0];
            assert_eq!(none_variant.variant_name(), "None");
            EValue::Enum {
                variant: none_variant,
                data: Box::new(EValue::Null),
            }
        }
        Some(value) => {
            let some_variant = data.variant_ids()[1];
            assert_eq!(some_variant.variant_name(), "Some");
            EValue::Enum {
                variant: some_variant,
                data: Box::new(value),
            }
        }
    }
}

pub fn wrap_in_some(registry: &ETypesRegistry, value: EValue) -> EValue {
    wrap_in_option(registry, value.ty(), Some(value))
}

pub fn none_of_type(registry: &ETypesRegistry, ty: EDataType) -> EValue {
    wrap_in_option(registry, ty, None)
}

pub fn unwrap_optional_value<'a>(
    registry: &ETypesRegistry,
    value: &'a EValue,
) -> miette::Result<Option<&'a EValue>> {
    let EValue::Enum {
        variant,
        data: value,
    } = value
    else {
        bail!("Expected an optional value, got {}", value.ty().name());
    };

    if cfg!(debug_assertions) {
        let Some(data) = registry.get_enum(&variant.enum_id()) else {
            bail!("Expected an optional value, got {}", value.ty().name());
        };

        if data.generic_parent_id() != Some(*OPTIONAL_ID) {
            bail!("Expected an optional value, got {}", value.ty().name());
        }
    }

    match variant.variant_name().as_str() {
        "Some" => Ok(Some(value)),
        "None" => Ok(None),
        _ => panic!("Unexpected variant name"),
    }
}

pub fn is_type_option(ty: EDataType) -> bool {
    let EDataType::Object { ident } = ty else {
        return false;
    };

    ident
        .as_raw()
        .is_some_and(|id| id.starts_with(OPTIONAL_PREFIX))
}
