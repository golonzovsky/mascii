use crate::graph::{
    ArrowTip, Direction, Edge, EdgeStyle, Graph, Node, NodeId, Shape, SubgraphId,
};
use crate::style::Style;
use std::collections::HashMap;

// LR-only: horizontal `─` padding on each side of an inline edge label.
pub const LR_LABEL_PAD: usize = 2;

// Inner padding of subgraph containers. Must match render::draw_subgraph_containers
// so the reserved space we add here lines up with what the renderer actually
// draws as the box border + breathing room.
const CONTAINER_PAD_X: usize = 2;
const CONTAINER_PAD_TOP: usize = 2;
const CONTAINER_PAD_BOTTOM: usize = 1;

// Within-layer packing gap. Horizontal in TD/BT (4 cols feels comfortable),
// vertical in LR/RL (3 rows lines up with standard 3-row node heights so
// single-node layers sit exactly centered).
fn minor_gap(dir: Direction) -> usize {
    if dir.is_vertical() { 4 } else { 3 }
}

pub fn layout(mut g: Graph, padding: usize) -> Graph {
    compute_node_dims(&mut g, padding);

    // LR/RL: run the whole pipeline in TD coordinates internally. Every
    // sub-layout uses these swapped dimensions, so cached sub-sizes compose
    // consistently all the way up. The final pass flips x/y + w/h back.
    let horizontal = !g.dir.is_vertical();
    if horizontal {
        for n in &mut g.nodes {
            std::mem::swap(&mut n.width, &mut n.height);
        }
    }

    // Post-order: lay out every subgraph's contents innermost-first so each
    // parent level can size its meta-nodes from a known bounding box.
    let order = subgraph_post_order(&g);
    let mut sub_info: HashMap<SubgraphId, LevelResult> = HashMap::new();
    for sid in order {
        let result = layout_level(&mut g, Some(sid), &sub_info);
        sub_info.insert(sid, result);
    }
    let root = layout_level(&mut g, None, &sub_info);

    // Top-down: recursively apply offsets so every real node and dummy ends
    // up with its absolute (x, y) written back to g.nodes.
    apply_positions(&mut g, &root, 0, 0, &sub_info);

    if horizontal {
        for n in &mut g.nodes {
            std::mem::swap(&mut n.x, &mut n.y);
            std::mem::swap(&mut n.width, &mut n.height);
        }
    }
    g
}

/// Output of laying out a single context level.
#[derive(Debug, Default)]
struct LevelResult {
    /// Bounding box at this level, in this level's local coordinate system.
    /// (Before container padding — the parent adds padding when it builds
    /// its meta-node.)
    w: usize,
    h: usize,
    /// Real nodes and dummies placed at this level, with local positions.
    /// Absolute positions come from walking back down with per-meta offsets.
    real_pos: Vec<(NodeId, usize, usize)>,
    /// Meta-nodes placed at this level. Used to recurse into child subgraphs
    /// and assign their contents absolute positions.
    meta_pos: Vec<(SubgraphId, usize, usize)>,
}

/// Item in a Scope — one row of layout state, indexed by slice-local id.
#[derive(Debug, Clone, Copy)]
enum Item {
    /// Backed by `g.nodes[node]`.
    Real { node: NodeId },
    /// Represents a child subgraph as a single box (sized from its inner
    /// layout plus container padding).
    Meta { sub: SubgraphId },
    /// Pass-through inserted during `insert_dummies_scope` for long edges.
    /// Gets materialized into `g.nodes` as a real dummy node after layout.
    Dummy,
}

#[derive(Debug)]
struct SliceEdge {
    /// Slice-local indices (into `Scope::items`).
    from: usize,
    to: usize,
    label: Option<String>,
    style: EdgeStyle,
    tip_fwd: ArrowTip,
    tip_back: bool,
    length: usize,
    /// Index into `g.edges` if this slice edge maps 1:1 to an original edge.
    /// Dummy segments inserted by `insert_dummies_scope` have None.
    orig_g_edge: Option<usize>,
}

#[derive(Debug)]
struct Scope {
    items: Vec<Item>,
    edges: Vec<SliceEdge>,
    widths: Vec<usize>,
    heights: Vec<usize>,
    layers: Vec<usize>,
    orders: Vec<usize>,
    xs: Vec<usize>,
    ys: Vec<usize>,
}

impl Scope {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            edges: Vec::new(),
            widths: Vec::new(),
            heights: Vec::new(),
            layers: Vec::new(),
            orders: Vec::new(),
            xs: Vec::new(),
            ys: Vec::new(),
        }
    }

    fn push(&mut self, item: Item, w: usize, h: usize) -> usize {
        let id = self.items.len();
        self.items.push(item);
        self.widths.push(w);
        self.heights.push(h);
        self.layers.push(0);
        self.orders.push(0);
        self.xs.push(0);
        self.ys.push(0);
        id
    }

    fn len(&self) -> usize {
        self.items.len()
    }

    fn is_dummy(&self, i: usize) -> bool {
        matches!(self.items[i], Item::Dummy)
    }
}

fn layout_level(
    g: &mut Graph,
    context: Option<SubgraphId>,
    sub_info: &HashMap<SubgraphId, LevelResult>,
) -> LevelResult {
    let mut scope = Scope::new();
    let mut item_of_node: HashMap<NodeId, usize> = HashMap::new();
    let mut item_of_meta: HashMap<SubgraphId, usize> = HashMap::new();

    // Direct member real nodes — skip dummies (phantoms left over from
    // parse-time subgraph-edge resolution, or dummies already materialized
    // by a prior level).
    let node_count = g.nodes.len();
    for nid in 0..node_count {
        let n = &g.nodes[nid];
        if n.is_dummy {
            continue;
        }
        if n.subgraph == context {
            let id = scope.push(Item::Real { node: n.id }, n.width, n.height);
            item_of_node.insert(n.id, id);
        }
    }

    // Meta-nodes for every direct child subgraph of `context`. The title
    // constraint applies to whichever internal dimension will become the
    // meta's horizontal extent in the final output — that's `w` for TD
    // and `h` for LR (since LR swaps x/y at the end).
    let horizontal_output = !g.dir.is_vertical();
    for sid in 0..g.subgraphs.len() {
        if g.subgraphs[sid].parent != context {
            continue;
        }
        let Some(child) = sub_info.get(&sid) else {
            continue;
        };
        let title = if !g.subgraphs[sid].label.is_empty() {
            &g.subgraphs[sid].label
        } else {
            &g.subgraphs[sid].name
        };
        // Minimum extent to fit "─ title ─" on the top border.
        let min_title = title.chars().count() + 4;
        let base_w = child.w + 2 * CONTAINER_PAD_X;
        let base_h = (child.h + CONTAINER_PAD_TOP + CONTAINER_PAD_BOTTOM).max(3);
        let (meta_w, meta_h) = if horizontal_output {
            (base_w, base_h.max(min_title))
        } else {
            (base_w.max(min_title), base_h)
        };
        let id = scope.push(Item::Meta { sub: sid }, meta_w, meta_h);
        item_of_meta.insert(sid, id);
    }

    // Edges whose lowest common ancestor matches this context.
    let edge_count = g.edges.len();
    for e_idx in 0..edge_count {
        let e = &g.edges[e_idx];
        if g.nodes[e.from].is_dummy || g.nodes[e.to].is_dummy {
            continue;
        }
        let lca = lowest_common_ancestor(g, e.from, e.to);
        if lca != context {
            continue;
        }
        let from_item =
            map_endpoint_to_slice(g, e.from, context, &item_of_node, &item_of_meta);
        let to_item =
            map_endpoint_to_slice(g, e.to, context, &item_of_node, &item_of_meta);
        scope.edges.push(SliceEdge {
            from: from_item,
            to: to_item,
            label: e.label.clone(),
            style: e.style,
            tip_fwd: e.tip_fwd,
            tip_back: e.tip_back,
            length: e.length,
            orig_g_edge: Some(e_idx),
        });
    }

    if scope.items.is_empty() {
        return LevelResult::default();
    }

    let dir = g.dir;
    assign_layers_scope(&mut scope);
    let chain_rewrites = insert_dummies_scope(&mut scope);
    order_layers_scope(&mut scope, 8);
    assign_x_scope(&mut scope, minor_gap(dir));
    assign_y_scope(&mut scope, dir);

    // Turn slice dummies into real dummy nodes in g and rewrite any long
    // g.edges entries into chains through the new dummies.
    let dummy_node_ids = materialize_dummies(&scope, g, context);
    rewrite_g_edges(g, &chain_rewrites, &dummy_node_ids);

    // Collect positions for the top-down offset walk.
    let mut real_pos = Vec::new();
    let mut meta_pos = Vec::new();
    for i in 0..scope.len() {
        let x = scope.xs[i];
        let y = scope.ys[i];
        match scope.items[i] {
            Item::Real { node } => real_pos.push((node, x, y)),
            Item::Meta { sub } => meta_pos.push((sub, x, y)),
            Item::Dummy => {
                let nid = dummy_node_ids[&i];
                // Dummies behave like real nodes for the apply step: their
                // absolute position gets written directly to g.nodes.
                real_pos.push((nid, x, y));
            }
        }
    }

    let w = (0..scope.len())
        .map(|i| scope.xs[i] + scope.widths[i])
        .max()
        .unwrap_or(0);
    let h = (0..scope.len())
        .map(|i| scope.ys[i] + scope.heights[i])
        .max()
        .unwrap_or(0);

    LevelResult {
        w,
        h,
        real_pos,
        meta_pos,
    }
}

// ── Sugiyama pipeline (scope-local versions) ───────────────────────────────

fn assign_layers_scope(scope: &mut Scope) {
    let n = scope.len();
    let mut indeg = vec![0usize; n];
    let mut adj: Vec<Vec<(usize, usize)>> = vec![vec![]; n];
    for e in &scope.edges {
        indeg[e.to] += 1;
        adj[e.from].push((e.to, e.length));
    }
    let mut layer = vec![0usize; n];
    let mut remaining = indeg.clone();
    let mut queue: Vec<usize> = (0..n).filter(|&i| remaining[i] == 0).collect();
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
    scope.layers = layer;
}

/// For each slice edge spanning more than one layer, insert a chain of Dummy
/// slice items and replace the edge with per-hop segments. Returns a list of
/// (original g.edges index, Vec of slice dummy ids) for the follow-up pass
/// that rewrites `g.edges` to go through real dummy nodes.
fn insert_dummies_scope(scope: &mut Scope) -> Vec<(usize, Vec<usize>)> {
    let mut chains: Vec<(usize, Vec<usize>)> = Vec::new();
    let old_edges: Vec<SliceEdge> = std::mem::take(&mut scope.edges);

    for e in old_edges {
        let from_layer = scope.layers[e.from];
        let to_layer = scope.layers[e.to];
        if to_layer <= from_layer + 1 {
            scope.edges.push(e);
            continue;
        }

        let mut dummy_ids: Vec<usize> = Vec::new();
        let mut prev = e.from;
        let mut label = e.label.clone();
        for l in (from_layer + 1)..to_layer {
            let slice_id = scope.push(Item::Dummy, 1, 1);
            scope.layers[slice_id] = l;

            scope.edges.push(SliceEdge {
                from: prev,
                to: slice_id,
                label: label.take(),
                style: e.style,
                tip_fwd: ArrowTip::None,
                tip_back: false,
                length: 1,
                orig_g_edge: None,
            });

            dummy_ids.push(slice_id);
            prev = slice_id;
        }

        scope.edges.push(SliceEdge {
            from: prev,
            to: e.to,
            label: None,
            style: e.style,
            tip_fwd: e.tip_fwd,
            tip_back: e.tip_back,
            length: 1,
            orig_g_edge: None,
        });

        if let Some(orig) = e.orig_g_edge {
            chains.push((orig, dummy_ids));
        }
    }

    chains
}

fn order_layers_scope(scope: &mut Scope, iterations: usize) {
    let max_layer = *scope.layers.iter().max().unwrap_or(&0);
    let n = scope.len();
    let mut layers: Vec<Vec<usize>> = vec![vec![]; max_layer + 1];
    for i in 0..n {
        layers[scope.layers[i]].push(i);
    }
    for layer in &layers {
        for (j, &id) in layer.iter().enumerate() {
            scope.orders[id] = j;
        }
    }

    let mut preds: Vec<Vec<usize>> = vec![vec![]; n];
    let mut succs: Vec<Vec<usize>> = vec![vec![]; n];
    for e in &scope.edges {
        succs[e.from].push(e.to);
        preds[e.to].push(e.from);
    }

    for _ in 0..iterations {
        #[allow(clippy::needless_range_loop)]
        for l in 1..=max_layer {
            sort_layer_by_neighbors_scope(&mut layers[l], &preds, scope);
            for (j, &id) in layers[l].iter().enumerate() {
                scope.orders[id] = j;
            }
        }
        for l in (0..max_layer).rev() {
            sort_layer_by_neighbors_scope(&mut layers[l], &succs, scope);
            for (j, &id) in layers[l].iter().enumerate() {
                scope.orders[id] = j;
            }
        }
    }
}

fn sort_layer_by_neighbors_scope(
    layer: &mut [usize],
    neighbors: &[Vec<usize>],
    scope: &Scope,
) {
    layer.sort_by(|&a, &b| {
        let ba = barycenter_scope(a, neighbors, scope);
        let bb = barycenter_scope(b, neighbors, scope);
        ba.partial_cmp(&bb).unwrap_or(std::cmp::Ordering::Equal)
    });
}

fn barycenter_scope(id: usize, neighbors: &[Vec<usize>], scope: &Scope) -> f64 {
    let ns = &neighbors[id];
    if ns.is_empty() {
        scope.orders[id] as f64
    } else {
        ns.iter().map(|&ni| scope.orders[ni] as f64).sum::<f64>() / ns.len() as f64
    }
}

fn assign_x_scope(scope: &mut Scope, gap: usize) {
    let max_layer = *scope.layers.iter().max().unwrap_or(&0);
    let mut layers: Vec<Vec<usize>> = vec![vec![]; max_layer + 1];
    for i in 0..scope.len() {
        layers[scope.layers[i]].push(i);
    }
    for layer in &mut layers {
        layer.sort_by_key(|&id| scope.orders[id]);
    }

    let mut layer_x: Vec<Vec<usize>> = vec![vec![]; max_layer + 1];
    for (l, layer) in layers.iter().enumerate() {
        let mut x = 0usize;
        for &id in layer {
            layer_x[l].push(x);
            x += scope.widths[id] + gap;
        }
    }

    let n = scope.len();
    let mut preds: Vec<Vec<usize>> = vec![vec![]; n];
    let mut succs: Vec<Vec<usize>> = vec![vec![]; n];
    for e in &scope.edges {
        succs[e.from].push(e.to);
        preds[e.to].push(e.from);
    }

    // Down passes align per-node to preds; up passes block-shift whole layer
    // (per-node up-alignment causes positive-feedback drift).
    for _ in 0..4 {
        for l in 1..=max_layer {
            align_layer_scope(&layers, &mut layer_x, l, &preds, scope, gap);
        }
        for l in (0..max_layer).rev() {
            block_shift_layer_scope(&layers, &mut layer_x, l, &succs, scope);
        }
        normalize_x(&mut layer_x);
    }

    // Solitary-layer symmetry: center a lone node over/under its neighbors.
    for l in 0..=max_layer {
        if layers[l].len() != 1 {
            continue;
        }
        let id = layers[l][0];
        let mut centers: Vec<f64> = Vec::new();
        for &ni in preds[id].iter().chain(succs[id].iter()) {
            let np = scope.orders[ni];
            let nl = scope.layers[ni];
            centers.push(layer_x[nl][np] as f64 + scope.widths[ni] as f64 / 2.0);
        }
        if centers.is_empty() {
            continue;
        }
        let avg = centers.iter().sum::<f64>() / centers.len() as f64;
        let half = scope.widths[id] as f64 / 2.0;
        let target = if avg <= half {
            0
        } else {
            (avg - half).round() as usize
        };
        layer_x[l][0] = target;
    }

    // Chain alignment: a chain is a maximal run of single-node layers where
    // every internal link is 1:1. The first node may have any number of
    // predecessors; the last may have any number of successors (it's the
    // terminator of a fan-out). This lets the chain include its fan-out
    // end so the whole path — including the node that spawns the fan-out —
    // shares a common integer center.
    //
    // The shared center is the max of each chain node's current center
    // (after down/up passes). That preserves whatever barycenter the
    // optimization step produced instead of collapsing to max(width)/2,
    // which would override correct positions for chains that need to sit
    // above or below neighbors.
    let starts_chain = |id: usize| succs[id].len() == 1;
    let continues_chain = |id: usize| preds[id].len() == 1 && succs[id].len() == 1;
    let ends_chain = |id: usize| preds[id].len() == 1;
    {
        let mut l = 0;
        while l <= max_layer {
            if layers[l].len() != 1 || !starts_chain(layers[l][0]) {
                l += 1;
                continue;
            }
            let mut end = l;
            while end < max_layer && layers[end + 1].len() == 1 {
                let next_id = layers[end + 1][0];
                if continues_chain(next_id) {
                    end += 1;
                    continue;
                }
                if ends_chain(next_id) {
                    end += 1;
                }
                break;
            }
            if end > l {
                let shared_center = (l..=end)
                    .map(|k| layer_x[k][0] + scope.widths[layers[k][0]] / 2)
                    .max()
                    .unwrap_or(0);
                for k in l..=end {
                    let w2 = scope.widths[layers[k][0]] / 2;
                    layer_x[k][0] = shared_center.saturating_sub(w2);
                }
            }
            l = end + 1;
        }
    }
    normalize_x(&mut layer_x);

    for (l, layer) in layers.iter().enumerate() {
        for (j, &id) in layer.iter().enumerate() {
            scope.xs[id] = layer_x[l][j];
        }
    }
}

fn neighbor_target_x_scope(
    id: usize,
    neighbors: &[usize],
    layer_x: &[Vec<usize>],
    scope: &Scope,
) -> Option<usize> {
    if neighbors.is_empty() {
        return None;
    }
    let center: f64 = neighbors
        .iter()
        .map(|&ni| {
            let nl = scope.layers[ni];
            let np = scope.orders[ni];
            layer_x[nl][np] as f64 + scope.widths[ni] as f64 / 2.0
        })
        .sum::<f64>()
        / neighbors.len() as f64;
    let half = scope.widths[id] as f64 / 2.0;
    Some(if center < half {
        0
    } else {
        (center - half).round() as usize
    })
}

fn block_shift_layer_scope(
    layers: &[Vec<usize>],
    layer_x: &mut [Vec<usize>],
    l: usize,
    neighbors: &[Vec<usize>],
    scope: &Scope,
) {
    if layers[l].is_empty() {
        return;
    }
    let mut sum_delta: i64 = 0;
    let mut count: i64 = 0;
    for (j, &id) in layers[l].iter().enumerate() {
        let Some(target) = neighbor_target_x_scope(id, &neighbors[id], layer_x, scope)
        else {
            continue;
        };
        sum_delta += target as i64 - layer_x[l][j] as i64;
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

fn align_layer_scope(
    layers: &[Vec<usize>],
    layer_x: &mut [Vec<usize>],
    l: usize,
    neighbors: &[Vec<usize>],
    scope: &Scope,
    gap: usize,
) {
    let n = layers[l].len();
    if n == 0 {
        return;
    }
    let targets: Vec<Option<usize>> = layers[l]
        .iter()
        .map(|&id| neighbor_target_x_scope(id, &neighbors[id], layer_x, scope))
        .collect();

    let mut new_x: Vec<usize> = vec![0; n];
    for i in 0..n {
        let target = targets[i].unwrap_or(layer_x[l][i]);
        let lb = if i == 0 {
            0
        } else {
            let prev_id = layers[l][i - 1];
            new_x[i - 1] + scope.widths[prev_id] + gap
        };
        new_x[i] = target.max(lb);
    }
    for i in (0..n).rev() {
        let target = targets[i].unwrap_or(new_x[i]);
        let id = layers[l][i];
        let ub = if i == n - 1 {
            usize::MAX
        } else {
            new_x[i + 1].saturating_sub(scope.widths[id] + gap)
        };
        let lb = if i == 0 {
            0
        } else {
            let prev_id = layers[l][i - 1];
            new_x[i - 1] + scope.widths[prev_id] + gap
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
///
/// Edges touching a meta-node are never considered tight: the meta-node's
/// inner range spans its entire bounding box, but the actual rendered edge
/// terminates at the child node it was rerouted to (often much narrower),
/// so the real edge needs L-turn breathing room even though the slice-level
/// check would otherwise say "straight".
fn channel_is_tight_scope(scope: &Scope, l: usize) -> bool {
    let mut src_out: HashMap<usize, usize> = HashMap::new();
    let mut dst_in: HashMap<usize, usize> = HashMap::new();
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for e in &scope.edges {
        if scope.layers[e.from] == l && scope.layers[e.to] == l + 1 {
            if matches!(scope.items[e.from], Item::Meta { .. })
                || matches!(scope.items[e.to], Item::Meta { .. })
            {
                return false;
            }
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
    for (src, dst) in edges {
        let (slo, shi) = item_inner_range_scope(scope, src);
        let (dlo, dhi) = item_inner_range_scope(scope, dst);
        if slo.max(dlo) > shi.min(dhi) {
            return false;
        }
    }
    true
}

fn item_inner_range_scope(scope: &Scope, id: usize) -> (usize, usize) {
    let x = scope.xs[id];
    let w = scope.widths[id];
    if scope.is_dummy(id) {
        (x, x)
    } else if w >= 3 {
        (x + 1, x + w - 2)
    } else {
        (x, x + w.saturating_sub(1))
    }
}

fn assign_y_scope(scope: &mut Scope, dir: Direction) {
    let max_layer = *scope.layers.iter().max().unwrap_or(&0);
    let mut layer_heights: Vec<usize> = vec![0; max_layer + 1];
    for i in 0..scope.len() {
        let h = if scope.is_dummy(i) { 1 } else { scope.heights[i] };
        if h > layer_heights[scope.layers[i]] {
            layer_heights[scope.layers[i]] = h;
        }
    }

    // Channel size. Tight (2) when every edge crossing the channel is 1:1
    // and straight — just `│ ▼`. Otherwise 4 rows so L-turn corners get
    // breathing room between the bend and the arrow.
    const LTURN_CHANNEL: usize = 4;
    let mut channel_heights: Vec<usize> = Vec::with_capacity(max_layer);
    for l in 0..max_layer {
        channel_heights.push(if channel_is_tight_scope(scope, l) {
            2
        } else {
            LTURN_CHANNEL
        });
    }
    let horizontal = !dir.is_vertical();
    for e in &scope.edges {
        let Some(text) = e.label.as_ref() else {
            continue;
        };
        let l = scope.layers[e.from];
        if l >= max_layer {
            continue;
        }
        if horizontal {
            let needed = text.chars().count() + 2 * LR_LABEL_PAD + 1;
            if needed > channel_heights[l] {
                channel_heights[l] = needed;
            }
        } else {
            // Labels need their own row plus drop cells above + below to
            // look clean; force at least 4 rows for labeled TD channels.
            channel_heights[l] = channel_heights[l].max(4);
        }
    }

    let mut y = 0usize;
    for l in 0..=max_layer {
        let lh = layer_heights[l];
        for i in 0..scope.len() {
            if scope.layers[i] == l {
                scope.ys[i] = y;
                if scope.is_dummy(i) {
                    scope.heights[i] = lh;
                }
            }
        }
        y += lh;
        if l < max_layer {
            y += channel_heights[l];
        }
    }
}

// ── Dummy materialization ──────────────────────────────────────────────────

fn materialize_dummies(
    scope: &Scope,
    g: &mut Graph,
    context: Option<SubgraphId>,
) -> HashMap<usize, NodeId> {
    let mut map: HashMap<usize, NodeId> = HashMap::new();
    for i in 0..scope.len() {
        if let Item::Dummy = scope.items[i] {
            let nid = g.nodes.len();
            g.nodes.push(Node {
                id: nid,
                name: format!("__dummy_{}", nid),
                label_lines: vec![],
                is_dummy: true,
                shape: Shape::Round,
                width: 1,
                height: 1,
                x: 0,
                y: 0,
                style: Style::new(),
                subgraph: context,
            });
            map.insert(i, nid);
        }
    }
    map
}

/// For every long `g.edges` entry that got broken up by `insert_dummies_scope`,
/// rewrite the original edge in place as the first segment of the chain and
/// append the remaining segments. The original index is preserved so render
/// iteration over g.edges sees a consistent chain.
fn rewrite_g_edges(
    g: &mut Graph,
    chains: &[(usize, Vec<usize>)],
    dummy_node_ids: &HashMap<usize, NodeId>,
) {
    for (e_idx, slice_ids) in chains {
        if slice_ids.is_empty() {
            continue;
        }
        let g_ids: Vec<NodeId> = slice_ids.iter().map(|s| dummy_node_ids[s]).collect();
        let orig = g.edges[*e_idx].clone();

        g.edges[*e_idx] = Edge {
            from: orig.from,
            to: g_ids[0],
            label: orig.label.clone(),
            style: orig.style,
            tip_fwd: ArrowTip::None,
            tip_back: false,
            length: 1,
        };

        for w in g_ids.windows(2) {
            g.edges.push(Edge {
                from: w[0],
                to: w[1],
                label: None,
                style: orig.style,
                tip_fwd: ArrowTip::None,
                tip_back: false,
                length: 1,
            });
        }

        g.edges.push(Edge {
            from: *g_ids.last().unwrap(),
            to: orig.to,
            label: None,
            style: orig.style,
            tip_fwd: orig.tip_fwd,
            tip_back: orig.tip_back,
            length: 1,
        });
    }
}

// ── Node dimension / subgraph walks ─────────────────────────────────────────

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

fn subgraph_post_order(g: &Graph) -> Vec<SubgraphId> {
    let mut order = Vec::new();
    let mut visited = vec![false; g.subgraphs.len()];
    for root in 0..g.subgraphs.len() {
        if g.subgraphs[root].parent.is_none() {
            visit_post(g, root, &mut visited, &mut order);
        }
    }
    // Defensive: any disconnected subgraphs.
    for sid in 0..g.subgraphs.len() {
        if !visited[sid] {
            visit_post(g, sid, &mut visited, &mut order);
        }
    }
    order
}

fn visit_post(g: &Graph, sid: SubgraphId, visited: &mut [bool], out: &mut Vec<SubgraphId>) {
    if visited[sid] {
        return;
    }
    visited[sid] = true;
    for cid in 0..g.subgraphs.len() {
        if g.subgraphs[cid].parent == Some(sid) {
            visit_post(g, cid, visited, out);
        }
    }
    out.push(sid);
}

fn lowest_common_ancestor(g: &Graph, a: NodeId, b: NodeId) -> Option<SubgraphId> {
    // a's ancestor chain (leaf→root, including None at the end).
    let mut chain: Vec<Option<SubgraphId>> = Vec::new();
    let mut cur = g.nodes[a].subgraph;
    chain.push(cur);
    while let Some(s) = cur {
        cur = g.subgraphs[s].parent;
        chain.push(cur);
    }
    // Walk b's chain until hitting something in `chain`.
    let mut cur = g.nodes[b].subgraph;
    loop {
        if chain.contains(&cur) {
            return cur;
        }
        match cur {
            None => return None, // chain always contains None so this is unreachable
            Some(s) => cur = g.subgraphs[s].parent,
        }
    }
}

fn map_endpoint_to_slice(
    g: &Graph,
    node: NodeId,
    context: Option<SubgraphId>,
    item_of_node: &HashMap<NodeId, usize>,
    item_of_meta: &HashMap<SubgraphId, usize>,
) -> usize {
    if g.nodes[node].subgraph == context {
        return item_of_node[&node];
    }
    // Walk up the node's subgraph chain until the one whose parent is the
    // context — that subgraph is the child whose meta-node wraps this node
    // at the current level.
    let mut cur = g.nodes[node]
        .subgraph
        .expect("node outside context must live in some subgraph");
    loop {
        let parent = g.subgraphs[cur].parent;
        if parent == context {
            return item_of_meta[&cur];
        }
        cur = parent.expect("walked past root without finding context");
    }
}

fn apply_positions(
    g: &mut Graph,
    level: &LevelResult,
    offset_x: usize,
    offset_y: usize,
    sub_info: &HashMap<SubgraphId, LevelResult>,
) {
    for &(nid, rx, ry) in &level.real_pos {
        g.nodes[nid].x = offset_x + rx;
        g.nodes[nid].y = offset_y + ry;
    }
    for &(sid, rx, ry) in &level.meta_pos {
        let Some(child) = sub_info.get(&sid) else {
            continue;
        };
        // The child's contents live inside the meta-node's container padding.
        let child_origin_x = offset_x + rx + CONTAINER_PAD_X;
        let child_origin_y = offset_y + ry + CONTAINER_PAD_TOP;
        apply_positions(g, child, child_origin_x, child_origin_y, sub_info);
    }
}
