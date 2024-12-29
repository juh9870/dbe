use crate::DbeApp;
use dbe_backend::etype::eobject::EObject;
use dbe_backend::graph::node::{get_node_factory, NodeFactory};
use dbe_backend::project::docs::{
    Docs, DocsDescription, DocsRef, DocsWindowRef, NodeDocs, TypeDocs,
};
use dbe_backend::registry::{EObjectType, ETypesRegistry};
use dbe_backend::value::id::ETypeId;
use egui::{CollapsingHeader, RichText, ScrollArea, TextEdit, Ui, Widget, WidgetText, Window};
use egui_commonmark::CommonMarkCache;
use egui_hooks::UseHookExt;
use inline_tweak::tweak;
use std::borrow::Cow;
use std::ops::{Deref, DerefMut};
use strum::EnumIs;
use ustr::Ustr;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, EnumIs)]
pub enum SelectedDocs {
    #[default]
    None,
    Node(String),
    Type(ETypeId),
}

pub fn docs_tab(ui: &mut Ui, app: &mut DbeApp) {
    let mut selection = ui.use_state(|| SelectedDocs::None, ()).into_var();
    let mut search_text = ui.use_state(String::new, selection.clone()).into_var();
    ui.horizontal(|ui| {
        ui.label("Documentation");
        if ui
            .add_enabled(!selection.is_none(), egui::Button::new("Back"))
            .clicked()
        {
            *selection = SelectedDocs::None;
        }
    });

    let Some(project) = &app.project else {
        return;
    };

    ScrollArea::vertical().show(ui, |ui| match selection.deref_mut() {
        SelectedDocs::None => {
            TextEdit::singleline(search_text.deref_mut())
                .hint_text("Search")
                .ui(ui);

            let search_query = search_text.trim();
            let force_show = (!search_query.is_empty()).then_some(true);

            fn search<'a>(query: &str, title: &'a str, id: &'a str) -> (bool, Cow<'a, str>) {
                if query.is_empty() {
                    return (true, title.into());
                }
                let title_contains = title.contains(query);
                let id_contains = id.contains(query);
                let content = if !title_contains && id_contains {
                    Cow::Owned(format!("{} ({})", title, id))
                } else {
                    Cow::Borrowed(title)
                };
                (title_contains || id_contains, content)
            }

            CollapsingHeader::new("Nodes")
                .open(force_show)
                .show(ui, |ui| {
                    for (node_id, docs) in &project.docs.nodes {
                        let Some(_) = get_node_factory(&Ustr::from(node_id)) else {
                            continue;
                        };
                        let (found, name) = search(search_query, &docs.title, node_id);
                        if !found {
                            continue;
                        }
                        if ui.button(name).clicked() {
                            *selection = SelectedDocs::Node(node_id.clone());
                        }
                    }
                });

            CollapsingHeader::new("Types")
                .open(force_show)
                .show(ui, |ui| {
                    for (type_id, _) in &project.docs.types {
                        let Some(ty) = project.registry.get_object(type_id) else {
                            continue;
                        };
                        let title = ty.title(&project.registry);
                        let id = type_id.to_string();
                        let (found, title) = search(search_query, &title, &id);
                        if !found {
                            continue;
                        }
                        if ui.button(title).clicked() {
                            *selection = SelectedDocs::Type(*type_id);
                        }
                    }
                });
        }
        SelectedDocs::Node(name) => {
            if let (Some(docs), Some(node)) = (
                project.docs.nodes.get(name),
                get_node_factory(&Ustr::from(name)),
            ) {
                node_docs(ui, node.deref(), docs);
            } else {
                *selection = SelectedDocs::None;
            }
        }
        SelectedDocs::Type(ty) => {
            if let (Some(docs), Some(ty)) =
                (project.docs.types.get(ty), project.registry.get_object(ty))
            {
                type_docs(ui, &project.registry, ty, docs);
            } else {
                *selection = SelectedDocs::None;
            }
        }
    });
}

fn labeled_text(ui: &mut Ui, label: impl Into<WidgetText>, text: impl Into<WidgetText>) {
    ui.horizontal_wrapped(|ui| {
        ui.label(label);
        ui.label(text);
    });
}

fn show_description(ui: &mut Ui, docs: &impl DocsDescription, md_cache: &mut CommonMarkCache) {
    ui.label(docs.description());

    if !docs.docs().is_empty() {
        ui.label("");
        egui_commonmark::CommonMarkViewer::new().show(ui, md_cache, docs.docs());
    }
}

fn show_collapsing_description(
    ui: &mut Ui,
    docs: &impl DocsDescription,
    md_cache: &mut CommonMarkCache,
    title: &str,
) {
    CollapsingHeader::new(RichText::new(title).strong().size(tweak!(14.0)))
        .default_open(true)
        .show(ui, |ui| {
            show_description(ui, docs, md_cache);
        });
}

pub fn node_docs(ui: &mut Ui, node: &dyn NodeFactory, docs: &NodeDocs) {
    egui::Label::new(RichText::new(&docs.title).heading())
        .ui(ui)
        .on_hover_text(format!("ID: {}", node.id()));

    if !node.categories().is_empty() {
        labeled_text(ui, "Categories: ", node.categories().join(", "));
    }

    ui.separator();

    let mut md_cache = CommonMarkCache::default();

    show_description(ui, docs, &mut md_cache);

    ui.style_mut().visuals.indent_has_left_vline = false;
    if !docs.inputs.is_empty() {
        ui.label(RichText::new("Inputs").heading());
        ui.separator();
        for docs in &docs.inputs {
            show_collapsing_description(ui, docs, &mut md_cache, &docs.title);
        }
    }
    if !docs.outputs.is_empty() {
        ui.label(RichText::new("Outputs").heading());
        ui.separator();
        for docs in &docs.outputs {
            show_collapsing_description(ui, docs, &mut md_cache, &docs.title);
        }
    }
}

pub fn type_docs(ui: &mut Ui, registry: &ETypesRegistry, ty: &EObjectType, docs: &TypeDocs) {
    egui::Label::new(RichText::new(ty.title(registry)).heading())
        .ui(ui)
        .on_hover_text(format!("ID: {}", ty.ident()));

    ui.separator();

    let mut md_cache = CommonMarkCache::default();

    show_description(ui, docs, &mut md_cache);

    ui.style_mut().visuals.indent_has_left_vline = false;
    if !docs.variants.is_empty() {
        ui.label(RichText::new("Variants").heading());
        ui.separator();
        for docs in &docs.variants {
            show_collapsing_description(ui, docs, &mut md_cache, &docs.id);
        }
    }

    if !docs.fields.is_empty() {
        ui.label(RichText::new("Fields").heading());
        ui.separator();
        for docs in &docs.fields {
            show_collapsing_description(ui, docs, &mut md_cache, &docs.id);
        }
    }
}

fn show_window_ref(
    ui: &mut Ui,
    docs: &Docs,
    registry: &ETypesRegistry,
    window_ref: &DocsWindowRef,
) {
    let mut shown = false;
    match window_ref {
        DocsWindowRef::Node(node) => {
            if let (Some(docs), Some(factory)) =
                (docs.nodes.get(node.as_str()), get_node_factory(node))
            {
                node_docs(ui, &*factory, docs);
                shown = true;
            }
        }
        DocsWindowRef::Type(ty) => {
            if let (Some(docs), Some(ty)) = (docs.types.get(ty), registry.get_object(ty)) {
                type_docs(ui, registry, ty, docs);
                shown = true;
            }
        }
    }
    if !shown {
        ui.label("No documentation available");
    }
}

pub fn docs_label(
    ui: &mut Ui,
    label: &str,
    docs: &Docs,
    registry: &ETypesRegistry,
    docs_ref: DocsRef,
) {
    const NO_DOCS: &str = "No documentation available";
    let res = egui::Label::new(label).selectable(false).ui(ui);

    if docs_ref.is_none() {
        if cfg!(debug_assertions) {
            res.on_hover_text("DocsRef is None");
        }
        return;
    }

    let docs_window_ref = docs_ref.as_window_ref();

    let mut show_window = ui.use_state(|| false, docs_window_ref).into_var();

    res.on_hover_ui(|ui| {
        match &docs_ref {
            DocsRef::Custom(text) => {
                ui.label(*text);
            }
            DocsRef::None => {
                unreachable!()
            }
            _ if docs_ref.has_field_structure() => {
                let description = if let Some(desc) = docs_ref.get_description(docs) {
                    if desc.is_empty() {
                        NO_DOCS
                    } else {
                        desc
                    }
                } else {
                    NO_DOCS
                };
                let parent_title = docs_ref.get_parent_title(docs, registry);
                let field_title = docs_ref.get_field_title(docs);
                let label = match docs_ref {
                    DocsRef::NodeInput(_, _) => {
                        format!("{} <- {}", parent_title, field_title)
                    }
                    DocsRef::NodeOutput(_, _) => {
                        format!("{} -> {}", parent_title, field_title)
                    }
                    DocsRef::TypeField(_, _) => {
                        format!("{}.{}", parent_title, field_title)
                    }
                    DocsRef::Custom(_) | DocsRef::None => {
                        unreachable!()
                    }
                };

                ui.label(label);
                ui.separator();
                ui.label(description);
            }
            _ => {
                unimplemented!()
            }
        }

        if docs_window_ref.has_docs(docs) && ui.button("View full docs").clicked() {
            *show_window = true;
        }
    });

    if *show_window {
        Window::new(docs_window_ref.title(docs, registry))
            .id(ui.id().with(label).with("docs_window"))
            .open(&mut show_window)
            .default_height(300.0)
            .collapsible(false)
            .show(ui.ctx(), |ui| {
                ScrollArea::vertical().show(ui, |ui| {
                    show_window_ref(ui, docs, registry, &docs_window_ref);
                })
            });
    }
}
