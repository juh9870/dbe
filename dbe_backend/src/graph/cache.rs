use crate::value::EValue;
use egui_snarl::NodeId;
use utils::map::HashMap;

#[derive(Debug, Default)]
pub struct GraphCache {
    nodes: HashMap<NodeId, Vec<EValue>>,
}

impl GraphCache {
    pub fn clear(&mut self) {
        self.nodes.clear();
    }

    pub fn insert(&mut self, node: NodeId, values: Vec<EValue>) {
        self.nodes.insert(node, values);
    }

    pub fn get(&self, node: &NodeId) -> Option<&Vec<EValue>> {
        self.nodes.get(node)
    }

    pub fn contains_key(&self, node: &NodeId) -> bool {
        self.nodes.contains_key(node)
    }

    pub fn remove(&mut self, node: &NodeId) -> Option<Vec<EValue>> {
        self.nodes.remove(node)
    }
}
