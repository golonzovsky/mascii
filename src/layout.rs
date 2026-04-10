use crate::graph::{Edge, Graph, Node, NodeId, Shape};

const GAP: usize = 4;
const CHANNEL: usize = 3;

pub fn layout(mut g: Graph, padding: usize) -> Graph {
    compute_node_dims(&mut g, padding);
    assign_layers(&mut g);
    insert_dummies(&mut g);
    order_layers(&mut g, 8);
    assign_x(&mut g);
    assign_y(&mut g);
    g
}

fn compute_node_dims(g: &mut Graph, padding: usize) {
    for n in &mut g.nodes {
        if n.is_dummy {
            n.width = 1;
            n.height = 1;
            continue;
        }
        let label_w = n
            .label_lines
            .iter()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0);
        n.width = label_w + 2 * padding + 2;
        n.height = n.label_lines.len() + 2;
    }
}

fn assign_layers(g: &mut Graph) {
    let n = g.nodes.len();
    let mut indeg = vec![0usize; n];
    let mut adj: Vec<Vec<NodeId>> = vec![vec![]; n];
    for e in &g.edges {
        indeg[e.to] += 1;
        adj[e.from].push(e.to);
    }

    let mut layer = vec![0usize; n];
    let mut remaining = indeg.clone();
    let mut queue: Vec<NodeId> = (0..n).filter(|&i| remaining[i] == 0).collect();

    while let Some(u) = queue.pop() {
        for &v in &adj[u] {
            if layer[v] < layer[u] + 1 {
                layer[v] = layer[u] + 1;
            }
            remaining[v] -= 1;
            if remaining[v] == 0 {
                queue.push(v);
            }
        }
    }

    for i in 0..n {
        g.nodes[i].layer = layer[i];
    }
}

fn insert_dummies(g: &mut Graph) {
    let old_edges = std::mem::take(&mut g.edges);
    for e in old_edges {
        let from_layer = g.nodes[e.from].layer;
        let to_layer = g.nodes[e.to].layer;
        if to_layer <= from_layer + 1 {
            g.edges.push(e);
            continue;
        }
        // Split long edge into chain. Keep the label on the FIRST hop only,
        // so it renders in the channel directly below the source.
        let mut prev = e.from;
        let mut label = e.label.clone();
        for l in (from_layer + 1)..to_layer {
            let id = g.nodes.len();
            g.nodes.push(Node {
                id,
                name: format!("__dummy_{}", id),
                label_lines: vec![],
                is_dummy: true,
                shape: Shape::Round,
                width: 1,
                height: 1,
                layer: l,
                order: 0,
                x: 0,
                y: 0,
            });
            g.edges.push(Edge {
                from: prev,
                to: id,
                label: label.take(),
            });
            prev = id;
        }
        g.edges.push(Edge {
            from: prev,
            to: e.to,
            label: None,
        });
    }
}

fn order_layers(g: &mut Graph, iterations: usize) {
    let max_layer = g.nodes.iter().map(|n| n.layer).max().unwrap_or(0);
    let mut layers: Vec<Vec<NodeId>> = vec![vec![]; max_layer + 1];
    for n in &g.nodes {
        layers[n.layer].push(n.id);
    }
    for layer in &layers {
        for (i, &id) in layer.iter().enumerate() {
            g.nodes[id].order = i;
        }
    }

    let n = g.nodes.len();
    let mut preds: Vec<Vec<NodeId>> = vec![vec![]; n];
    let mut succs: Vec<Vec<NodeId>> = vec![vec![]; n];
    for e in &g.edges {
        succs[e.from].push(e.to);
        preds[e.to].push(e.from);
    }

    for _ in 0..iterations {
        for l in 1..=max_layer {
            sort_layer_by_neighbors(&mut layers[l], &preds, g);
            for (i, &id) in layers[l].iter().enumerate() {
                g.nodes[id].order = i;
            }
        }
        for l in (0..max_layer).rev() {
            sort_layer_by_neighbors(&mut layers[l], &succs, g);
            for (i, &id) in layers[l].iter().enumerate() {
                g.nodes[id].order = i;
            }
        }
    }
}

fn sort_layer_by_neighbors(layer: &mut Vec<NodeId>, neighbors: &[Vec<NodeId>], g: &Graph) {
    let mut keyed: Vec<(NodeId, f64, usize)> = layer
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            let ns = &neighbors[id];
            let bary = if ns.is_empty() {
                g.nodes[id].order as f64
            } else {
                ns.iter().map(|&ni| g.nodes[ni].order as f64).sum::<f64>() / ns.len() as f64
            };
            (id, bary, i)
        })
        .collect();
    keyed.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.2.cmp(&b.2))
    });
    *layer = keyed.into_iter().map(|(id, _, _)| id).collect();
}

fn assign_x(g: &mut Graph) {
    let max_layer = g.nodes.iter().map(|n| n.layer).max().unwrap_or(0);
    let mut layers: Vec<Vec<NodeId>> = vec![vec![]; max_layer + 1];
    for n in &g.nodes {
        layers[n.layer].push(n.id);
    }
    for layer in &mut layers {
        layer.sort_by_key(|&id| g.nodes[id].order);
    }

    // Initial pack
    let mut layer_x: Vec<Vec<usize>> = vec![vec![]; max_layer + 1];
    for (l, layer) in layers.iter().enumerate() {
        let mut x = 0usize;
        for &id in layer {
            layer_x[l].push(x);
            x += g.nodes[id].width + GAP;
        }
    }

    // Adjacency cache
    let n = g.nodes.len();
    let mut preds: Vec<Vec<NodeId>> = vec![vec![]; n];
    let mut succs: Vec<Vec<NodeId>> = vec![vec![]; n];
    for e in &g.edges {
        succs[e.from].push(e.to);
        preds[e.to].push(e.from);
    }

    let pos_in_layer = |layers: &Vec<Vec<NodeId>>, id: NodeId, lyr: usize| -> usize {
        layers[lyr].iter().position(|&x| x == id).unwrap()
    };

    // Down passes use per-node alignment to children's preds.
    // Up passes use a block-shift heuristic (move whole layer by avg of desired
    // offsets) — per-node up alignment causes positive-feedback drift when
    // siblings all target the same neighbor.
    for _ in 0..4 {
        for l in 1..=max_layer {
            align_layer(&layers, &mut layer_x, l, &preds, g, &pos_in_layer);
        }
        for l in (0..max_layer).rev() {
            block_shift_layer(&layers, &mut layer_x, l, &succs, g, &pos_in_layer);
        }
        normalize_x(&mut layer_x);
    }
    normalize_x(&mut layer_x);

    for (l, layer) in layers.iter().enumerate() {
        for (i, &id) in layer.iter().enumerate() {
            g.nodes[id].x = layer_x[l][i];
        }
    }
}

fn block_shift_layer<F>(
    layers: &Vec<Vec<NodeId>>,
    layer_x: &mut Vec<Vec<usize>>,
    l: usize,
    neighbors: &[Vec<NodeId>],
    g: &Graph,
    pos_in_layer: &F,
) where
    F: Fn(&Vec<Vec<NodeId>>, NodeId, usize) -> usize,
{
    let n = layers[l].len();
    if n == 0 {
        return;
    }
    let mut sum_delta: i64 = 0;
    let mut count: i64 = 0;
    for (i, &id) in layers[l].iter().enumerate() {
        let ns = &neighbors[id];
        if ns.is_empty() {
            continue;
        }
        let target_center: f64 = ns
            .iter()
            .map(|&ni| {
                let nl = g.nodes[ni].layer;
                let np = pos_in_layer(layers, ni, nl);
                (layer_x[nl][np] + g.nodes[ni].width / 2) as f64
            })
            .sum::<f64>()
            / ns.len() as f64;
        let half = g.nodes[id].width as f64 / 2.0;
        let target_x = (target_center - half).round() as i64;
        sum_delta += target_x - layer_x[l][i] as i64;
        count += 1;
    }
    if count == 0 {
        return;
    }
    let shift = (sum_delta as f64 / count as f64).round() as i64;
    if shift == 0 {
        return;
    }
    if shift > 0 {
        let s = shift as usize;
        for x in layer_x[l].iter_mut() {
            *x += s;
        }
    } else {
        let s = (-shift) as usize;
        // Don't shift below 0 — clamp at zero
        let leftmost = *layer_x[l].iter().min().unwrap();
        let actual = s.min(leftmost);
        for x in layer_x[l].iter_mut() {
            *x -= actual;
        }
    }
}

fn normalize_x(layer_x: &mut Vec<Vec<usize>>) {
    let global_min: usize = layer_x
        .iter()
        .flat_map(|v| v.iter())
        .copied()
        .min()
        .unwrap_or(0);
    if global_min == 0 {
        return;
    }
    for row in layer_x.iter_mut() {
        for x in row.iter_mut() {
            *x -= global_min;
        }
    }
}

fn align_layer<F>(
    layers: &Vec<Vec<NodeId>>,
    layer_x: &mut Vec<Vec<usize>>,
    l: usize,
    neighbors: &[Vec<NodeId>],
    g: &Graph,
    pos_in_layer: &F,
) where
    F: Fn(&Vec<Vec<NodeId>>, NodeId, usize) -> usize,
{
    let n = layers[l].len();
    if n == 0 {
        return;
    }
    // Compute target x for each node (left edge), or None if no neighbors
    let targets: Vec<Option<usize>> = layers[l]
        .iter()
        .map(|&id| {
            let ns = &neighbors[id];
            if ns.is_empty() {
                return None;
            }
            let target_center: f64 = ns
                .iter()
                .map(|&ni| {
                    let nl = g.nodes[ni].layer;
                    let np = pos_in_layer(layers, ni, nl);
                    (layer_x[nl][np] + g.nodes[ni].width / 2) as f64
                })
                .sum::<f64>()
                / ns.len() as f64;
            let half = g.nodes[id].width as f64 / 2.0;
            let t = if target_center < half {
                0
            } else {
                (target_center - half).round() as usize
            };
            Some(t)
        })
        .collect();

    let mut new_x: Vec<usize> = vec![0; n];
    // Forward: each node at max(target, lb_from_left)
    for i in 0..n {
        let target = targets[i].unwrap_or(layer_x[l][i]);
        let lb = if i == 0 {
            0
        } else {
            let prev_id = layers[l][i - 1];
            new_x[i - 1] + g.nodes[prev_id].width + GAP
        };
        new_x[i] = target.max(lb);
    }
    // Backward: pull each node leftward toward target if right neighbor allows
    for i in (0..n).rev() {
        let target = targets[i].unwrap_or(new_x[i]);
        let id = layers[l][i];
        let ub = if i == n - 1 {
            usize::MAX
        } else {
            new_x[i + 1].saturating_sub(g.nodes[id].width + GAP)
        };
        let lb = if i == 0 {
            0
        } else {
            let prev_id = layers[l][i - 1];
            new_x[i - 1] + g.nodes[prev_id].width + GAP
        };
        new_x[i] = target.min(ub).max(lb);
    }
    layer_x[l] = new_x;
}

fn assign_y(g: &mut Graph) {
    let max_layer = g.nodes.iter().map(|n| n.layer).max().unwrap_or(0);
    let mut layer_heights: Vec<usize> = vec![0; max_layer + 1];
    for n in &g.nodes {
        let h = if n.is_dummy { 1 } else { n.height };
        if h > layer_heights[n.layer] {
            layer_heights[n.layer] = h;
        }
    }

    // Compute channel heights: add a row when any edge leaving this layer
    // carries a label, so the label has its own row in the channel.
    let mut channel_heights: Vec<usize> = vec![CHANNEL; max_layer];
    for e in &g.edges {
        if e.label.is_some() {
            let l = g.nodes[e.from].layer;
            if l < max_layer {
                channel_heights[l] = channel_heights[l].max(CHANNEL + 1);
            }
        }
    }

    let mut y = 0usize;
    for l in 0..=max_layer {
        let lh = layer_heights[l];
        for n in &mut g.nodes {
            if n.layer == l {
                if n.is_dummy {
                    n.y = y;
                    n.height = lh;
                } else {
                    let offset = (lh - n.height) / 2;
                    n.y = y + offset;
                }
            }
        }
        y += lh;
        if l < max_layer {
            y += channel_heights[l];
        }
    }
}
