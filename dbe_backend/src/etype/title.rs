use crate::etype::econst::ETypeConst;
use crate::etype::eobject::EObject;
use crate::etype::property::default_properties::PROP_OBJECT_TITLE;
use crate::etype::EDataType;
use crate::registry::ETypesRegistry;
use crate::value::id::ETypeId;
use itertools::Itertools;
use squidfmt::formatting::{FormatKeyError, FormatKeys};
use std::fmt::Formatter;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// Cached human-readable title of the object
#[derive(Debug, Default)]
pub struct ObjectTitle {
    pub(crate) value: OnceLock<String>,
    pub(crate) currently_initializing: AtomicBool,
}

impl Clone for ObjectTitle {
    fn clone(&self) -> Self {
        Self {
            value: OnceLock::new(),
            currently_initializing: AtomicBool::new(false),
        }
    }
}

impl ObjectTitle {
    pub fn get<T: EObject>(&self, obj: &T, registry: &ETypesRegistry) -> String {
        let Some(fmt) = PROP_OBJECT_TITLE.try_get(obj.extra_properties()) else {
            return obj.ident().to_string();
        };

        if self.currently_initializing.load(Ordering::Acquire) {
            let fmt_arg = FmtStub(obj);
            return fmt
                .0
                .format_to_string(&fmt_arg)
                .expect("Formatter should not fail");
        }

        if let Some(value) = self.value.get() {
            return value.clone();
        }
        self.currently_initializing.store(true, Ordering::Release);

        let str = fmt
            .0
            .format_to_string(&FmtTitle(obj, registry))
            .expect("Formatter should not fail");

        let result = self.value.get_or_init(|| str).clone();

        self.currently_initializing.store(false, Ordering::Release);

        result
    }
}

struct FmtStub<'a, T: EObject>(&'a T);

impl<T: EObject> FormatKeys for FmtStub<'_, T> {
    fn fmt(&self, key: &str, f: &mut Formatter<'_>) -> Result<(), FormatKeyError> {
        if !self
            .0
            .generic_arguments_names()
            .iter()
            .any(|e| e.as_str() == key)
        {
            return write!(f, "!!Unknown key `{}`!!", key).map_err(FormatKeyError::Fmt);
        };

        write!(f, "...").map_err(FormatKeyError::Fmt)
    }
}

struct FmtTitle<'a, T: EObject>(&'a T, &'a ETypesRegistry);

impl<T: EObject> FormatKeys for FmtTitle<'_, T> {
    fn fmt(&self, key: &str, f: &mut Formatter<'_>) -> Result<(), FormatKeyError> {
        let Some((pos, name)) = self
            .0
            .generic_arguments_names()
            .iter()
            .find_position(|e| e.as_str() == key)
        else {
            return write!(f, "!!Unknown key `{}`!!", key).map_err(FormatKeyError::Fmt);
        };

        if let Some(item) = &self.0.generic_arguments_values().get(pos) {
            let ty = item.ty();
            if let EDataType::Const {
                value: ETypeConst::String(value),
            } = ty
            {
                if let Ok(ty) = ETypeId::parse(value.as_str()) {
                    if let Some(obj) = self.1.get_object(&ty) {
                        return write!(f, "{}", obj.title(self.1)).map_err(FormatKeyError::Fmt);
                    }
                }
            }

            write!(f, "{}", ty.title(self.1)).map_err(FormatKeyError::Fmt)
        } else {
            write!(f, "{}", name).map_err(FormatKeyError::Fmt)
        }
    }
}
