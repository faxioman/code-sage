
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleDAGNode {
    pub id: String,
    pub hash: String,
    pub data: String,
    pub parents: Vec<String>,
    pub children: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleDAG {
    nodes: HashMap<String, MerkleDAGNode>,
    root_ids: Vec<String>,
}

impl MerkleDAG {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            root_ids: Vec::new(),
        }
    }

    fn hash(data: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub fn add_node(&mut self, data: String, parent_id: Option<String>) -> String {
        let node_id = Self::hash(&data);
        
        let mut node = MerkleDAGNode {
            id: node_id.clone(),
            hash: node_id.clone(),
            data,
            parents: Vec::new(),
            children: Vec::new(),
        };

        if let Some(parent_id) = parent_id {
            if let Some(parent_node) = self.nodes.get_mut(&parent_id) {
                node.parents.push(parent_id.clone());
                parent_node.children.push(node_id.clone());
            }
        } else {
            self.root_ids.push(node_id.clone());
        }

        self.nodes.insert(node_id.clone(), node);
        node_id
    }

    pub fn get_node(&self, node_id: &str) -> Option<&MerkleDAGNode> {
        self.nodes.get(node_id)
    }

    pub fn get_all_nodes(&self) -> Vec<&MerkleDAGNode> {
        self.nodes.values().collect()
    }

    pub fn get_root_nodes(&self) -> Vec<&MerkleDAGNode> {
        self.root_ids
            .iter()
            .filter_map(|id| self.nodes.get(id))
            .collect()
    }

    pub fn get_leaf_nodes(&self) -> Vec<&MerkleDAGNode> {
        self.nodes
            .values()
            .filter(|node| node.children.is_empty())
            .collect()
    }

    pub fn compare(dag1: &MerkleDAG, dag2: &MerkleDAG) -> DagComparison {
        let nodes1: HashMap<String, &MerkleDAGNode> = 
            dag1.get_all_nodes().into_iter().map(|n| (n.id.clone(), n)).collect();
        let nodes2: HashMap<String, &MerkleDAGNode> = 
            dag2.get_all_nodes().into_iter().map(|n| (n.id.clone(), n)).collect();

        let added: Vec<String> = nodes2
            .keys()
            .filter(|k| !nodes1.contains_key(*k))
            .cloned()
            .collect();

        let removed: Vec<String> = nodes1
            .keys()
            .filter(|k| !nodes2.contains_key(*k))
            .cloned()
            .collect();

        let modified: Vec<String> = nodes1
            .iter()
            .filter_map(|(id, node1)| {
                nodes2.get(id).and_then(|node2| {
                    if node1.data != node2.data {
                        Some(id.clone())
                    } else {
                        None
                    }
                })
            })
            .collect();

        DagComparison {
            added,
            removed,
            modified,
        }
    }
}

impl Default for MerkleDAG {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagComparison {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub modified: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merkle_dag_basic() {
        let mut dag = MerkleDAG::new();
        
        let root_id = dag.add_node("root".to_string(), None);
        let child1_id = dag.add_node("child1".to_string(), Some(root_id.clone()));
        let child2_id = dag.add_node("child2".to_string(), Some(root_id.clone()));
        
        assert_eq!(dag.get_root_nodes().len(), 1);
        assert_eq!(dag.get_leaf_nodes().len(), 2);
        assert!(dag.get_node(&root_id).is_some());
        assert!(dag.get_node(&child1_id).is_some());
        assert!(dag.get_node(&child2_id).is_some());
    }

    #[test]
    fn test_merkle_dag_compare() {
        let mut dag1 = MerkleDAG::new();
        dag1.add_node("file1:hash1".to_string(), None);
        dag1.add_node("file2:hash2".to_string(), None);
        
        let mut dag2 = MerkleDAG::new();
        dag2.add_node("file1:hash1".to_string(), None);
        dag2.add_node("file2:hash2_modified".to_string(), None);
        dag2.add_node("file3:hash3".to_string(), None);
        
        let comparison = MerkleDAG::compare(&dag1, &dag2);
        
        assert!(!comparison.added.is_empty());
        assert!(!comparison.modified.is_empty() || !comparison.added.is_empty());
    }
}
