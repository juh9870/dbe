use crate::etype::eenum::pattern::EnumPattern;
use crate::etype::eenum::variant::{EEnumVariantId, EEnumVariantWithId};
use crate::etype::eitem::EItemInfo;
use crate::etype::EDataType;
use crate::json_utils::repr::JsonRepr;
use crate::json_utils::JsonValue;
use crate::registry::ETypesRegistry;
use crate::value::EValue;
use miette::{bail, miette};

#[derive(Debug)]
pub struct OptionalRepr;

impl JsonRepr for OptionalRepr {
    fn id(&self) -> &'static str {
        "option"
    }

    fn from_repr(
        &self,
        _registry: &ETypesRegistry,
        data: &mut JsonValue,
        _ignore_extra_fields: bool,
    ) -> miette::Result<JsonValue> {
        Ok(data.take())
    }

    fn into_repr(&self, _registry: &ETypesRegistry, data: JsonValue) -> miette::Result<JsonValue> {
        Ok(data)
    }

    fn enum_pat(&self) -> Option<EnumPattern> {
        None
    }

    fn is_convertible_from(
        &self,
        registry: &ETypesRegistry,
        this: &EItemInfo,
        other: &EItemInfo,
    ) -> bool {
        let (variant, _) = get_ty(registry, this).unwrap();
        let inner_info = &variant.data;

        if inner_info.ty() == other.ty() {
            return true;
        }

        if let Some(inner_repr) = inner_info.repr(registry) {
            if inner_repr.is_convertible_from(registry, inner_info, other) {
                return true;
            }
        };

        other
            .repr(registry)
            .is_some_and(|r| r.is_convertible_to(registry, other, inner_info))
    }

    fn convert_from(
        &self,
        registry: &ETypesRegistry,
        this: &EItemInfo,
        value: EValue,
    ) -> miette::Result<EValue> {
        let (variant, id) = get_ty(registry, this)?;

        let inner_info = &variant.data;

        fn make_enum(variant: &EEnumVariantId, value: EValue) -> EValue {
            EValue::Enum {
                variant: *variant,
                data: Box::new(value),
            }
        }

        if inner_info.ty() == value.ty() {
            return Ok(make_enum(id, value));
        }

        let other = EItemInfo::simple_type(value.ty());

        if let Some(inner_repr) = inner_info.repr(registry) {
            if inner_repr.is_convertible_from(registry, inner_info, &other) {
                return Ok(make_enum(
                    id,
                    inner_repr.convert_from(registry, inner_info, value)?,
                ));
            }
        } else if let Some(repr) = other.repr(registry) {
            if repr.is_convertible_to(registry, &other, inner_info) {
                return Ok(make_enum(id, repr.convert_to(registry, inner_info, value)?));
            }
        }

        bail!("conversion not supported")
    }
}

fn get_ty<'a>(
    registry: &'a ETypesRegistry,
    ty: &EItemInfo,
) -> miette::Result<EEnumVariantWithId<'a>> {
    let EDataType::Object { ident } = ty.ty() else {
        bail!(
            "`option` repr can only be applied to objects, got `{}`",
            ty.ty().name()
        );
    };

    let Some(data) = registry.get_enum(&ident) else {
        bail!(
            "`option` repr can only be applied to enums, got `{}`",
            ty.ty().name()
        );
    };

    let variant = data
        .variants_with_ids()
        .find(|e| e.0.name == "some")
        .ok_or_else(|| {
            miette!(
                "`option` repr can only be applied to enum types with `some` variant, got `{}`",
                ty.ty().name()
            )
        })?;

    Ok(variant)
}
