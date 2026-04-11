use crate::graph::{ArrowTip, Direction, Edge, Graph, Node, NodeId, Shape};

// LR-only: horizontal `─` padding on each side of an inline edge label.
pub const LR_LABEL_PAD: usize = 2;

// Within-layer packing gap. Horizontal in TD/BT (4 cols feels comfortable),
// vertical in LR/RL (3 rows lines up with standard 3-row node heights so
// single-node layers sit exactly centered).
fn minor_gap(dir: Direction) -> usize {
    if dir.is_vertical() { 4 } else { 3 }
}

pub fn layout(mut g: Graph, padding: usize) -> Graph {
    compute_node_dims(&mut g, padding);
    assign_layers(&mut g);
    insert_dummies(&mut g);
    order_layers(&mut g, 8);

    let dir = g.dir;
    // Run as TD (or LR if horizontal). BT/RL are handled as a post-render
    // canvas flip in render.rs.
    let horizontal = !dir.is_vertical();
    if horizontal {
        for n in &mut g.nodes {
            std::mem::swap(&mut n.width, &mut n.height);
        }
    }
    assign_x(&mut g, minor_gap(dir));
    assign_y(&mut g);
    push_subgraphs_clear_of_neighbors(&mut g);
    if horizontal {
        for n in &mut g.nodes {
            std::mem::swap(&mut n.x, &mut n.y);
            std::mem::swap(&mut n.width, &mut n.height);
        }
    }
    g
}

/// After x/y positions are assigned, give each subgraph a little breathing
/// room: if any non-member node's right edge is within the container's left
/// padding zone, shift every member of that subgraph rightward until the
/// container border has a 1-cell visible gap from the neighbor.
fn push_subgraphs_clear_of_neighbors(g: &mut Graph) {
    // Matches the padding used by the renderer when it draws containers.
    const CONTAINER_PAD_X: usize = 2;
    const MIN_GAP: usize = 1;

    for sid in 0..g.subgraphs.len() {
        let members: Vec<NodeId> = g
            .nodes
            .iter()
            .filter(|n| !n.is_dummy && g.node_in_subgraph(n.id, sid))
            .map(|n| n.id)
            .collect();
        if members.is_empty() {
            continue;
        }
        let min_x = members.iter().map(|&id| g.nodes[id].x).min().unwrap();
        // Needs: container_left = min_x - PAD_X >= neighbor_right + 1 + MIN_GAP
        let neighbor_right = g
            .nodes
            .iter()
            .filter(|n| !n.is_dummy && !g.node_in_subgraph(n.id, sid))
            .filter(|n| n.x + n.width - 1 < min_x)
            .map(|n| n.x + n.width - 1)
            .max();
        let Some(nr) = neighbor_right else { continue };
        let needed_min_x = nr + 1 + MIN_GAP + CONTAINER_PAD_X;
        if min_x < needed_min_x {
            let delta = needed_min_x - min_x;
            for &id in &members {
                g.nodes[id].x += delta;
            }
        }
    }
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
    // Adjacency list carrying (target, edge length).
    let mut adj: Vec<Vec<(NodeId, usize)>> = vec![vec![]; n];
    for e in &g.edges {
        indeg[e.to] += 1;
        adj[e.from].push((e.to, e.length));
    }

    let mut layer = vec![0usize; n];
    let mut remaining = indeg.clone();
    let mut queue: Vec<NodeId> = (0..n).filter(|&i| remaining[i] == 0).collect();

    while let Some(u) = queue.pop() {
        for &(v, len) in &adj[u] {
            if layer[v] < layer[u] + len {
                layer[v] = layer[u] + len;
            }
            remaining[v] -= 1;
            if remaining[v] == 0 {
                queue.push(v);
            }
        }
    }

    for (i, l) in layer.iter().enumerate() {
        g.nodes[i].layer = *l;
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
        // Label stays on the first hop only.
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
                style: crate::style::Style::new(),
                subgraph: None,
            });
            g.edges.push(Edge {
                from: prev,
                to: id,
                label: label.take(),
                style: e.style,
                tip_fwd: ArrowTip::None,
                tip_back: false,
                length: 1,
            });
            prev = id;
        }
        g.edges.push(Edge {
            from: prev,
            to: e.to,
            label: None,
            style: e.style,
            tip_fwd: e.tip_fwd,
            tip_back: e.tip_back,
            length: 1,
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
        #[allow(clippy::needless_range_loop)]
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

fn sort_layer_by_neighbors(layer: &mut [NodeId], neighbors: &[Vec<NodeId>], g: &Graph) {
    layer.sort_by(|&a, &b| {
        let ba = barycenter(a, neighbors, g);
        let bb = barycenter(b, neighbors, g);
        ba.partial_cmp(&bb).unwrap_or(std::cmp::Ordering::Equal)
    });
}

fn barycenter(id: NodeId, neighbors: &[Vec<NodeId>], g: &Graph) -> f64 {
    let ns = &neighbors[id];
    if ns.is_empty() {
        g.nodes[id].order as f64
    } else {
        ns.iter().map(|&ni| g.nodes[ni].order as f64).sum::<f64>() / ns.len() as f64
    }
}

fn assign_x(g: &mut Graph, gap: usize) {
    let max_layer = g.nodes.iter().map(|n| n.layer).max().unwrap_or(0);
    let mut layers: Vec<Vec<NodeId>> = vec![vec![]; max_layer + 1];
    for n in &g.nodes {
        layers[n.layer].push(n.id);
    }
    for layer in &mut layers {
        layer.sort_by_key(|&id| g.nodes[id].order);
    }

    let mut layer_x: Vec<Vec<usize>> = vec![vec![]; max_layer + 1];
    for (l, layer) in layers.iter().enumerate() {
        let mut x = 0usize;
        for &id in layer {
            layer_x[l].push(x);
            x += g.nodes[id].width + gap;
        }
    }

    let n = g.nodes.len();
    let mut preds: Vec<Vec<NodeId>> = vec![vec![]; n];
    let mut succs: Vec<Vec<NodeId>> = vec![vec![]; n];
    for e in &g.edges {
        succs[e.from].push(e.to);
        preds[e.to].push(e.from);
    }

    // Down passes align per-node to preds; up passes block-shift whole layer
    // (per-node up-alignment causes positive-feedback drift).
    for _ in 0..4 {
        for l in 1..=max_layer {
            align_layer(&layers, &mut layer_x, l, &preds, g, gap);
        }
        for l in (0..max_layer).rev() {
            block_shift_layer(&layers, &mut layer_x, l, &succs, g);
        }
        normalize_x(&mut layer_x);
    }

    // Single-node layer symmetry: align solitary nodes on the midpoint of
    // their neighbors (so fan-in/fan-out siblings center under/over them).
    for l in 0..=max_layer {
        if layers[l].len() != 1 {
            continue;
        }
        let id = layers[l][0];
        let mut centers: Vec<f64> = Vec::new();
        for &ni in preds[id].iter().chain(succs[id].iter()) {
            let np = g.nodes[ni].order;
            let nl = g.nodes[ni].layer;
            centers.push(layer_x[nl][np] as f64 + g.nodes[ni].width as f64 / 2.0);
        }
        if centers.is_empty() {
            continue;
        }
        let avg = centers.iter().sum::<f64>() / centers.len() as f64;
        let half = g.nodes[id].width as f64 / 2.0;
        let target = if avg <= half { 0 } else { (avg - half).round() as usize };
        layer_x[l][0] = target;
    }

    // Chain alignment: when a run of consecutive single-node layers forms a
    // linear chain (each node has ≤ 1 predecessor and ≤ 1 successor), line
    // up their integer centers on a common column. Fixes parity mismatches
    // in mixed-width chains where no single float-center works for all boxes.
    let is_linear = |id: NodeId| preds[id].len() <= 1 && succs[id].len() <= 1;
    {
        let mut l = 0;
        while l <= max_layer {
            if layers[l].len() != 1 || !is_linear(layers[l][0]) {
                l += 1;
                continue;
            }
            let mut end = l;
            while end < max_layer
                && layers[end + 1].len() == 1
                && is_linear(layers[end + 1][0])
            {
                end += 1;
            }
            if end > l {
                let c = (l..=end)
                    .map(|k| g.nodes[layers[k][0]].width / 2)
                    .max()
                    .unwrap_or(0);
                for k in l..=end {
                    let w2 = g.nodes[layers[k][0]].width / 2;
                    layer_x[k][0] = c - w2;
                }
            }
            l = end + 1;
        }
    }
    normalize_x(&mut layer_x);

    for (l, layer) in layers.iter().enumerate() {
        for (i, &id) in layer.iter().enumerate() {
            g.nodes[id].x = layer_x[l][i];
        }
    }
}

// Compute the barycenter of `id`'s neighbors' center-x positions (left edge).
fn neighbor_target_x(
    id: NodeId,
    neighbors: &[NodeId],
    layer_x: &[Vec<usize>],
    g: &Graph,
) -> Option<usize> {
    if neighbors.is_empty() {
        return None;
    }
    let center: f64 = neighbors
        .iter()
        .map(|&ni| {
            let nl = g.nodes[ni].layer;
            let np = g.nodes[ni].order;
            layer_x[nl][np] as f64 + g.nodes[ni].width as f64 / 2.0
        })
        .sum::<f64>()
        / neighbors.len() as f64;
    let half = g.nodes[id].width as f64 / 2.0;
    Some(if center < half { 0 } else { (center - half).round() as usize })
}

fn block_shift_layer(
    layers: &[Vec<NodeId>],
    layer_x: &mut [Vec<usize>],
    l: usize,
    neighbors: &[Vec<NodeId>],
    g: &Graph,
) {
    if layers[l].is_empty() {
        return;
    }
    let mut sum_delta: i64 = 0;
    let mut count: i64 = 0;
    for (i, &id) in layers[l].iter().enumerate() {
        let Some(target) = neighbor_target_x(id, &neighbors[id], layer_x, g) else {
            continue;
        };
        sum_delta += target as i64 - layer_x[l][i] as i64;
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
        let leftmost = *layer_x[l].iter().min().unwrap();
        let actual = s.min(leftmost);
        for x in layer_x[l].iter_mut() {
            *x -= actual;
        }
    }
}

fn normalize_x(layer_x: &mut [Vec<usize>]) {
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

fn align_layer(
    layers: &[Vec<NodeId>],
    layer_x: &mut [Vec<usize>],
    l: usize,
    neighbors: &[Vec<NodeId>],
    g: &Graph,
    gap: usize,
) {
    let n = layers[l].len();
    if n == 0 {
        return;
    }
    let targets: Vec<Option<usize>> = layers[l]
        .iter()
        .map(|&id| neighbor_target_x(id, &neighbors[id], layer_x, g))
        .collect();

    let mut new_x: Vec<usize> = vec![0; n];
    for i in 0..n {
        let target = targets[i].unwrap_or(layer_x[l][i]);
        let lb = if i == 0 {
            0
        } else {
            let prev_id = layers[l][i - 1];
            new_x[i - 1] + g.nodes[prev_id].width + gap
        };
        new_x[i] = target.max(lb);
    }
    for i in (0..n).rev() {
        let target = targets[i].unwrap_or(new_x[i]);
        let id = layers[l][i];
        let ub = if i == n - 1 {
            usize::MAX
        } else {
            new_x[i + 1].saturating_sub(g.nodes[id].width + gap)
        };
        let lb = if i == 0 {
            0
        } else {
            let prev_id = layers[l][i - 1];
            new_x[i - 1] + g.nodes[prev_id].width + gap
        };
        new_x[i] = target.min(ub).max(lb);
    }
    layer_x[l] = new_x;
}

/// A channel between layer `l` and `l+1` is tight if it contains only
/// straight 1:1 edges — every source in `l` has exactly one successor and
/// every target in `l+1` has exactly one predecessor across this channel,
/// and every pair's minor-axis inner ranges overlap so `preferred_endpoints`
/// will emit a straight edge.
fn channel_is_tight(g: &Graph, l: usize) -> bool {
    use std::collections::HashMap;
    let mut src_out: HashMap<NodeId, usize> = HashMap::new();
    let mut dst_in: HashMap<NodeId, usize> = HashMap::new();
    let mut edges: Vec<(NodeId, NodeId)> = Vec::new();
    for e in &g.edges {
        if g.nodes[e.from].layer == l && g.nodes[e.to].layer == l + 1 {
            *src_out.entry(e.from).or_insert(0) += 1;
            *dst_in.entry(e.to).or_insert(0) += 1;
            edges.push((e.from, e.to));
        }
    }
    if edges.is_empty() {
        return true;
    }
    if src_out.values().any(|&c| c != 1) || dst_in.values().any(|&c| c != 1) {
        return false;
    }
    for (src_id, dst_id) in edges {
        let src = &g.nodes[src_id];
        let dst = &g.nodes[dst_id];
        let (slo, shi) = node_inner_range(src);
        let (dlo, dhi) = node_inner_range(dst);
        if slo.max(dlo) > shi.min(dhi) {
            return false;
        }
    }
    true
}

fn node_inner_range(n: &Node) -> (usize, usize) {
    if n.is_dummy {
        (n.x, n.x)
    } else if n.width >= 3 {
        (n.x + 1, n.x + n.width - 2)
    } else {
        (n.x, n.x + n.width.saturating_sub(1))
    }
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

    // Channel size. Tight (2) when every edge crossing the channel is 1:1
    // and straight — just `│ ▼`. Otherwise 4 rows so L-turn corners get
    // breathing room between the bend and the arrow.
    const LTURN_CHANNEL: usize = 4;
    let mut channel_heights: Vec<usize> = Vec::with_capacity(max_layer);
    for l in 0..max_layer {
        channel_heights.push(if channel_is_tight(g, l) {
            2
        } else {
            LTURN_CHANNEL
        });
    }
    let horizontal = !g.dir.is_vertical();
    for e in &g.edges {
        let Some(text) = e.label.as_ref() else { continue };
        let l = g.nodes[e.from].layer;
        if l >= max_layer {
            continue;
        }
        if horizontal {
            let needed = text.chars().count() + 2 * LR_LABEL_PAD + 1;
            if needed > channel_heights[l] {
                channel_heights[l] = needed;
            }
        } else {
            // Labels need their own row plus at least one drop cell above +
            // below to look clean. Minimum 4 rows for labeled TD channels.
            channel_heights[l] = channel_heights[l].max(4);
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
                    n.y = y;
                }
            }
        }
        y += lh;
        if l < max_layer {
            y += channel_heights[l];
        }
    }
}
