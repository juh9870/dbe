use crate::graph::inputs::{GraphInput, GraphOutput};
use crate::graph::Graph;
use crate::json_utils::JsonValue;
use crate::project::ProjectFile;
use crate::registry::ETypesRegistry;
use camino::Utf8PathBuf;
use miette::{bail, Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::collections::hash_map::Entry;
use std::hash::Hash;
use strum::EnumIs;
use utils::map::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Hash)]
pub struct ProjectGraph {
    pub id: Uuid,
    pub name: String,
    /// Whether the graph is a node group
    pub is_node_group: bool,
    /// Whenever the graph should be hidden from search
    pub hide_from_search: bool,
    /// Categories of the graph
    pub categories: Vec<String>,
    graph: GraphHolder,
    inputs_cache: SmallVec<[GraphInput; 1]>,
    outputs_cache: SmallVec<[GraphOutput; 1]>,
}

#[derive(Debug, EnumIs)]
enum GraphHolder {
    Graph(Box<Graph>),
    Editing,
}

impl Clone for GraphHolder {
    fn clone(&self) -> Self {
        match self {
            GraphHolder::Graph(g) => GraphHolder::Graph(g.clone()),
            GraphHolder::Editing => panic!("Cannot clone graph: graph is being edited"),
        }
    }
}

impl Hash for GraphHolder {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            GraphHolder::Graph(g) => g.hash(state),
            GraphHolder::Editing => panic!("Cannot hash graph: graph is being edited"),
        }
    }
}

impl ProjectGraph {
    pub fn new(id: Uuid) -> Self {
        Self {
            id,
            name: "".to_string(),
            is_node_group: false,
            hide_from_search: false,
            categories: Default::default(),
            graph: GraphHolder::Graph(Box::default()),
            inputs_cache: Default::default(),
            outputs_cache: Default::default(),
        }
    }

    /// Get the graph data
    pub fn graph(&self) -> &Graph {
        match &self.graph {
            GraphHolder::Graph(g) => g,
            GraphHolder::Editing => panic!("Cannot borrow graph: graph is being edited"),
        }
    }

    /// Get the graph data mutably
    pub fn graph_mut(&mut self) -> &mut Graph {
        match &mut self.graph {
            GraphHolder::Graph(g) => g,
            GraphHolder::Editing => panic!("Cannot borrow graph: graph is being edited"),
        }
    }

    /// Get the graph inputs
    pub fn inputs(&self) -> &[GraphInput] {
        match self.graph {
            GraphHolder::Graph(ref g) => g.inputs(),
            GraphHolder::Editing => &self.inputs_cache,
        }
    }

    /// Get the graph outputs
    pub fn outputs(&self) -> &[GraphOutput] {
        match self.graph {
            GraphHolder::Graph(ref g) => g.outputs(),
            GraphHolder::Editing => &self.outputs_cache,
        }
    }

    pub fn display_name(&self) -> String {
        let trimmed = self.name.trim();

        if trimmed.is_empty() {
            format!("Graph {:8}", self.id)
        } else {
            trimmed.to_string()
        }
    }

    pub fn parse_json(registry: &ETypesRegistry, value: &mut JsonValue) -> miette::Result<Self> {
        let data = if value.get("version").is_none() {
            SerializedGraphRepr::V0(value.take())
        } else {
            SerializedGraphRepr::deserialize(value.take()).into_diagnostic()?
        };

        match data {
            SerializedGraphRepr::V1(mut data) => {
                let graph = Graph::parse_json(registry, &mut data.graph)
                    .context("failed to deserialize graph data")?;

                Ok(Self {
                    id: data.id,
                    name: data.name,
                    is_node_group: data.is_node_group,
                    hide_from_search: data.hide_from_search,
                    categories: data.categories,
                    graph: GraphHolder::Graph(Box::new(graph)),
                    inputs_cache: Default::default(),
                    outputs_cache: Default::default(),
                })
            }
            SerializedGraphRepr::V0(mut data) => {
                // Legacy raw graph data

                if let Some(obj) = data.as_object_mut() {
                    // Patch renamed field
                    if let Some(inputs) = obj.remove("inputs") {
                        obj.insert("inline_values".to_string(), inputs);
                    }
                }

                let graph = Graph::parse_json(registry, dbg!(&mut data))
                    .context("failed to deserialize graph data")?;

                Ok(Self {
                    id: Uuid::new_v4(),
                    name: "".to_string(),
                    is_node_group: false,
                    hide_from_search: false,
                    categories: Default::default(),
                    graph: GraphHolder::Graph(Box::new(graph)),
                    inputs_cache: Default::default(),
                    outputs_cache: Default::default(),
                })
            }
        }
    }

    pub fn write_json(&self, registry: &ETypesRegistry) -> miette::Result<JsonValue> {
        let graph = self.graph().write_json(registry)?;

        let serialized = SerializedGraphRepr::V1(PackedProjectGraph {
            id: self.id,
            is_node_group: self.is_node_group,
            hide_from_search: self.hide_from_search,
            categories: self.categories.clone(),
            name: self.name.clone(),
            graph,
        });

        serde_json::value::to_value(&serialized)
            .into_diagnostic()
            .with_context(|| "failed to serialize graph")
    }

    fn take_graph(&mut self) -> Box<Graph> {
        match std::mem::replace(&mut self.graph, GraphHolder::Editing) {
            GraphHolder::Graph(g) => {
                self.inputs_cache = g.inputs().clone();
                self.outputs_cache = g.outputs().clone();
                g
            }
            GraphHolder::Editing => panic!("Cannot take graph: graph is being edited"),
        }
    }

    fn return_graph(&mut self, graph: Box<Graph>) {
        if !self.graph.is_editing() {
            panic!("Cannot return graph: graph is not being edited");
        }
        self.graph = GraphHolder::Graph(graph);
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct PackedProjectGraph {
    #[serde(default = "default_uuid")]
    id: Uuid,
    #[serde(default)]
    is_node_group: bool,
    #[serde(default)]
    hide_from_search: bool,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    name: String,
    graph: JsonValue,
}

fn default_uuid() -> Uuid {
    Uuid::new_v4()
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "version")]
enum SerializedGraphRepr {
    V1(PackedProjectGraph),
    V0(JsonValue),
}

#[derive(Debug, Default)]
pub struct ProjectGraphs {
    /// All project graphs
    pub graphs: HashMap<Uuid, ProjectGraph>,
    paths: HashMap<Uuid, Utf8PathBuf>,
}

impl ProjectGraphs {
    pub fn edit_graph<Fn: FnOnce(&mut Graph, &Self) -> R, R>(&mut self, id: Uuid, func: Fn) -> R {
        // Take the graph data out of the graphs map
        let graph = self.graphs.get_mut(&id).unwrap();
        let mut taken = graph.take_graph();

        // Run the callback
        let result = func(&mut taken, self);

        let graph = self.graphs.get_mut(&id).unwrap();
        graph.return_graph(taken);
        result
    }

    pub fn insert_new_graph(&mut self) -> Uuid {
        let id = loop {
            let id = Uuid::new_v4();
            if !self.graphs.contains_key(&id) {
                break id;
            }
        };

        self.graphs.insert(id, ProjectGraph::new(id));

        id
    }

    pub fn add_graph(
        &mut self,
        path: Utf8PathBuf,
        graph: ProjectGraph,
    ) -> miette::Result<ProjectFile> {
        let id = graph.id;
        match self.graphs.entry(id) {
            Entry::Occupied(_) => {
                let other_path = self
                    .paths
                    .get(&id)
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "unknown file".to_string());
                bail!(
                    "graph with id {:?} already exists at `{}`. Were graph files copied manually?",
                    id,
                    other_path
                );
            }
            Entry::Vacant(e) => {
                e.insert(graph);
            }
        }

        self.paths.insert(id, path);

        Ok(ProjectFile::Graph(id))
    }
}
