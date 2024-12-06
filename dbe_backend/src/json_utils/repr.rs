use crate::etype::eenum::pattern::EnumPattern;
use crate::json_utils::repr::colors::ColorStringRepr;
use crate::json_utils::JsonValue;
use crate::registry::ETypesRegistry;
use crate::validation::Validator;
use crate::value::EValue;
use miette::{bail, miette};
use parking_lot::RwLock;
use std::borrow::Cow;
use std::fmt::Debug;
use std::str::FromStr;
use std::sync::{Arc, LazyLock};
use ustr::{Ustr, UstrMap};

mod colors;
mod ids;

#[allow(clippy::wrong_self_convention)]
pub trait JsonRepr: Send + Sync + Debug {
    fn id(&self) -> &'static str;

    /// Converts from the serialized data representation to the consumable data
    fn from_repr(
        &self,
        registry: &ETypesRegistry,
        data: &mut JsonValue,
        ignore_extra_fields: bool,
    ) -> miette::Result<JsonValue>;

    /// Converts from the consumable data to the serialized data representation
    fn into_repr(&self, registry: &ETypesRegistry, data: JsonValue) -> miette::Result<JsonValue>;

    /// Custom enum pattern for this repr. Leave none if this repr does not change the shape of the data
    fn enum_pat(&self) -> Option<EnumPattern>;

    /// Custom validators imposed by this repr
    fn validators(&self) -> Cow<'static, [Validator]> {
        Cow::Borrowed(&[])
    }

    /// Whenever objects of this repr can be created from the given type
    fn is_convertible_from(
        &self,
        registry: &ETypesRegistry,
        this: &EItemInfo,
        other: &EItemInfo,
    ) -> bool {
        self.is_convertible_both_way(registry, this, other)
    }

    /// Whenever objects of this repr can be converted to the given type
    fn is_convertible_to(
        &self,
        registry: &ETypesRegistry,
        this: &EItemInfo,
        other: &EItemInfo,
    ) -> bool {
        self.is_convertible_both_way(registry, this, other)
    }

    /// Whenever objects of this repr can be converted both ways with the given type
    fn is_convertible_both_way(
        &self,
        registry: &ETypesRegistry,
        this: &EItemInfo,
        other: &EItemInfo,
    ) -> bool {
        let _ = (registry, this, other);
        false
    }

    /// Converts the provided value to the value of this repr
    ///
    /// This function should only be called if [JsonRepr::is_convertible_from] returned true
    fn convert_from(
        &self,
        registry: &ETypesRegistry,
        this: &EItemInfo,
        _other: &EItemInfo,
        value: EValue,
    ) -> miette::Result<EValue> {
        let _ = (registry, this, value);
        bail!("conversion not supported")
    }

    /// Converts the value of this repr to the provided type
    ///
    /// This function should only be called if [JsonRepr::is_convertible_to] returned true
    fn convert_to(
        &self,
        registry: &ETypesRegistry,
        _this: &EItemInfo,
        other: &EItemInfo,
        value: EValue,
    ) -> miette::Result<EValue> {
        let _ = (registry, other, value);
        bail!("conversion not supported")
    }
}

#[derive(Debug, Clone)]
pub struct Repr(Arc<dyn JsonRepr>);

impl JsonRepr for Repr {
    fn id(&self) -> &'static str {
        self.0.id()
    }

    fn from_repr(
        &self,
        registry: &ETypesRegistry,
        data: &mut JsonValue,
        ignore_extra_fields: bool,
    ) -> miette::Result<JsonValue> {
        self.0.from_repr(registry, data, ignore_extra_fields)
    }

    fn into_repr(&self, registry: &ETypesRegistry, data: JsonValue) -> miette::Result<JsonValue> {
        self.0.into_repr(registry, data)
    }

    fn enum_pat(&self) -> Option<EnumPattern> {
        self.0.enum_pat()
    }

    fn validators(&self) -> Cow<'static, [Validator]> {
        self.0.validators()
    }

    fn is_convertible_from(
        &self,
        registry: &ETypesRegistry,
        this: &EItemInfo,
        other: &EItemInfo,
    ) -> bool {
        self.0.is_convertible_from(registry, this, other)
    }

    fn is_convertible_to(
        &self,
        registry: &ETypesRegistry,
        this: &EItemInfo,
        other: &EItemInfo,
    ) -> bool {
        self.0.is_convertible_to(registry, this, other)
    }

    fn is_convertible_both_way(
        &self,
        registry: &ETypesRegistry,
        this: &EItemInfo,
        other: &EItemInfo,
    ) -> bool {
        self.0.is_convertible_both_way(registry, this, other)
    }

    fn convert_from(
        &self,
        registry: &ETypesRegistry,
        this: &EItemInfo,
        other: &EItemInfo,
        value: EValue,
    ) -> miette::Result<EValue> {
        self.0.convert_from(registry, this, other, value)
    }

    fn convert_to(
        &self,
        registry: &ETypesRegistry,
        this: &EItemInfo,
        other: &EItemInfo,
        value: EValue,
    ) -> miette::Result<EValue> {
        self.0.convert_to(registry, this, other, value)
    }
}

impl FromStr for Repr {
    type Err = miette::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        get_repr(&s.into()).ok_or_else(|| miette!("unknown repr `{}`", s))
    }
}

static REPR_REGISTRY: LazyLock<RwLock<UstrMap<Repr>>> = LazyLock::new(|| {
    RwLock::new({
        let reprs: Vec<Arc<dyn JsonRepr>> = vec![
            Arc::new(ColorStringRepr::ARGB),
            Arc::new(ColorStringRepr::RGBA),
            Arc::new(ids::numeric::Id),
            Arc::new(ids::numeric::Ref),
        ];

        reprs
            .into_iter()
            .map(|r| (Ustr::from(r.id()), Repr(r)))
            .collect()
    })
});

pub fn get_repr(name: &Ustr) -> Option<Repr> {
    REPR_REGISTRY.read().get(name).cloned()
}

macro_rules! transparent {
    ($field_name:literal, $cast_fn:path, $expected:literal, $pattern:expr) => {
        $crate::json_utils::repr::transparent_from!($field_name, $cast_fn, $expected);
        $crate::json_utils::repr::transparent_to!($field_name, $cast_fn, $expected);

        fn enum_pat(&self) -> Option<EnumPattern> {
            Some($pattern)
        }
    };
}

macro_rules! transparent_from {
    ($field_name:literal, $cast_fn:path, $expected:literal) => {
        fn from_repr(
            &self,
            _registry: &$crate::registry::ETypesRegistry,
            data: &mut crate::json_utils::JsonValue,
            _ignore_extra_fields: bool,
        ) -> miette::Result<crate::json_utils::JsonValue> {
            let str = $crate::json_utils::json_expected($cast_fn(data), data, $expected)?;

            let mut fields = serde_json::value::Map::new();

            fields.insert($field_name.to_string(), str.into());

            Ok(fields.into())
        }
    };
}
macro_rules! transparent_to {
    ($field_name:literal, $cast_fn:path, $expected:literal) => {
        fn into_repr(
            &self,
            _registry: &$crate::registry::ETypesRegistry,
            data: JsonValue,
        ) -> miette::Result<crate::json_utils::JsonValue> {
            let obj = $crate::json_utils::json_expected(data.as_object(), &data, "object")?;

            let id = $cast_fn(
                obj.get($field_name)
                    .ok_or_else(|| miette::miette!("missing `{}` field", $field_name))?,
            )
            .ok_or_else(|| miette::miette!("`{}` field must be a {}", $field_name, $expected))?;

            Ok(id.into())
        }
    };
}

use crate::etype::eitem::EItemInfo;
pub(crate) use transparent;
pub(crate) use transparent_from;
pub(crate) use transparent_to;
