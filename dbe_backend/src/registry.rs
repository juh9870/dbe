use crate::etype::default::DefaultEValue;
use crate::etype::econst::ETypeConst;
use crate::etype::eenum::EEnumData;
use crate::etype::eitem::EItemInfo;
use crate::etype::eobject::EObject;
use crate::etype::estruct::EStructData;
use crate::etype::property::{default_properties, ObjectPropertyId};
use crate::etype::EDataType;
use crate::graph::node::all_node_factories;
use crate::json_utils::repr::{JsonRepr, Repr};
use crate::json_utils::JsonValue;
use crate::project::ProjectConfig;
use crate::registry::config::ExtraConfig;
use crate::serialization::deserialize_etype;
use crate::value::id::{EListId, EMapId, ETypeId};
use crate::value::EValue;
use atomic_refcell::AtomicRefCell;
use camino::Utf8PathBuf;
use itertools::Itertools;
use miette::{bail, miette, Context};
use parking_lot::RwLock;
use smallvec::SmallVec;
use std::any::{Any, TypeId};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::ops::Deref;
use std::sync::{Arc, LazyLock};
use ustr::{Ustr, UstrMap};
use utils::map::HashMap;
use utils::whatever_ref::WhateverRef;

pub mod config;
pub mod optional_helpers;

pub static OPTIONAL_PREFIX: &str = "sys:optional";

pub static OPTIONAL_ID: LazyLock<ETypeId> =
    LazyLock::new(|| ETypeId::from_raw("sys:optional".into()));
pub static OPTIONAL_STRING_ID: LazyLock<ETypeId> =
    LazyLock::new(|| ETypeId::from_raw("sys:optional<Item=string>".into()));
pub static OPTIONAL_BOOLEAN_ID: LazyLock<ETypeId> =
    LazyLock::new(|| ETypeId::from_raw("sys:optional<Item=boolean>".into()));
pub static OPTIONAL_NUMBER_ID: LazyLock<ETypeId> =
    LazyLock::new(|| ETypeId::from_raw("sys:optional<Item=number>".into()));

#[derive(Debug, Copy, Clone)]
pub struct ListData {
    pub value_type: EDataType,
}

#[derive(Debug, Copy, Clone)]
pub struct MapData {
    pub key_type: EDataType,
    pub value_type: EDataType,
}

#[derive(Debug, Clone)]
pub enum EObjectType {
    Struct(EStructData),
    Enum(EEnumData),
}

impl EObjectType {
    pub fn as_struct(&self) -> Option<&EStructData> {
        if let EObjectType::Struct(data) = self {
            return Some(data);
        }
        None
    }

    pub fn as_enum(&self) -> Option<&EEnumData> {
        if let EObjectType::Enum(data) = self {
            return Some(data);
        }
        None
    }

    pub fn parse_json(
        &self,
        registry: &ETypesRegistry,
        data: &mut JsonValue,
        inline: bool,
    ) -> miette::Result<EValue> {
        let mut data_holder: Option<JsonValue> = None;
        let repr = match self {
            EObjectType::Struct(s) => &s.repr,
            EObjectType::Enum(e) => &e.repr,
        };

        let data = if let Some(repr) = repr {
            data_holder.insert(repr.from_repr(registry, data, inline)?)
        } else {
            data
        };

        match self {
            EObjectType::Struct(s) => s
                .parse_json(registry, data, inline)
                .with_context(|| format!("in struct `{}`", s.ident)),
            EObjectType::Enum(e) => e
                .parse_json(registry, data, inline)
                .with_context(|| format!("in enum `{}`", e.ident)),
        }
    }
}

impl EObject for EObjectType {
    fn extra_properties(&self) -> &HashMap<ObjectPropertyId, ETypeConst> {
        match self {
            EObjectType::Struct(s) => s.extra_properties(),
            EObjectType::Enum(e) => e.extra_properties(),
        }
    }

    fn repr(&self) -> Option<&Repr> {
        match self {
            EObjectType::Struct(s) => s.repr(),
            EObjectType::Enum(e) => e.repr(),
        }
    }

    fn ident(&self) -> ETypeId {
        match self {
            EObjectType::Struct(s) => s.ident(),
            EObjectType::Enum(e) => e.ident(),
        }
    }

    fn generic_arguments_names(&self) -> &[Ustr] {
        match self {
            EObjectType::Struct(s) => s.generic_arguments_names(),
            EObjectType::Enum(e) => e.generic_arguments_names(),
        }
    }

    fn generic_arguments_values(&self) -> &[EItemInfo] {
        match self {
            EObjectType::Struct(s) => s.generic_arguments_values(),
            EObjectType::Enum(e) => e.generic_arguments_values(),
        }
    }

    fn generic_parent_id(&self) -> Option<ETypeId> {
        match self {
            EObjectType::Struct(s) => s.generic_parent_id(),
            EObjectType::Enum(e) => e.generic_parent_id(),
        }
    }

    fn title(&self, registry: &ETypesRegistry) -> String {
        match self {
            EObjectType::Struct(s) => s.title(registry),
            EObjectType::Enum(e) => e.title(registry),
        }
    }
}

// Allow large enum variants because we mainly consist of them once deserialized
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
enum RegistryItem {
    Raw(String),
    DeserializationInProgress,
    Ready(Arc<EObjectType>),
}

impl RegistryItem {
    #[inline(always)]
    pub fn expect_ready(&self) -> &Arc<EObjectType> {
        match self {
            RegistryItem::Ready(item) => item,
            _ => panic!("Registry item is not ready when expected"),
        }
    }
}

#[derive(Debug)]
pub struct ETypesRegistry {
    /// Main storage for types in the system
    types: BTreeMap<ETypeId, RegistryItem>,
    /// Secondary types storage. Used for creation of generic types at runtime.
    ///
    /// Less efficient than `types` because it requires locking and cloning
    /// the Arc
    pending_types: AtomicRefCell<HashMap<ETypeId, RegistryItem>>,
    /// Storage for lists
    lists: RwLock<BTreeMap<EListId, ListData>>,
    /// Storage for maps
    maps: RwLock<BTreeMap<EMapId, MapData>>,
    /// Cache for default values
    default_objects_cache: AtomicRefCell<BTreeMap<ETypeId, Arc<EValue>>>,
    /// Main project configuration
    project_config: ProjectConfig,
    /// Read/write data used by various editors, validators, etc
    extra_data: RwLock<BTreeMap<TypeId, Arc<dyn Any + Send + Sync>>>,
    /// Read/write cache storage
    cache: RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>,
    /// Read-only configuration used by various editors, validators, etc
    extra_config: BTreeMap<String, SmallVec<[(Utf8PathBuf, JsonValue); 1]>>,
    /// Whenever all types are deserialized and ready
    ready: bool,
}

impl ETypesRegistry {
    pub fn from_raws(
        data: impl IntoIterator<Item = (ETypeId, String)>,
        project_config: ProjectConfig,
    ) -> miette::Result<Self> {
        default_properties::register_extra_properties();

        let iter = data.into_iter();

        let types: BTreeMap<ETypeId, RegistryItem> = iter
            .map(|(id, v)| {
                Result::<(ETypeId, RegistryItem), miette::Error>::Ok((id, RegistryItem::Raw(v)))
            })
            .try_collect()
            .context("While grouping entries")?;

        let reg = Self {
            types,
            pending_types: Default::default(),
            lists: Default::default(),
            maps: Default::default(),
            default_objects_cache: Default::default(),
            project_config,
            extra_data: Default::default(),
            cache: Default::default(),
            extra_config: Default::default(),
            ready: false,
        };

        let mut reg = reg
            .deserialize_all()
            .context("failed to deserialize types")?
            .register_node_requirements()
            .context("failed to register node requirements")?
            .register_optionals()
            .context("failed to register optional types")?;

        reg.ready = true;

        Ok(reg)
    }

    /// Flushes all pending types into the main storage
    pub fn apply_pending(&mut self) {
        let mut pending = self.pending_types.borrow_mut();
        for (id, item) in pending.drain() {
            if let Some(old) = self.types.insert(id, item) {
                panic!("Type `{}` is already defined as {:?}", id, old);
            }
        }
    }

    /// Iterator over all ready objects
    ///
    /// Does not include pending objects
    pub fn all_ready_objects(&self) -> impl Iterator<Item = WhateverRef<EObjectType>> {
        self.types
            .values()
            .map(RegistryItem::expect_ready)
            .map(|x| WhateverRef::from_ref(x.deref()))
    }

    pub fn get_object(&self, id: &ETypeId) -> Option<WhateverRef<EObjectType>> {
        self.types
            .get(id)
            .map(RegistryItem::expect_ready)
            .map(|x| WhateverRef::from_ref(x.deref()))
            .or_else(|| {
                if !self.ready {
                    return None;
                }
                self.pending_types
                    .borrow()
                    .get(id)
                    .map(RegistryItem::expect_ready)
                    .map(|x| WhateverRef::from_arc(x.clone()))
            })
    }

    pub fn get_struct(&self, id: &ETypeId) -> Option<WhateverRef<EStructData>> {
        self.types
            .get(id)
            .and_then(|e| e.expect_ready().as_struct())
            .map(WhateverRef::from_ref)
            .or_else(|| {
                if !self.ready {
                    return None;
                }
                self.pending_types
                    .borrow()
                    .get(id)
                    .map(RegistryItem::expect_ready)
                    .and_then(|e| {
                        e.as_struct()?;
                        Some(WhateverRef::call_map(
                            WhateverRef::from_arc(e.clone()),
                            |e| e.as_struct().expect("Was checked to be struct"),
                        ))
                    })
                    .map(|e| e.into_dyn_ref())
            })
    }

    pub fn get_enum(&self, id: &ETypeId) -> Option<WhateverRef<EEnumData>> {
        self.types
            .get(id)
            .and_then(|e| e.expect_ready().as_enum())
            .map(WhateverRef::from_ref)
            .or_else(|| {
                if !self.ready {
                    return None;
                }
                self.pending_types
                    .borrow()
                    .get(id)
                    .map(RegistryItem::expect_ready)
                    .and_then(|e| {
                        e.as_enum()?;
                        Some(WhateverRef::call_map(
                            WhateverRef::from_arc(e.clone()),
                            |e| e.as_enum().expect("Was checked to be enum"),
                        ))
                    })
                    .map(|e| e.into_dyn_ref())
            })
    }

    pub fn get_list(&self, id: &EListId) -> Option<ListData> {
        self.lists.read().get(id).copied()
    }

    pub fn get_map(&self, id: &EMapId) -> Option<MapData> {
        self.maps.read().get(id).copied()
    }

    // pub fn register_struct(&mut self, id: ETypeId, data: EStructData) -> EDataType {
    //     self.types
    //         .insert(id, RegistryItem::Ready(EObjectType::Struct(data)));
    //     EDataType::Object { ident: id }
    // }
    //
    // pub fn register_enum(&mut self, id: ETypeId, data: EEnumData) -> EDataType {
    //     self.types
    //         .insert(id, RegistryItem::Ready(EObjectType::Enum(data)));
    //     EDataType::Object { ident: id }
    // }

    pub fn list_of(&self, value_type: EDataType) -> EDataType {
        EDataType::List {
            id: self.list_id_of(value_type),
        }
    }
    pub fn list_id_of(&self, value_type: EDataType) -> EListId {
        let id = format!("List<Item={}>", value_type.name());
        let id = EListId::from_raw(id.into());
        match self.lists.write().entry(id) {
            Entry::Occupied(_) => {}
            Entry::Vacant(entry) => {
                entry.insert(ListData { value_type });
            }
        }
        id
    }

    pub fn map_of(&self, key_type: EDataType, value_type: EDataType) -> EDataType {
        let id = format!("Map<Key={}, Item={}>", key_type.name(), value_type.name());
        let id = EMapId::from_raw(id.into());
        match self.maps.write().entry(id) {
            Entry::Occupied(_) => {}
            Entry::Vacant(entry) => {
                entry.insert(MapData {
                    key_type,
                    value_type,
                });
            }
        }
        EDataType::Map { id }
    }

    pub fn option_data_of(&self, value_type: EDataType) -> WhateverRef<EEnumData> {
        let id = self.option_id_of(value_type);
        self.get_enum(&id).expect("Optional enum should be present")
    }

    pub fn option_id_of(&self, value_type: EDataType) -> ETypeId {
        static NAMES: LazyLock<Ustr> = LazyLock::new(|| Ustr::from("Item"));
        let map: UstrMap<EItemInfo> = [(*NAMES, EItemInfo::simple_type(value_type))]
            .into_iter()
            .collect();
        self.make_generic_pending(*OPTIONAL_ID, map)
            .expect("Optional enum creation should not fail")
    }

    /// Creates a generic type with the specified arguments, or returns the existing one
    pub fn make_generic(
        &mut self,
        id: ETypeId,
        arguments: UstrMap<EItemInfo>,
    ) -> miette::Result<ETypeId> {
        let long_id = generic_id(id, arguments.iter());

        if self.types.contains_key(&long_id)
            || (self.ready && self.pending_types.borrow().contains_key(&long_id))
        {
            return Ok(long_id);
        }

        let obj = Self::make_type_generic(id, arguments, long_id, |id| {
            self.fetch_or_deserialize(*id)
                .map(|x| WhateverRef::from_arc(x.clone()))
        })?;

        self.types
            .insert(long_id, RegistryItem::Ready(Arc::new(obj)));
        Ok(long_id)
    }

    /// Same as [`ETypesRegistry::make_generic`], but uses the pending storage, allowing
    /// performing this operation on the non-owned registry.
    ///
    /// Make sure to call [`ETypesRegistry::apply_pending`] once mutable
    /// registry access is available
    pub fn make_generic_pending(
        &self,
        id: ETypeId,
        arguments: UstrMap<EItemInfo>,
    ) -> miette::Result<ETypeId> {
        let long_id = generic_id(id, arguments.iter());

        if self.types.contains_key(&long_id)
            || (self.ready && self.pending_types.borrow().contains_key(&long_id))
        {
            return Ok(long_id);
        }

        if !self.ready {
            bail!("Registry is not ready yet")
        }

        let obj = Self::make_type_generic(id, arguments, long_id, |id| {
            self.get_object(id)
                .ok_or_else(|| miette!("Type `{}` is not defined", id))
        })?;

        self.pending_types
            .borrow_mut()
            .insert(long_id, RegistryItem::Ready(Arc::new(obj)));

        Ok(long_id)
    }

    fn make_type_generic<'a>(
        id: ETypeId,
        arguments: UstrMap<EItemInfo>,
        generic_id: ETypeId,
        mut get_object: impl FnMut(&ETypeId) -> miette::Result<WhateverRef<'a, EObjectType>>,
    ) -> miette::Result<EObjectType> {
        let check_generics = |args: &[Ustr]| {
            if args.len() != arguments.len() {
                bail!(
                    "Object {id} expects {} generic arguments, but {} were provided",
                    args.len(),
                    arguments.len()
                )
            }

            Ok(())
        };

        let obj =
            get_object(&id).with_context(|| format!("failed to find object with id {}", id))?;

        let obj: EObjectType = obj.deref().clone();

        match obj {
            EObjectType::Struct(data) => {
                check_generics(&data.generic_arguments)?;
                let obj = data.apply_generics(&arguments, generic_id)?;
                Ok(EObjectType::Struct(obj))
            }
            EObjectType::Enum(data) => {
                check_generics(&data.generic_arguments)?;
                let obj = data.apply_generics(&arguments, generic_id, &mut get_object)?;
                Ok(EObjectType::Enum(obj))
            } // EObjectType::List(mut data) => {
              //     let args = if data.value_type.is_generic() {
              //         [Ustr::from("Item")].as_slice()
              //     } else {
              //         [].as_slice()
              //     };
              //     check_generics(args)?;
              //     let Some(ty) = arguments.get(&Ustr::from("Item")) else {
              //         bail!("Generic argument `Item` is not provided");
              //     };
              //     data.value_type = ty.clone();
              //     self.register_list(long_id, data);
              //     Ok(long_id)
              // }
              // EObjectType::Map(mut data) => {
              //     let args = if data.key_type.is_generic() {
              //         [Ustr::from("Key"), Ustr::from("Item")].as_slice()
              //     } else {
              //         [].as_slice()
              //     };
              //     check_generics(args)?;
              //     let Some(key_type) = arguments.get(&Ustr::from("Key")) else {
              //         bail!("Generic argument `Key` is not provided");
              //     };
              //     let Some(value_type) = arguments.get(&Ustr::from("Item")) else {
              //         bail!("Generic argument `Item` is not provided");
              //     };
              //     data.key_type = key_type.clone();
              //     data.value_type = value_type.clone();
              //     self.register_map(long_id, data);
              //     Ok(long_id)
              // }
        }
    }

    /// Returns Arc with extra registry data of the specified type
    pub fn extra_data<T: Any + Send + Sync + Default>(&self) -> Arc<T> {
        self.extra_data
            .write()
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Arc::new(T::default()))
            .clone()
            .downcast::<T>()
            .unwrap()
    }

    pub fn config(&self) -> ExtraConfig<'_> {
        ExtraConfig(self)
    }

    // fn extra_config(&self, name: &str) -> Option<&SmallVec<[(Utf8PathBuf, Value); 1]>> {
    //     self.extra_config.get(name)
    // }

    pub(crate) fn extra_config_mut(
        &mut self,
        name: String,
    ) -> &mut SmallVec<[(Utf8PathBuf, JsonValue); 1]> {
        self.extra_config.entry(name).or_default()
    }

    pub(crate) fn default_value_inner(&self, ident: &ETypeId) -> DefaultEValue {
        let Some(data) = self.types.get(ident) else {
            return EValue::Null.into();
        };

        let borrow = self.default_objects_cache.borrow();
        if let Some(cached) = borrow.get(ident) {
            cached.clone().into()
        } else {
            drop(borrow);
            let data = match &**data.expect_ready() {
                EObjectType::Struct(data) => data.default_value_inner(self),
                EObjectType::Enum(data) => data.default_value_inner(self),
            };

            let arced = Arc::new(data);

            self.default_objects_cache
                .borrow_mut()
                .insert(*ident, arced.clone());

            arced.into()
        }
    }

    // pub fn editor_for(
    //     &self,
    //     name: Option<&str>,
    //     ty: &EItemType,
    // ) -> miette::Result<Box<dyn EFieldEditor>> {
    //     let name = match name {
    //         None => match ty {
    //             EItemType::Number(_) => "number",
    //             EItemType::String(_) => "string",
    //             EItemType::Boolean(_) => "boolean",
    //             EItemType::Const(_) => "const",
    //             EItemType::ObjectId(_) => "id",
    //             EItemType::ObjectRef(_) => "ref",
    //             EItemType::Generic(_) => "generic",
    //             EItemType::Struct(EItemStruct { id, .. }) => self
    //                 .get_object(id)
    //                 .and_then(|e| e.default_editor())
    //                 .unwrap_or("struct"),
    //             EItemType::Enum(EItemEnum { id, .. }) => self
    //                 .get_object(id)
    //                 .and_then(|e| e.default_editor())
    //                 .unwrap_or("enum"),
    //         },
    //         Some(name) => name,
    //     };
    //     let ctor = self.editors.get(name);
    //     let Some(ctor) = ctor else {
    //         bail!("Editor `{name}` is not found");
    //     };
    //
    //     ctor.make_editor(ty)
    // }

    // pub fn editor_for_or_err(&self, name: Option<&str>, ty: &EItemType) -> Box<dyn EFieldEditor> {
    //     match self.editor_for(name, ty) {
    //         Ok(editor) => editor,
    //         Err(err) => Box::new(EFieldEditorError::new(err.to_string(), ty.ty())),
    //     }
    // }

    // fn register_raw_json_object(&mut self, id: ETypetId, data: JsonValue) -> EDataType {
    //     self.types.insert(*id, RegistryItem::Raw(data));
    //     EDataType::Object { ident: id }
    // }

    pub(crate) fn fetch_or_deserialize(
        &mut self,
        id: ETypeId,
    ) -> miette::Result<&Arc<EObjectType>> {
        let data = self
            .types
            .get_mut(&id)
            .ok_or_else(|| miette!("Type `{id}` is not defined"))?;

        match data {
            RegistryItem::Ready(_) => {
                return Ok(self
                    .types
                    .get(&id)
                    .expect("Should be present")
                    .expect_ready());
            }
            RegistryItem::DeserializationInProgress => {
                bail!("Recursion error! Type `{id}` is in process of getting evaluated")
            }
            RegistryItem::Raw(_) => {} // handled next
        };

        let RegistryItem::Raw(old) =
            std::mem::replace(data, RegistryItem::DeserializationInProgress)
        else {
            panic!("Item should be raw")
        };
        let ready = RegistryItem::Ready(Arc::new(deserialize_etype(self, id, &old)?));
        self.types.insert(id, ready);
        Ok(self
            .types
            .get(&id)
            .expect("Item should be present")
            .expect_ready())
    }

    fn deserialize_all(mut self) -> miette::Result<Self> {
        let keys = self.types.keys().copied().collect_vec();
        for id in keys {
            self.fetch_or_deserialize(id)
                .with_context(|| format!("failed to deserialize `{id}`"))?;
        }

        debug_assert!(
            self.types
                .values()
                .all(|e| matches!(e, RegistryItem::Ready(_))),
            "All items should be deserialized"
        );

        Ok(self)
    }

    fn register_node_requirements(mut self) -> miette::Result<Self> {
        for factory in all_node_factories().values() {
            factory
                .register_required_types(&mut self)
                .with_context(|| {
                    format!(
                        "failed to register types required by node `{}`",
                        factory.id()
                    )
                })?;
        }

        Ok(self)
    }

    fn register_optionals(mut self) -> miette::Result<Self> {
        for ty in [EDataType::Boolean, EDataType::Number, EDataType::String] {
            self.make_generic(
                ETypeId::from_raw("sys:optional".into()),
                [(Ustr::from("Item"), EItemInfo::simple_type(ty))]
                    .into_iter()
                    .collect(),
            )
            .with_context(|| format!("failed to register optional {}", ty.name()))?;
        }

        Ok(self)
    }

    // MAYBE?: use https://github.com/compenguy/ngrammatic for hints
    pub(crate) fn assert_defined(&self, id: &ETypeId) -> miette::Result<()> {
        if !self.types.contains_key(id) {
            bail!("Type `{id}` is not defined")
        }
        Ok(())
    }

    pub fn project_config(&self) -> &ProjectConfig {
        &self.project_config
    }
}

fn generic_id<'a>(
    id: ETypeId,
    arguments: impl Iterator<Item = (&'a Ustr, &'a EItemInfo)>,
) -> ETypeId {
    let args = arguments
        .map(|e| format!("{}={}", e.0, e.1.ty().name()))
        .sorted()
        .join(",");
    ETypeId::from_raw(format!("{id}<{args}>").into())
}

mod send_sync_check {
    use crate::registry::ETypesRegistry;

    fn expect_send_sync<T: Send + Sync>() {}

    #[allow(dead_code)]
    #[cfg_attr(test, test)]
    fn test() {
        expect_send_sync::<ETypesRegistry>();
    }
}
