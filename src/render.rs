use crate::graph::{Graph, NodeId, Shape};
use std::collections::HashMap;

const RESET: &str = "\x1b[0m";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CellKind {
    #[default]
    Empty,
    Border,
    Label,
    Edge,
    Arrow,
    Crossing,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Theme {
    pub border: Option<&'static str>,
    pub label: Option<&'static str>,
    pub edge: Option<&'static str>,
    pub arrow: Option<&'static str>,
    pub crossing: Option<&'static str>,
}

impl Theme {
    pub const fn plain() -> Self {
        Self {
            border: None,
            label: None,
            edge: None,
            arrow: None,
            crossing: None,
        }
    }

    pub const fn grey() -> Self {
        const G: &str = "\x1b[90m";
        Self {
            border: Some(G),
            label: None,
            edge: Some(G),
            arrow: Some(G),
            crossing: Some(G),
        }
    }

    pub const fn mono() -> Self {
        Self {
            border: Some("\x1b[90m"),
            label: None,
            edge: Some("\x1b[90m"),
            arrow: Some("\x1b[97m"),
            crossing: Some("\x1b[93m"),
        }
    }

    pub const fn neon() -> Self {
        // Truecolor (24-bit) palette.
        // Violet borders (#BC13FE), muted green lines/arrows (#4A8C5C),
        // bright white labels, hot pink crossings (#FF1493).
        Self {
            border: Some("\x1b[38;2;188;19;254m"),
            label: Some("\x1b[38;2;255;255;255m"),
            edge: Some("\x1b[38;2;110;190;130m"),
            arrow: Some("\x1b[38;2;110;190;130m"),
            crossing: Some("\x1b[38;2;255;20;147m"),
        }
    }

    pub const fn dim() -> Self {
        const D: &str = "\x1b[2m";
        Self {
            border: Some(D),
            label: Some(D),
            edge: Some(D),
            arrow: Some(D),
            crossing: Some(D),
        }
    }

    pub fn by_name(name: &str) -> Option<Self> {
        match name {
            "none" | "plain" => Some(Self::plain()),
            "grey" | "gray" => Some(Self::grey()),
            "mono" => Some(Self::mono()),
            "neon" => Some(Self::neon()),
            "dim" => Some(Self::dim()),
            _ => None,
        }
    }

    fn color_for(&self, kind: CellKind) -> Option<&'static str> {
        match kind {
            CellKind::Empty => None,
            CellKind::Border => self.border,
            CellKind::Label => self.label,
            CellKind::Edge => self.edge,
            CellKind::Arrow => self.arrow,
            CellKind::Crossing => self.crossing,
        }
    }
}

struct Canvas {
    chars: Vec<Vec<char>>,
    kinds: Vec<Vec<CellKind>>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        Self {
            chars: vec![vec![' '; w]; h],
            kinds: vec![vec![CellKind::Empty; w]; h],
        }
    }

    fn set(&mut self, x: usize, y: usize, ch: char, kind: CellKind) {
        if y < self.chars.len() && x < self.chars[y].len() {
            self.chars[y][x] = ch;
            self.kinds[y][x] = kind;
        }
    }

    fn get_char(&self, x: usize, y: usize) -> char {
        if y < self.chars.len() && x < self.chars[y].len() {
            self.chars[y][x]
        } else {
            ' '
        }
    }

    fn set_overlay(&mut self, x: usize, y: usize, ch: char, kind: CellKind) {
        let cur = self.get_char(x, y);
        if cur == ' ' {
            self.set(x, y, ch, kind);
        } else if cur == ch {
            // identical
        } else if (cur == '─' && ch == '│') || (cur == '│' && ch == '─') {
            self.set(x, y, '╳', CellKind::Crossing);
        }
        // else: leave existing character
    }
}

pub fn render(g: &Graph, theme: &Theme) -> String {
    let width: usize = g
        .nodes
        .iter()
        .map(|n| n.x + n.width)
        .max()
        .unwrap_or(0)
        + 1;
    let height: usize = g
        .nodes
        .iter()
        .map(|n| n.y + n.height.max(1))
        .max()
        .unwrap_or(0)
        + 1;

    let mut canvas = Canvas::new(width, height);

    // Boxes first
    for n in &g.nodes {
        if n.is_dummy {
            continue;
        }
        draw_box(&mut canvas, n.x, n.y, n.width, n.height, &n.label_lines, n.shape);
    }

    // Dummies (vertical pass-throughs)
    for n in &g.nodes {
        if n.is_dummy {
            for dy in 0..n.height {
                canvas.set(n.x, n.y + dy, '│', CellKind::Edge);
            }
        }
    }

    // Edge connection point assignment
    let endpoints = compute_endpoints(g);

    // Group edges by target so we can render merges as a single bar
    let mut by_target: HashMap<NodeId, Vec<usize>> = HashMap::new();
    for (i, e) in g.edges.iter().enumerate() {
        by_target.entry(e.to).or_default().push(i);
    }
    let mut targets: Vec<NodeId> = by_target.keys().copied().collect();
    targets.sort();
    for tid in targets {
        let edges = &by_target[&tid];
        let target = &g.nodes[tid];
        if edges.len() == 1 || target.is_dummy {
            for &i in edges {
                let (sx, sy, dx, dy) = endpoints[&i];
                draw_edge(&mut canvas, sx, sy, dx, dy, target.is_dummy);
            }
        } else {
            draw_merge(&mut canvas, g, tid, edges);
        }
    }

    // Edge labels — placed beside the exit column in the channel below source.
    for (i, e) in g.edges.iter().enumerate() {
        let Some(text) = e.label.as_deref() else { continue };
        if text.is_empty() {
            continue;
        }
        let (sx, sy, _dx, _dy) = endpoints[&i];
        place_edge_label(&mut canvas, sx, sy, text);
    }

    emit(&canvas, theme)
}

fn place_edge_label(canvas: &mut Canvas, sx: usize, sy: usize, text: &str) {
    // Inline placement: center label on the edge column, breaking the `│`
    // drop at that row. Only write over space or `│` cells, so we don't
    // clobber a neighbour's line art.
    let row = sy + 1;
    if row >= canvas.chars.len() {
        return;
    }
    let len = text.chars().count();
    if len == 0 {
        return;
    }
    let start = sx.saturating_sub(len / 2);
    let width = canvas.chars[row].len();
    if start + len > width {
        return;
    }
    for k in 0..len {
        let c = canvas.chars[row][start + k];
        if c != ' ' && c != '│' {
            return;
        }
    }
    for (k, ch) in text.chars().enumerate() {
        canvas.set(start + k, row, ch, CellKind::Label);
    }
}

fn emit(canvas: &Canvas, theme: &Theme) -> String {
    let mut out = String::new();
    for (row_chars, row_kinds) in canvas.chars.iter().zip(canvas.kinds.iter()) {
        // Find last non-empty char for trimming
        let last_nonempty = row_chars.iter().rposition(|&c| c != ' ');
        let end = match last_nonempty {
            Some(i) => i + 1,
            None => {
                out.push('\n');
                continue;
            }
        };
        let mut current: Option<&'static str> = None;
        for i in 0..end {
            let ch = row_chars[i];
            let kind = row_kinds[i];
            let want = theme.color_for(kind);
            if want != current {
                if current.is_some() {
                    out.push_str(RESET);
                }
                if let Some(code) = want {
                    out.push_str(code);
                }
                current = want;
            }
            out.push(ch);
        }
        if current.is_some() {
            out.push_str(RESET);
        }
        out.push('\n');
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

fn draw_box(
    canvas: &mut Canvas,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    lines: &[String],
    shape: Shape,
) {
    let (tl, tr, bl, br) = match shape {
        Shape::Round => ('╭', '╮', '╰', '╯'),
        Shape::Square => ('┌', '┐', '└', '┘'),
    };
    canvas.set(x, y, tl, CellKind::Border);
    canvas.set(x + w - 1, y, tr, CellKind::Border);
    canvas.set(x, y + h - 1, bl, CellKind::Border);
    canvas.set(x + w - 1, y + h - 1, br, CellKind::Border);
    for i in 1..w - 1 {
        canvas.set(x + i, y, '─', CellKind::Border);
        canvas.set(x + i, y + h - 1, '─', CellKind::Border);
    }
    for j in 1..h - 1 {
        canvas.set(x, y + j, '│', CellKind::Border);
        canvas.set(x + w - 1, y + j, '│', CellKind::Border);
    }
    for (li, line) in lines.iter().enumerate() {
        let row = y + 1 + li;
        let inner = w - 2;
        let len = line.chars().count();
        let pad_left = if inner > len { (inner - len) / 2 } else { 0 };
        for (ci, ch) in line.chars().enumerate() {
            canvas.set(x + 1 + pad_left + ci, row, ch, CellKind::Label);
        }
    }
}

fn inner_range(n: &crate::graph::Node) -> (usize, usize) {
    if n.is_dummy {
        (n.x, n.x)
    } else if n.width >= 3 {
        (n.x + 1, n.x + n.width - 2)
    } else {
        (n.x, n.x + n.width.saturating_sub(1))
    }
}

fn preferred_endpoints(src: &crate::graph::Node, dst: &crate::graph::Node) -> (usize, usize) {
    let (slo, shi) = inner_range(src);
    let (dlo, dhi) = inner_range(dst);
    let overlap_lo = slo.max(dlo);
    let overlap_hi = shi.min(dhi);
    if overlap_lo <= overlap_hi {
        let mid = (overlap_lo + overlap_hi) / 2;
        (mid, mid)
    } else {
        let src_center = src.x + src.width / 2;
        let dst_center = dst.x + dst.width / 2;
        (
            clamp(dst_center, slo, shi),
            clamp(src_center, dlo, dhi),
        )
    }
}

fn compute_endpoints(g: &Graph) -> HashMap<usize, (usize, usize, usize, usize)> {
    let mut out_by_node: HashMap<NodeId, Vec<usize>> = HashMap::new();
    let mut in_by_node: HashMap<NodeId, Vec<usize>> = HashMap::new();
    for (i, e) in g.edges.iter().enumerate() {
        out_by_node.entry(e.from).or_default().push(i);
        in_by_node.entry(e.to).or_default().push(i);
    }

    // Initial preferred per-edge endpoints
    let mut exit_x: HashMap<usize, usize> = HashMap::new();
    let mut entry_x: HashMap<usize, usize> = HashMap::new();
    for (i, e) in g.edges.iter().enumerate() {
        let (ex, en) = preferred_endpoints(&g.nodes[e.from], &g.nodes[e.to]);
        exit_x.insert(i, ex);
        entry_x.insert(i, en);
    }

    // Spread out-edges with collisions on the source bottom
    for (&node_id, edges) in &out_by_node {
        if edges.len() < 2 {
            continue;
        }
        let node = &g.nodes[node_id];
        let (lo, hi) = inner_range(node);
        let mut sorted = edges.clone();
        sorted.sort_by_key(|&ei| {
            let t = &g.nodes[g.edges[ei].to];
            t.x + t.width / 2
        });
        let mut ports: Vec<usize> = sorted.iter().map(|ei| exit_x[ei]).collect();
        // Forward: enforce strictly increasing
        for i in 1..ports.len() {
            if ports[i] <= ports[i - 1] {
                ports[i] = ports[i - 1] + 1;
            }
        }
        // Clamp to range
        for p in ports.iter_mut() {
            if *p > hi {
                *p = hi;
            }
        }
        // Backward fix-up if clamping broke order
        for i in (1..ports.len()).rev() {
            if ports[i] <= ports[i - 1] && ports[i - 1] > lo {
                ports[i - 1] = ports[i].saturating_sub(1).max(lo);
            }
        }
        for (i, ei) in sorted.iter().enumerate() {
            exit_x.insert(*ei, ports[i]);
        }
    }

    // Same for in-edges
    for (&node_id, edges) in &in_by_node {
        if edges.len() < 2 {
            continue;
        }
        let node = &g.nodes[node_id];
        let (lo, hi) = inner_range(node);
        let mut sorted = edges.clone();
        sorted.sort_by_key(|&ei| {
            let s = &g.nodes[g.edges[ei].from];
            s.x + s.width / 2
        });
        let mut ports: Vec<usize> = sorted.iter().map(|ei| entry_x[ei]).collect();
        for i in 1..ports.len() {
            if ports[i] <= ports[i - 1] {
                ports[i] = ports[i - 1] + 1;
            }
        }
        for p in ports.iter_mut() {
            if *p > hi {
                *p = hi;
            }
        }
        for i in (1..ports.len()).rev() {
            if ports[i] <= ports[i - 1] && ports[i - 1] > lo {
                ports[i - 1] = ports[i].saturating_sub(1).max(lo);
            }
        }
        for (i, ei) in sorted.iter().enumerate() {
            entry_x.insert(*ei, ports[i]);
        }
    }

    let mut result = HashMap::new();
    for (i, _e) in g.edges.iter().enumerate() {
        let src = &g.nodes[g.edges[i].from];
        let dst = &g.nodes[g.edges[i].to];
        let sx = *exit_x.get(&i).unwrap();
        let sy = src.y + src.height;
        let ex = *entry_x.get(&i).unwrap();
        let ey = dst.y;
        result.insert(i, (sx, sy, ex, ey));
    }
    result
}

fn clamp(v: usize, lo: usize, hi: usize) -> usize {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

fn draw_merge(canvas: &mut Canvas, g: &Graph, target_id: NodeId, edge_ids: &[usize]) {
    let dst = &g.nodes[target_id];
    let dx = dst.x + dst.width / 2;
    let dy = dst.y;

    let mut srcs: Vec<(usize, usize)> = edge_ids
        .iter()
        .map(|&ei| {
            let s = &g.nodes[g.edges[ei].from];
            (s.x + s.width / 2, s.y + s.height)
        })
        .collect();
    srcs.sort();
    srcs.dedup();

    let max_sy = srcs.iter().map(|&(_, sy)| sy).max().unwrap();
    let mid_y = if dy > max_sy + 1 {
        max_sy + (dy - max_sy) / 2
    } else {
        max_sy
    };

    for &(sx, sy) in &srcs {
        for y in sy..mid_y {
            canvas.set_overlay(sx, y, '│', CellKind::Edge);
        }
    }

    let leftmost_src = srcs.first().unwrap().0;
    let rightmost_src = srcs.last().unwrap().0;
    let bar_lo = leftmost_src.min(dx);
    let bar_hi = rightmost_src.max(dx);

    for x in bar_lo..=bar_hi {
        canvas.set_overlay(x, mid_y, '─', CellKind::Edge);
    }

    for &(sx, _) in &srcs {
        let ch = if sx == bar_lo {
            '╰'
        } else if sx == bar_hi {
            '╯'
        } else {
            '┴'
        };
        canvas.set(sx, mid_y, ch, CellKind::Edge);
    }

    let target_ch = if dx == bar_lo && dx != leftmost_src {
        Some('╭')
    } else if dx == bar_hi && dx != rightmost_src {
        Some('╮')
    } else if dx >= bar_lo && dx <= bar_hi && srcs.iter().all(|&(sx, _)| sx != dx) {
        Some('┬')
    } else {
        None
    };
    if let Some(ch) = target_ch {
        canvas.set(dx, mid_y, ch, CellKind::Edge);
    }

    for y in (mid_y + 1)..dy {
        canvas.set_overlay(dx, y, '│', CellKind::Edge);
    }
    if dy > mid_y + 1 {
        canvas.set(dx, dy - 1, '▼', CellKind::Arrow);
    } else if dy == mid_y + 1 {
        canvas.set(dx, mid_y, '▼', CellKind::Arrow);
    }
}

fn draw_edge(
    canvas: &mut Canvas,
    sx: usize,
    sy: usize,
    dx: usize,
    dy: usize,
    dst_is_dummy: bool,
) {
    if dy <= sy {
        return;
    }
    if sx == dx {
        for y in sy..dy {
            canvas.set_overlay(sx, y, '│', CellKind::Edge);
        }
        if !dst_is_dummy {
            canvas.set(sx, dy - 1, '▼', CellKind::Arrow);
        }
        return;
    }
    let mid_y = (sy + dy) / 2;
    for y in sy..mid_y {
        canvas.set_overlay(sx, y, '│', CellKind::Edge);
    }
    let (lo, hi) = if sx < dx { (sx, dx) } else { (dx, sx) };
    for x in (lo + 1)..hi {
        canvas.set_overlay(x, mid_y, '─', CellKind::Edge);
    }
    if sx < dx {
        canvas.set(sx, mid_y, '╰', CellKind::Edge);
        canvas.set(dx, mid_y, '╮', CellKind::Edge);
    } else {
        canvas.set(sx, mid_y, '╯', CellKind::Edge);
        canvas.set(dx, mid_y, '╭', CellKind::Edge);
    }
    for y in (mid_y + 1)..dy {
        canvas.set_overlay(dx, y, '│', CellKind::Edge);
    }
    if !dst_is_dummy && dy > mid_y + 1 {
        canvas.set(dx, dy - 1, '▼', CellKind::Arrow);
    }
}
