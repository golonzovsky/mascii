use std::collections::HashMap;

pub type NodeId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Round,  // (...) and {...} and bare identifiers
    Square, // [...]
}

impl Default for Shape {
    fn default() -> Self {
        Shape::Round
    }
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub label_lines: Vec<String>,
    pub is_dummy: bool,
    pub shape: Shape,
    pub width: usize,
    pub height: usize,
    pub layer: usize,
    pub order: usize,
    pub x: usize,
    pub y: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeStyle {
    Normal,    // -->
    Thick,     // ==>
    Dotted,    // -.->
    Invisible, // ~~~
}

impl Default for EdgeStyle {
    fn default() -> Self {
        EdgeStyle::Normal
    }
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    pub label: Option<String>,
    pub style: EdgeStyle,
}

#[derive(Debug, Default)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub name_to_id: HashMap<String, NodeId>,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, name: &str, label: &str, shape: Shape) -> NodeId {
        if let Some(&id) = self.name_to_id.get(name) {
            let n = &mut self.nodes[id];
            if !label.is_empty()
                && n.label_lines.len() == 1
                && n.label_lines[0] == n.name
            {
                n.label_lines = vec![label.to_string()];
                // A named shape declaration wins over the default Round.
                n.shape = shape;
            }
            return id;
        }
        let id = self.nodes.len();
        let label_lines = if label.is_empty() {
            vec![name.to_string()]
        } else {
            vec![label.to_string()]
        };
        self.nodes.push(Node {
            id,
            name: name.to_string(),
            label_lines,
            is_dummy: false,
            shape,
            width: 0,
            height: 0,
            layer: 0,
            order: 0,
            x: 0,
            y: 0,
        });
        self.name_to_id.insert(name.to_string(), id);
        id
    }

    pub fn add_edge(
        &mut self,
        from: NodeId,
        to: NodeId,
        label: Option<String>,
        style: EdgeStyle,
    ) {
        self.edges.push(Edge {
            from,
            to,
            label,
            style,
        });
    }
}
