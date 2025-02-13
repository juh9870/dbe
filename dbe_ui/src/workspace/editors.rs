use crate::m_try;
use crate::ui_props::{PROP_FIELD_EDITOR, PROP_OBJECT_EDITOR};
use crate::workspace::editors::boolean::BooleanEditor;
use crate::workspace::editors::consts::ConstEditor;
use crate::workspace::editors::enum_flags::EnumFlagsEditor;
use crate::workspace::editors::enums::EnumEditor;
use crate::workspace::editors::errors::{ErrorEditor, ErrorProps};
use crate::workspace::editors::id_ref::IdRefEditor;
use crate::workspace::editors::map::MapEditor;
use crate::workspace::editors::number::NumberEditor;
use crate::workspace::editors::rgb::RgbEditor;
use crate::workspace::editors::string::StringEditor;
use crate::workspace::editors::structs::StructEditor;
use crate::workspace::editors::utils::EditorSize;
use crate::workspace::editors::wrapped::WrappedEditor;
use ::utils::map::HashMap;
use dbe_backend::diagnostic::context::DiagnosticContextRef;
use dbe_backend::etype::econst::ETypeConst;
use dbe_backend::etype::eitem::EItemInfo;
use dbe_backend::etype::eobject::EObject;
use dbe_backend::etype::property::{FieldPropertyId, ObjectPropertyId};
use dbe_backend::etype::EDataType;
use dbe_backend::project::docs::Docs;
use dbe_backend::project::docs::DocsRef;
use dbe_backend::registry::{EObjectType, ETypesRegistry};
use dbe_backend::value::EValue;
use downcast_rs::{impl_downcast, Downcast};
use dyn_clone::DynClone;
use egui::Ui;
use list::ListEditor;
use miette::{bail, miette};
use std::fmt::Debug;
use std::ops::{BitOr, BitOrAssign, Deref};
use std::sync::LazyLock;
use ustr::{Ustr, UstrMap};

pub mod quick;
mod utils;

mod boolean;
mod consts;
mod enum_flags;
mod enums;
mod errors;
mod id_ref;
mod list;
mod map;
mod number;
mod rgb;
mod string;
mod structs;
mod wrapped;

static EDITORS: LazyLock<UstrMap<Box<dyn Editor>>> = LazyLock::new(|| default_editors().collect());

fn default_editors() -> impl Iterator<Item = (Ustr, Box<dyn Editor>)> {
    let v: Vec<(Ustr, Box<dyn Editor>)> = vec![
        ("number".into(), Box::new(NumberEditor::new(false))),
        ("slider".into(), Box::new(NumberEditor::new(true))),
        ("string".into(), Box::new(StringEditor)),
        ("boolean".into(), Box::new(BooleanEditor)),
        ("struct".into(), Box::new(StructEditor)),
        ("rgba".into(), Box::new(RgbEditor::new(true))),
        ("rgb".into(), Box::new(RgbEditor::new(false))),
        ("const".into(), Box::new(ConstEditor)),
        ("enum".into(), Box::new(EnumEditor)),
        ("list".into(), Box::new(ListEditor)),
        ("map".into(), Box::new(MapEditor)),
        ("enum_flags".into(), Box::new(EnumFlagsEditor)),
        (
            "ids/numeric".into(),
            Box::new(WrappedEditor::new(NumberEditor::new(false), "id".into())),
        ),
        ("ids/numeric_ref".into(), Box::new(IdRefEditor)),
        // TODO: proper combobox editors
        ("eh:image".into(), Box::new(StringEditor)),
        ("eh:layout".into(), Box::new(StringEditor)),
        ("eh:audioclip".into(), Box::new(StringEditor)),
        ("eh:prefab".into(), Box::new(StringEditor)),
        // TODO: proper expression editor
        ("eh:expression".into(), Box::new(StringEditor)),
        // Enums
        // (
        //     "enum".to_string(),
        //     Box::new(EnumEditorConstructor::from(EnumEditorType::Auto)),
        // ),
        // (
        //     "enum:toggle".to_string(),
        //     Box::new(EnumEditorConstructor::from(EnumEditorType::Toggle)),
        // ),
        // (
        //     "enum:full".to_string(),
        //     Box::new(EnumEditorConstructor::from(EnumEditorType::Full)),
        // ),
        // ("const".to_string(), Box::new(ConstEditorConstructor)),
        // ("id".to_string(), Box::new(ObjectIdEditorConstructor)),
        // // other
        // ("rgb".to_string(), Box::new(RgbEditorConstructor::rgb())),
        // ("rgba".to_string(), Box::new(RgbEditorConstructor::rgba())),
    ];
    v.into_iter()
}
type Props<'a> = &'a HashMap<FieldPropertyId, ETypeConst>;
type ObjectProps<'a> = &'a HashMap<ObjectPropertyId, ETypeConst>;

trait EditorProps: std::any::Any + DynClone + Downcast {
    fn pack(self) -> DynProps
    where
        Self: Sized,
    {
        Some(Box::new(self))
    }
}

impl_downcast!(EditorProps);

fn cast_props<T: EditorProps>(props: &DynProps) -> &T {
    props.as_ref().and_then(|b| b.downcast_ref::<T>()).unwrap()
}

type DynProps = Option<Box<dyn EditorProps>>;

trait Editor: std::any::Any + Send + Sync + Debug {
    fn props(
        &self,
        reg: &ETypesRegistry,
        item: Option<&EItemInfo>,
        object_props: DynProps,
    ) -> miette::Result<DynProps> {
        let _ = (reg, item, object_props);
        Ok(None)
    }

    fn object_props(
        &self,
        reg: &ETypesRegistry,
        props: &HashMap<ObjectPropertyId, ETypeConst>,
    ) -> miette::Result<DynProps> {
        let _ = (reg, props);
        Ok(None)
    }

    fn size(&self, props: &DynProps) -> EditorSize;

    fn edit(
        &self,
        ui: &mut Ui,
        ctx: EditorContext,
        diagnostics: DiagnosticContextRef,
        field_name: &str,
        value: &mut EValue,
        props: &DynProps,
    ) -> EditorResponse;
}

#[derive(Debug)]
pub struct EditorContext<'a> {
    pub registry: &'a ETypesRegistry,
    pub docs: &'a Docs,
    pub docs_ref: DocsRef,
}

impl<'a> EditorContext<'a> {
    pub fn new(registry: &'a ETypesRegistry, docs: &'a Docs, docs_ref: DocsRef) -> Self {
        Self {
            registry,
            docs,
            docs_ref,
        }
    }

    pub fn copy_with_docs(&self, docs_ref: DocsRef) -> Self {
        Self {
            registry: self.registry,
            docs: self.docs,
            docs_ref,
        }
    }

    /// Returns the context with the current docs ref, leaving [self] context
    /// with the ref from `docs_ref`
    pub fn replace_docs_ref(&mut self, docs_ref: DocsRef) -> Self {
        Self {
            registry: self.registry,
            docs: self.docs,
            docs_ref: std::mem::replace(&mut self.docs_ref, docs_ref),
        }
    }
}

pub struct EditorData(&'static dyn Editor, DynProps);

#[derive(Debug, Clone)]
pub struct EditorResponse {
    pub changed: bool,
}

impl EditorResponse {
    pub fn new(changed: bool) -> Self {
        Self { changed }
    }

    pub fn changed() -> Self {
        Self { changed: true }
    }

    pub fn unchanged() -> Self {
        Self { changed: false }
    }
}

impl BitOr for EditorResponse {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self {
            changed: self.changed || rhs.changed,
        }
    }
}

impl BitOrAssign for EditorResponse {
    fn bitor_assign(&mut self, rhs: Self) {
        self.changed = self.changed || rhs.changed;
    }
}

pub fn editor_for_value(reg: &ETypesRegistry, value: &EValue) -> EditorData {
    editor_for_type(reg, &value.ty())
}

pub fn editor_for_type(reg: &ETypesRegistry, ty: &EDataType) -> EditorData {
    m_try(|| {
        let (editor, object_props) = editor_for_raw(reg, ty, None)?;

        Ok(EditorData(editor, editor.props(reg, None, object_props)?))
    })
    .unwrap_or_else(|err| EditorData(&ErrorEditor, ErrorProps(err.to_string()).pack()))
}

pub fn editor_for_item(reg: &ETypesRegistry, item: &EItemInfo) -> EditorData {
    m_try(|| {
        let name = PROP_FIELD_EDITOR.try_get(item.extra_properties());

        let (editor, object_props) = editor_for_raw(reg, &item.ty(), name)?;

        Ok(EditorData(
            editor,
            editor.props(reg, Some(item), object_props)?,
        ))
    })
    .unwrap_or_else(|err| EditorData(&ErrorEditor, ErrorProps(err.to_string()).pack()))
}

fn editor_for_raw(
    reg: &ETypesRegistry,
    ty: &EDataType,
    name: Option<Ustr>,
) -> miette::Result<(&'static dyn Editor, DynProps)> {
    let name = match name {
        None => match ty {
            EDataType::Number => "number".into(),
            EDataType::String => "string".into(),
            EDataType::Boolean => "boolean".into(),
            EDataType::Const { .. } => "const".into(),
            EDataType::Object { ident } => {
                let data = reg
                    .get_object(ident)
                    .ok_or_else(|| miette!("Unknown object ID `{}`", ident))?;
                let data = data.deref();
                let name = if let Some(prop) = PROP_OBJECT_EDITOR.try_get(data.extra_properties()) {
                    prop
                } else {
                    match data {
                        EObjectType::Struct(_) => "struct".into(),
                        EObjectType::Enum(_) => "enum".into(),
                    }
                };

                let Some(editor) = EDITORS.get(&name) else {
                    bail!("unknown editor `{}`", name)
                };

                let props = editor.object_props(reg, data.extra_properties())?;
                return Ok((editor.deref(), props));
            }
            EDataType::List { .. } => "list".into(),
            EDataType::Map { .. } => "map".into(),
            EDataType::Unknown => "unknown".into(),
        },
        Some(name) => name,
    };

    let Some(editor) = EDITORS.get(&name) else {
        bail!("unknown editor `{}`", name)
    };

    Ok((editor.deref(), None))
}

impl EditorData {
    pub fn show(
        &self,
        ui: &mut Ui,
        ctx: EditorContext,
        diagnostics: DiagnosticContextRef,
        field_name: &str,
        value: &mut EValue,
    ) -> EditorResponse {
        let Self(editor, props) = self;
        editor.edit(ui, ctx, diagnostics, field_name, value, props)
    }

    pub fn size(&self) -> EditorSize {
        self.0.size(&self.1)
    }
}
