use crate::etype::eenum::pattern::EnumPattern;
use crate::etype::eitem::EItemInfo;
use crate::etype::EDataType;
use crate::json_utils::repr::{transparent, JsonRepr};
use crate::json_utils::JsonValue;
use crate::registry::ETypesRegistry;
use crate::validation::ids::numeric::NumericIDRegistry;
use crate::validation::{validator_by_name, Validator};
use crate::value::id::ETypeId;
use crate::value::{estruct, EValue};
use miette::bail;
use std::borrow::Cow;
use std::sync::LazyLock;

#[derive(Debug)]
pub struct Id;

static ID_VALIDATOR: LazyLock<Validator> =
    LazyLock::new(|| validator_by_name("ids/numeric".into()).unwrap());
static REF_VALIDATOR: LazyLock<Validator> =
    LazyLock::new(|| validator_by_name("ids/numeric_ref".into()).unwrap());

impl JsonRepr for Id {
    fn id(&self) -> &'static str {
        "ids/numeric"
    }

    transparent!("id", JsonValue::as_f64, "number", EnumPattern::Number);

    fn validators(&self) -> Cow<'static, [Validator]> {
        Cow::Owned(vec![ID_VALIDATOR.clone()])
    }

    fn is_convertible_to(
        &self,
        registry: &ETypesRegistry,
        this: &EItemInfo,
        other: &EItemInfo,
    ) -> bool {
        // ids/numeric can be converted to numbers or ids/numeric_ref

        if other.ty().is_number() {
            return true;
        }

        if !other
            .repr(registry)
            .is_some_and(|r| r.id() == "ids/numeric_ref")
        {
            return false;
        };

        generics_compatible(registry, this, other)
    }

    fn convert_to(
        &self,
        registry: &ETypesRegistry,
        _this: &EItemInfo,
        other: &EItemInfo,
        value: EValue,
    ) -> miette::Result<EValue> {
        // ids/numeric can be converted to numbers or ids/numeric_ref

        if other.ty().is_number() {
            return Ok(value
                .try_get_field("id")
                .and_then(|f| f.try_as_number())
                .copied()?
                .into());
        }

        #[cfg(debug_assertions)]
        if !other
            .repr(registry)
            .is_some_and(|r| r.id() == "ids/numeric_ref")
        {
            panic!("ids/numeric can only be converted to number or ids/numeric_ref");
        };

        let EDataType::Object { ident } = other.ty() else {
            bail!("ids/numeric can only be converted to number or object types");
        };

        value_to_obj(ident, value)
    }
}

#[derive(Debug)]
pub struct Ref;

impl JsonRepr for Ref {
    fn id(&self) -> &'static str {
        "ids/numeric_ref"
    }

    transparent!("id", JsonValue::as_f64, "number", EnumPattern::Number);

    fn validators(&self) -> Cow<'static, [Validator]> {
        Cow::Owned(vec![REF_VALIDATOR.clone()])
    }

    fn is_convertible_from(
        &self,
        registry: &ETypesRegistry,
        this: &EItemInfo,
        other: &EItemInfo,
    ) -> bool {
        // ids/numeric_ref can be converted from ids/numeric

        if !other
            .repr(registry)
            .is_some_and(|r| r.id() == "ids/numeric")
        {
            return false;
        };

        generics_compatible(registry, this, other)
    }

    fn is_convertible_to(
        &self,
        _registry: &ETypesRegistry,
        _this: &EItemInfo,
        other: &EItemInfo,
    ) -> bool {
        // ids/numeric_ref can be converted to numbers

        other.ty().is_number()
    }

    fn convert_from(
        &self,
        _registry: &ETypesRegistry,
        this: &EItemInfo,
        _other: &EItemInfo,
        value: EValue,
    ) -> miette::Result<EValue> {
        // ids/numeric_ref can be converted from ids/numeric

        let EDataType::Object { ident } = this.ty() else {
            bail!("ids/numeric_ref can only be applied to object types");
        };

        value_to_obj(ident, value)
    }

    fn convert_to(
        &self,
        _registry: &ETypesRegistry,
        _this: &EItemInfo,
        other: &EItemInfo,
        value: EValue,
    ) -> miette::Result<EValue> {
        // ids/numeric_ref can be converted to numbers

        if !other.ty().is_number() {
            bail!("ids/numeric_ref can only be converted to number types");
        };

        Ok(value
            .try_get_field("id")
            .and_then(|f| f.try_as_number())
            .copied()?
            .into())
    }
}

fn generics_compatible(reg: &ETypesRegistry, a: &EItemInfo, b: &EItemInfo) -> bool {
    let a = a.ty();
    let b = b.ty();

    NumericIDRegistry::of(reg)
        .is_id_assignable_ty(a, b)
        .expect("types should be objects at this point")
}

fn value_to_obj(ident: ETypeId, value: EValue) -> miette::Result<EValue> {
    let id = if let EValue::Struct { .. } = &value {
        *value.try_get_field("id").and_then(|f| f.try_as_number())?
    } else {
        bail!("conversion not supported")
    };

    Ok(estruct!(ident {"id": id}))
}
