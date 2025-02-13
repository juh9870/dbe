use crate::ui_props::PROP_OBJECT_GRAPH_SEARCH_HIDE;
use dbe_backend::etype::eobject::EObject;
use dbe_backend::etype::EDataType;
use dbe_backend::graph::node::creation::NodeCombo;
use dbe_backend::graph::node::generic::destructuring::DestructuringNodeFactory;
use dbe_backend::graph::node::ports::{InputData, OutputData};
use dbe_backend::graph::node::{all_node_factories, node_factories_by_category, NodeFactory};
use dbe_backend::project::project_graph::ProjectGraphs;
use dbe_backend::registry::ETypesRegistry;
use egui::{ScrollArea, TextEdit, TextStyle, Ui};
use egui_hooks::UseHookExt;
use inline_tweak::tweak;
use itertools::{Itertools, Position};
use nucleo::pattern::{CaseMatching, Normalization};
use nucleo::{Item, Nucleo, Snapshot};
use std::collections::BTreeMap;
use std::hash::Hash;
use std::ops::{Deref, DerefMut, Range};
use std::sync::Arc;

pub fn category_tree(
    graphs: Option<&ProjectGraphs>,
    registry: &ETypesRegistry,
) -> BTreeMap<String, Vec<NodeCombo>> {
    let mut categories: BTreeMap<String, Vec<NodeCombo>> = node_factories_by_category()
        .iter()
        .map(|(category, factories)| {
            (
                category.to_string(),
                factories
                    .iter()
                    .map(|f| NodeCombo::Factory(f.id()))
                    .collect_vec(),
            )
        })
        .collect();

    if let Some(graphs) = graphs {
        for (id, graph) in &graphs.graphs {
            if !graph.is_node_group {
                continue;
            }

            for category in &graph.categories {
                categories
                    .entry(category.clone())
                    .or_default()
                    .push(NodeCombo::Subgraph(*id, graph.name.clone()));
            }
        }
    }

    for obj in registry
        .all_ready_objects()
        .filter(|obj| !PROP_OBJECT_GRAPH_SEARCH_HIDE.get(obj.extra_properties(), false))
    {
        if !obj.generic_arguments_names().is_empty() {
            continue;
        }
        let title = obj.title(registry);
        let id = obj.ident();
        let category = ["types"]
            .into_iter()
            .chain(id.as_raw().unwrap().split([':', '/', '.']))
            .with_position()
            .take_while(|x| x.0 != Position::Last)
            .map(|x| x.1)
            .join(".");

        categories
            .entry(category)
            .or_default()
            .push(NodeCombo::Object(id, title));
    }

    categories
}

#[derive(Clone)]
pub struct GraphSearch(Arc<parking_lot::RwLock<SearchData>>);

struct SearchData {
    engine: Nucleo<NodeCombo>,
    last_query: Option<String>,
}

impl GraphSearch {
    pub fn empty() -> Self {
        let nucleo = init_nucleo();
        let data = SearchData {
            engine: nucleo,
            last_query: None,
        };

        GraphSearch(Arc::new(parking_lot::RwLock::new(data)))
    }

    pub fn all_nodes(
        graphs: Option<&ProjectGraphs>,
        registry: &ETypesRegistry,
        filter: impl Fn(&NodeCombo) -> bool,
    ) -> GraphSearch {
        let nucleo = init_nucleo();

        let injector = nucleo.injector();
        let factories = all_node_factories();
        let all_nodes = factories.iter().map(|(id, _)| NodeCombo::Factory(*id));

        if let Some(graphs) = graphs {
            for node in graphs
                .graphs
                .iter()
                .filter(|x| x.1.is_node_group && !x.1.hide_from_search)
                .map(|x| NodeCombo::Subgraph(*x.0, x.1.name.clone()))
                .filter(|x| filter(x))
            {
                push_to_injector(&injector, node);
            }
        }

        let objects = registry
            .all_ready_objects()
            .filter(|obj| !PROP_OBJECT_GRAPH_SEARCH_HIDE.get(obj.extra_properties(), false))
            .map(|s| {
                let title = s.title(registry);
                NodeCombo::Object(s.ident(), title)
            });
        for node in all_nodes.chain(objects).filter(|x| filter(x)) {
            push_to_injector(&injector, node);
        }

        let data = SearchData {
            engine: nucleo,
            last_query: None,
        };

        GraphSearch(Arc::new(parking_lot::RwLock::new(data)))
    }

    pub fn for_input_data(
        _graphs: Option<&ProjectGraphs>,
        registry: &ETypesRegistry,
        input: &InputData,
    ) -> Self {
        if !input.ty.is_specific() {
            return Self::empty();
        }

        let nucleo = init_nucleo();
        let injector = nucleo.injector();

        let ty = input.ty.ty();
        match ty {
            EDataType::Object { ident } => {
                let obj = registry.get_object(&ident).unwrap();
                let title = obj.title(registry);
                push_to_injector(&injector, NodeCombo::Object(ident, title));
            }
            EDataType::List { id } => {
                push_to_injector(&injector, NodeCombo::List(id));
            }
            _ => {}
        };

        let factories = all_node_factories();

        for (id, factory) in factories.iter() {
            if factory.output_port_for(ty, registry).is_some() {
                push_to_injector(&injector, NodeCombo::Factory(*id));
            }
        }

        let data = SearchData {
            engine: nucleo,
            last_query: None,
        };

        GraphSearch(Arc::new(parking_lot::RwLock::new(data)))
    }

    pub fn for_output_data(
        _graphs: Option<&ProjectGraphs>,
        registry: &ETypesRegistry,
        input: &OutputData,
    ) -> Self {
        if !input.ty.is_specific() {
            return Self::empty();
        }

        let nucleo = init_nucleo();
        let injector = nucleo.injector();

        let ty = input.ty.ty();
        match ty {
            EDataType::Object { ident } => {
                if registry.get_struct(&ident).is_some() {
                    push_to_injector(&injector, NodeCombo::Factory(DestructuringNodeFactory.id()));
                }
            }
            EDataType::List { id } => {
                push_to_injector(&injector, NodeCombo::List(id));
            }
            _ => {}
        };

        let factories = all_node_factories();

        for (id, factory) in factories.iter() {
            if factory.input_port_for(ty, registry).is_some() {
                push_to_injector(&injector, NodeCombo::Factory(*id));
            }
        }

        let data = SearchData {
            engine: nucleo,
            last_query: None,
        };

        GraphSearch(Arc::new(parking_lot::RwLock::new(data)))
    }

    pub fn tick(&self, timeout: u64) {
        self.0.write().engine.tick(timeout);
    }

    pub fn apply_search(&self, search_query: &str) {
        let mut data = self.0.write();
        let trimmed = search_query.trim();
        if data.last_query.as_ref().is_some_and(|q| q == trimmed) {
            return;
        }

        data.last_query = Some(trimmed.to_string());

        data.engine.pattern.reparse(
            0,
            search_query.trim(),
            CaseMatching::Ignore,
            Normalization::Smart,
            false,
        )
    }

    pub fn snapshot(&self) -> impl SearchSnapshot + '_ {
        parking_lot::RwLockReadGuard::map(self.0.read(), |guard| guard.engine.snapshot())
    }
}

pub fn search_ui(
    ui: &mut Ui,
    id: impl Hash,
    search: GraphSearch,
    no_search_ui: impl FnOnce(&mut Ui) -> Option<NodeCombo>,
) -> Option<NodeCombo> {
    dyn_search_ui(ui, id, search, Some(Box::new(no_search_ui)))
}

pub fn search_ui_always(ui: &mut Ui, id: impl Hash, search: GraphSearch) -> Option<NodeCombo> {
    dyn_search_ui(ui, id, search, None)
}

type DynUiCb<'c> = Box<dyn FnOnce(&mut Ui) -> Option<NodeCombo> + 'c>;

fn dyn_search_ui(
    ui: &mut Ui,
    id: impl Hash,
    search: GraphSearch,
    no_search_ui: Option<DynUiCb>,
) -> Option<NodeCombo> {
    ui.push_id(id, |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        search.tick(10);

        let scale = ui.ctx().style().spacing.combo_width / ui.style().spacing.combo_width;
        let min_size = tweak!(300.0) / scale;
        let searchbar_galley_width = ui.use_state(|| min_size, ());

        let mut search_query = ui.use_state(|| "".to_string(), ()).into_var();
        let bar_id = ui.id().with("search_bar");
        let bar = TextEdit::singleline(search_query.deref_mut())
            .id(bar_id)
            .desired_width(*searchbar_galley_width + 8.0);
        let scroll = ScrollArea::horizontal()
            .id_salt("bar_scroll")
            .max_width(*searchbar_galley_width + 8.0);

        let search_bar = scroll.show(ui, |ui| bar.show(ui)).inner;
        ui.use_effect(
            || {
                search_bar.response.request_focus();
            },
            (),
        );
        searchbar_galley_width.set_next(search_bar.galley.size().y.max(min_size));

        ui.use_effect(
            || {
                search.apply_search(&search_query);
            },
            search_query.trim().to_owned(),
        );

        let snapshot = search.snapshot();

        if search_query.trim() == "" {
            if let Some(no_search_ui) = no_search_ui {
                return no_search_ui(ui);
            }
        }

        let row_height =
            ui.style().spacing.button_padding.y + ui.text_style_height(&TextStyle::Button);

        ui.add_space(4.0);

        if let Some(node) = ScrollArea::vertical()
            .min_scrolled_height(row_height * 10.0)
            .show_rows(
                ui,
                row_height,
                snapshot.matched_item_count(),
                |ui, range| {
                    for node in snapshot.matched_items_range(range) {
                        let node = node.data;
                        let name = node.display_title();
                        let btn = ui.button(name);
                        if btn.clicked() {
                            return Some(node.clone());
                        }
                    }
                    None
                },
            )
            .inner
        {
            return Some(node);
        }

        None
    })
    .inner
}

pub trait SearchSnapshot {
    fn matched_item_count(&self) -> usize;

    fn matched_items_range(
        &self,
        range: Range<usize>,
    ) -> impl ExactSizeIterator<Item = Item<'_, NodeCombo>> + DoubleEndedIterator + '_;
}

impl<'a> SearchSnapshot for parking_lot::MappedRwLockReadGuard<'a, Snapshot<NodeCombo>> {
    fn matched_item_count(&self) -> usize {
        Snapshot::matched_item_count(self) as usize
    }

    fn matched_items_range(
        &self,
        range: Range<usize>,
    ) -> impl ExactSizeIterator<Item = Item<'_, NodeCombo>> + DoubleEndedIterator + '_ {
        self.deref()
            .matched_items((range.start as u32)..(range.end as u32))
    }
}

fn init_nucleo() -> Nucleo<NodeCombo> {
    let mut cfg = nucleo::Config::DEFAULT;
    cfg.set_match_paths();
    Nucleo::<NodeCombo>::new(cfg, Arc::new(|| {}), None, 1)
}

fn push_to_injector(injector: &nucleo::Injector<NodeCombo>, node: NodeCombo) {
    injector.push(node, |i, col| {
        col[0] = i.display_search().to_string().into();
    });
}
