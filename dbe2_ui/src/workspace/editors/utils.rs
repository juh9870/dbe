use crate::workspace::editors::Props;
use dbe2::etype::econst::ETypeConst;
use dbe2::value::EValue;
use egui::{InnerResponse, RichText, Ui, WidgetText};
use itertools::Itertools;
use miette::miette;
use std::collections::BTreeMap;
use std::fmt::Display;
use ustr::Ustr;

/// Upper bound size guarantees of different editors
///
/// Editor may take up less space than what is specified by this enum, but
/// promise to not take any more than specified
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum EditorSize {
    /// Editors with this size promise to take up no space in UI
    None,
    /// Editors with this size promise to reasonably fit as a part of a single
    /// line, along with other content
    Inline,
    /// Editors with this size may occupy up to a whole line
    SingleLine,
    /// Editors with this size may occupy more than one line
    Block,
}

impl EditorSize {
    pub fn is_inline(&self) -> bool {
        matches!(self, EditorSize::Inline)
    }

    pub fn is_single_line(&self) -> bool {
        matches!(self, EditorSize::SingleLine)
    }
    pub fn is_block(&self) -> bool {
        matches!(self, EditorSize::Block)
    }
}

#[inline(always)]
pub fn prop_opt<'a, T: TryFrom<ETypeConst, Error = miette::Error>>(
    props: impl Into<Option<Props<'a>>>,
    name: &str,
) -> miette::Result<Option<T>> {
    if let Some(prop) = props.into().and_then(|props| props.get(name)) {
        Ok(Some(T::try_from(*prop).map_err(|e| {
            miette!("Bad value for property `{}`: `{}`", name, e)
        })?))
    } else {
        Ok(None)
    }
}

#[inline(always)]
pub fn prop<'a, T: TryFrom<ETypeConst, Error = miette::Error>>(
    props: impl Into<Option<Props<'a>>>,
    name: &str,
    default: T,
) -> miette::Result<T> {
    prop_opt(props, name).map(|o| o.unwrap_or(default))
}

#[inline(always)]
pub fn prop_required<'a, T: TryFrom<ETypeConst, Error = miette::Error>>(
    props: impl Into<Option<Props<'a>>>,
    name: &str,
) -> miette::Result<T> {
    prop_opt(props, name)
        .and_then(|s| s.ok_or_else(|| miette!("required property `{}` is missing", name)))
}

pub fn get_values<'a, T: TryFrom<&'a EValue, Error = E>, E: Into<miette::Error>, const N: usize>(
    fields: &'a BTreeMap<Ustr, EValue>,
    names: [&str; N],
) -> miette::Result<[T; N]> {
    let vec: Vec<T> = names
        .into_iter()
        .map(|name| {
            fields
                .get(&name.into())
                .ok_or_else(|| miette!("Field {name} is missing"))
                .and_then(|value| T::try_from(value).map_err(|err| err.into()))
        })
        .try_collect()?;

    Ok(vec
        .try_into()
        .map_err(|_| unreachable!("Length did not change"))
        .unwrap())
}

pub fn set_values<'a>(
    fields: &mut BTreeMap<Ustr, EValue>,
    entries: impl IntoIterator<Item = (&'a str, impl Into<EValue>)>,
) {
    let entries = entries.into_iter().map(|(k, v)| (Ustr::from(k), v.into()));
    fields.extend(entries);
}

pub fn ensure_field<'a, T: TryFrom<&'a mut EValue, Error = E>, E: Into<miette::Error>>(
    ui: &mut Ui,
    fields: &'a mut BTreeMap<Ustr, EValue>,
    field_name: impl AsRef<str> + Display,
    editor: impl FnOnce(&mut Ui, T),
) -> bool {
    let name = field_name.as_ref();
    let value = fields.get_mut(&name.into());

    let Some(val) = value else {
        labeled_error(ui, name, miette!("Field is missing"));
        return false;
    };

    let val: Result<T, T::Error> = val.try_into();
    match val {
        Err(err) => {
            labeled_error(ui, name, err);
            false
        }
        Ok(data) => {
            editor(ui, data);
            true
        }
    }
}

pub trait EditorResultExt {
    type Data;
    fn then_draw<Res>(
        self,
        ui: &mut Ui,
        draw: impl FnOnce(&mut Ui, Self::Data) -> Res,
    ) -> Option<Res>;
}

impl<T, Err: Into<miette::Error>> EditorResultExt for Result<T, Err> {
    type Data = T;

    fn then_draw<Res>(
        self,
        ui: &mut Ui,
        draw: impl FnOnce(&mut Ui, Self::Data) -> Res,
    ) -> Option<Res> {
        match self {
            Err(err) => {
                inline_error(ui, err);
                None
            }
            Ok(data) => Some(draw(ui, data)),
        }
    }
}

pub fn inline_error(ui: &mut Ui, err: impl Into<miette::Error>) {
    ui.label(RichText::new(err.into().to_string()).color(ui.style().visuals.error_fg_color));
}

pub fn labeled_field<T>(
    ui: &mut Ui,
    label: impl Into<WidgetText>,
    content: impl FnOnce(&mut Ui) -> T,
) -> InnerResponse<T> {
    ui.horizontal(|ui| {
        let text = label.into();
        if !text.is_empty() {
            ui.label(text);
        }
        content(ui)
    })
}

pub fn labeled_error(ui: &mut Ui, label: impl Into<WidgetText>, err: impl Into<miette::Error>) {
    ui.horizontal(|ui| {
        ui.label(label);
        inline_error(ui, err);
    });
}

pub fn unsupported_fn(ui: &mut Ui, label: impl Into<WidgetText>) {
    labeled_error(ui, label, miette!("{}", "dbe.editor.unsupported_value"));
}

macro_rules! unsupported {
    ($ui:expr, $label:expr, $value:expr, $editor:expr) => {
        // tracing::warn!(value=?$value, editor=?$editor, "Unsupported value for editor");
        $crate::workspace::editors::utils::labeled_error(
            $ui,
            $label,
            miette::miette!("{}", ("dbe.editor.unsupported_value")),
        );
        return
    };
}

pub(crate) use unsupported;
