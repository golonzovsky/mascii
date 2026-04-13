use crate::style::Style;
use std::collections::HashMap;

pub type NodeId = usize;
pub type SubgraphId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    TD,
    BT,
    LR,
    RL,
}

impl Direction {
    /// Whether flow runs along the vertical (y) or horizontal (x) axis.
    pub fn is_vertical(self) -> bool {
        matches!(self, Direction::TD | Direction::BT)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shape {
    #[default]
    Round,
    Square,
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
    pub x: usize,
    pub y: usize,
    /// User-supplied style (via `style X ..` / `classDef` / `class X ..`).
    pub style: Style,
    /// Enclosing subgraph, if any.
    pub subgraph: Option<SubgraphId>,
}

#[derive(Debug, Clone)]
pub struct Subgraph {
    pub name: String,
    pub label: String,
    pub parent: Option<SubgraphId>,
    pub style: Style,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EdgeStyle {
    #[default]
    Normal,
    Thick,
    Dotted,
    Invisible,
}

impl EdgeStyle {
    // Dominance for merging cells touched by multiple edges: Thick > Dotted > Normal.
    pub fn max_over(self, other: Self) -> Self {
        fn rank(s: EdgeStyle) -> u8 {
            match s {
                EdgeStyle::Thick => 3,
                EdgeStyle::Dotted => 2,
                EdgeStyle::Normal => 1,
                EdgeStyle::Invisible => 0,
            }
        }
        if rank(self) >= rank(other) { self } else { other }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArrowTip {
    None,
    #[default]
    Arrow,
    Cross,
    Circle,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    pub label: Option<String>,
    pub style: EdgeStyle,
    pub tip_fwd: ArrowTip,
    pub tip_back: bool,
    /// Edge "rank" — 1 for a base `-->`, 2 for `--->`, 3 for `---->`, etc.
    /// Used during layering to stretch long edges across more layers.
    pub length: usize,
}

#[derive(Debug, Default)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub name_to_id: HashMap<String, NodeId>,
    pub dir: Direction,
    pub subgraphs: Vec<Subgraph>,
    pub class_defs: HashMap<String, Style>,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, name: &str, label_lines: Vec<String>, shape: Shape) -> NodeId {
        if let Some(&id) = self.name_to_id.get(name) {
            let has_real_label = !label_lines.iter().all(|l| l.is_empty());
            let n = &mut self.nodes[id];
            if has_real_label
                && n.label_lines.len() == 1
                && n.label_lines[0] == n.name
            {
                n.label_lines = label_lines;
                n.shape = shape;
            }
            return id;
        }
        let id = self.nodes.len();
        let label_lines = if label_lines.iter().all(|l| l.is_empty()) {
            vec![name.to_string()]
        } else {
            label_lines
        };
        self.nodes.push(Node {
            id,
            name: name.to_string(),
            label_lines,
            is_dummy: false,
            shape,
            width: 0,
            height: 0,
            x: 0,
            y: 0,
            style: Style::new(),
            subgraph: None,
        });
        self.name_to_id.insert(name.to_string(), id);
        id
    }

    /// Is `node` a member of `sid` (directly or via a nested parent)?
    pub fn node_in_subgraph(&self, node: NodeId, sid: SubgraphId) -> bool {
        let mut cur = self.nodes[node].subgraph;
        while let Some(s) = cur {
            if s == sid {
                return true;
            }
            cur = self.subgraphs[s].parent;
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_edge(
        &mut self,
        from: NodeId,
        to: NodeId,
        label: Option<String>,
        style: EdgeStyle,
        tip_fwd: ArrowTip,
        tip_back: bool,
        length: usize,
    ) {
        self.edges.push(Edge {
            from,
            to,
            label,
            style,
            tip_fwd,
            tip_back,
            length: length.max(1),
        });
    }
}
