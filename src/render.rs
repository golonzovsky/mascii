use crate::graph::{Direction, EdgeStyle, Graph, NodeId, Shape};
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
        // Violet borders, muted green lines, white labels, pink crossings.
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

// Side bitmask: each cell's line-art state. A single glyph is picked from
// the combination of connected sides and the edge style.
const UP: u8 = 1;
const DOWN: u8 = 2;
const LEFT: u8 = 4;
const RIGHT: u8 = 8;

// (normal, thick, dotted) per sides bitmask. Indices 0..16.
const GLYPHS: [(char, char, char); 16] = {
    let mut t = [(' ', ' ', ' '); 16];
    t[(UP | DOWN) as usize] = ('│', '┃', '┊');
    t[(LEFT | RIGHT) as usize] = ('─', '━', '┄');
    t[(UP | RIGHT) as usize] = ('╰', '┗', '╰');
    t[(UP | LEFT) as usize] = ('╯', '┛', '╯');
    t[(DOWN | RIGHT) as usize] = ('╭', '┏', '╭');
    t[(DOWN | LEFT) as usize] = ('╮', '┓', '╮');
    t[(UP | LEFT | RIGHT) as usize] = ('┴', '┻', '┴');
    t[(DOWN | LEFT | RIGHT) as usize] = ('┬', '┳', '┬');
    t[(UP | DOWN | RIGHT) as usize] = ('├', '┣', '├');
    t[(UP | DOWN | LEFT) as usize] = ('┤', '┫', '┤');
    t[(UP | DOWN | LEFT | RIGHT) as usize] = ('┼', '╋', '┼');
    t
};

fn lineart(sides: u8, style: EdgeStyle) -> char {
    let (n, t, d) = GLYPHS[(sides & 0xF) as usize];
    match style {
        EdgeStyle::Thick => t,
        EdgeStyle::Dotted => d,
        _ => n,
    }
}

// Direction-aware axis mapping. Every `draw_edge` / `draw_merge` works in
// (major, minor) coordinates; Axes converts to (x, y) and supplies direction-
// appropriate side bits and arrow glyph.
#[derive(Debug, Clone, Copy)]
struct Axes {
    dir: Direction,
}

impl Axes {
    fn xy(self, major: usize, minor: usize) -> (usize, usize) {
        match self.dir {
            Direction::TD => (minor, major),
            Direction::LR => (major, minor),
        }
    }
    fn major_sides(self) -> u8 {
        match self.dir {
            Direction::TD => UP | DOWN,
            Direction::LR => LEFT | RIGHT,
        }
    }
    fn minor_sides(self) -> u8 {
        match self.dir {
            Direction::TD => LEFT | RIGHT,
            Direction::LR => UP | DOWN,
        }
    }
    // "back" along major = toward the source (TD: UP; LR: LEFT)
    fn major_back(self) -> u8 {
        match self.dir {
            Direction::TD => UP,
            Direction::LR => LEFT,
        }
    }
    // "forward" along major = toward the target (TD: DOWN; LR: RIGHT)
    fn major_fwd(self) -> u8 {
        match self.dir {
            Direction::TD => DOWN,
            Direction::LR => RIGHT,
        }
    }
    // -minor (TD: LEFT; LR: UP)
    fn minor_back(self) -> u8 {
        match self.dir {
            Direction::TD => LEFT,
            Direction::LR => UP,
        }
    }
    // +minor (TD: RIGHT; LR: DOWN)
    fn minor_fwd(self) -> u8 {
        match self.dir {
            Direction::TD => RIGHT,
            Direction::LR => DOWN,
        }
    }
    fn arrow(self) -> char {
        match self.dir {
            Direction::TD => '▼',
            Direction::LR => '▶',
        }
    }
}

struct Canvas {
    chars: Vec<Vec<char>>,
    kinds: Vec<Vec<CellKind>>,
    sides: Vec<Vec<u8>>,
    cell_style: Vec<Vec<EdgeStyle>>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        Self {
            chars: vec![vec![' '; w]; h],
            kinds: vec![vec![CellKind::Empty; w]; h],
            sides: vec![vec![0u8; w]; h],
            cell_style: vec![vec![EdgeStyle::Normal; w]; h],
        }
    }

    fn in_bounds(&self, x: usize, y: usize) -> bool {
        y < self.chars.len() && x < self.chars[y].len()
    }

    // Direct write — for borders, labels, arrows (anything that isn't
    // line-art managed by the sides bitmask).
    fn set(&mut self, x: usize, y: usize, ch: char, kind: CellKind) {
        if self.in_bounds(x, y) {
            self.chars[y][x] = ch;
            self.kinds[y][x] = kind;
        }
    }

    // Add line-art sides to a cell and recompute its glyph. Two plain
    // straight runs (│ + ─) crossing becomes `╳` (crossing, not junction).
    // Thick > Dotted > Normal when styles differ.
    fn add_sides(&mut self, x: usize, y: usize, new: u8, style: EdgeStyle, kind: CellKind) {
        if !self.in_bounds(x, y) || new == 0 {
            return;
        }
        let old = self.sides[y][x];
        // Crossing: a plain vertical meeting a plain horizontal.
        if (old == (UP | DOWN) && new == (LEFT | RIGHT))
            || (old == (LEFT | RIGHT) && new == (UP | DOWN))
        {
            self.chars[y][x] = '╳';
            self.kinds[y][x] = CellKind::Crossing;
            // Don't OR into sides — subsequent ops will treat it as empty.
            return;
        }
        let combined = old | new;
        self.sides[y][x] = combined;
        let cur_style = self.cell_style[y][x];
        let merged_style = style.max_over(cur_style);
        self.cell_style[y][x] = merged_style;
        self.chars[y][x] = lineart(combined, merged_style);
        self.kinds[y][x] = kind;
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

    let axes = Axes { dir: g.dir };

    // Dummies (pass-throughs) — inherit style from incoming edge.
    for n in &g.nodes {
        if !n.is_dummy {
            continue;
        }
        let style = g
            .edges
            .iter()
            .find(|e| e.to == n.id)
            .map(|e| e.style)
            .unwrap_or(EdgeStyle::Normal);
        if style == EdgeStyle::Invisible {
            continue;
        }
        let (major_start, major_end, minor) = match g.dir {
            Direction::TD => (n.y, n.y + n.height, n.x),
            Direction::LR => (n.x, n.x + n.width, n.y),
        };
        for m in major_start..major_end {
            let (x, y) = axes.xy(m, minor);
            canvas.add_sides(x, y, axes.major_sides(), style, CellKind::Edge);
        }
    }

    let endpoints = compute_endpoints(g);

    // Group edges by target so merges render as a single bar.
    let mut by_target: HashMap<NodeId, Vec<usize>> = HashMap::new();
    for (i, e) in g.edges.iter().enumerate() {
        by_target.entry(e.to).or_default().push(i);
    }
    let mut targets: Vec<NodeId> = by_target.keys().copied().collect();
    targets.sort();
    for tid in targets {
        let edges = &by_target[&tid];
        let target = &g.nodes[tid];
        let visible: Vec<usize> = edges
            .iter()
            .copied()
            .filter(|&i| g.edges[i].style != EdgeStyle::Invisible)
            .collect();
        if visible.is_empty() {
            continue;
        }
        if visible.len() == 1 || target.is_dummy {
            for i in visible {
                let (sx, sy, dx, dy) = endpoints[i];
                let (sm, smn, dm, dmn) = match g.dir {
                    Direction::TD => (sy, sx, dy, dx),
                    Direction::LR => (sx, sy, dx, dy),
                };
                draw_edge(
                    &mut canvas,
                    axes,
                    sm,
                    smn,
                    dm,
                    dmn,
                    target.is_dummy,
                    g.edges[i].style,
                );
            }
        } else {
            draw_merge(&mut canvas, axes, g, tid, &visible);
        }
    }

    for (i, e) in g.edges.iter().enumerate() {
        let Some(text) = e.label.as_deref() else { continue };
        if text.is_empty() {
            continue;
        }
        let (sx, sy, _dx, _dy) = endpoints[i];
        match g.dir {
            Direction::TD => place_edge_label_td(&mut canvas, sx, sy, text),
            Direction::LR => place_edge_label_lr(&mut canvas, sx, sy, text),
        }
    }

    emit(&canvas, theme)
}

fn place_edge_label_td(canvas: &mut Canvas, sx: usize, sy: usize, text: &str) {
    // Center label on the edge column, replacing the `│` on that row.
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

// Minor-axis inner range (TD: x, LR: y).
fn inner_range(n: &crate::graph::Node, dir: Direction) -> (usize, usize) {
    if n.is_dummy {
        match dir {
            Direction::TD => (n.x, n.x),
            Direction::LR => (n.y, n.y),
        }
    } else {
        match dir {
            Direction::TD => {
                if n.width >= 3 {
                    (n.x + 1, n.x + n.width - 2)
                } else {
                    (n.x, n.x + n.width.saturating_sub(1))
                }
            }
            Direction::LR => {
                if n.height >= 3 {
                    (n.y + 1, n.y + n.height - 2)
                } else {
                    (n.y, n.y + n.height.saturating_sub(1))
                }
            }
        }
    }
}

fn minor_center(n: &crate::graph::Node, dir: Direction) -> usize {
    match dir {
        Direction::TD => n.x + n.width / 2,
        Direction::LR => n.y + n.height / 2,
    }
}

fn preferred_endpoints(
    src: &crate::graph::Node,
    dst: &crate::graph::Node,
    dir: Direction,
) -> (usize, usize) {
    let (slo, shi) = inner_range(src, dir);
    let (dlo, dhi) = inner_range(dst, dir);
    let overlap_lo = slo.max(dlo);
    let overlap_hi = shi.min(dhi);
    if overlap_lo <= overlap_hi {
        let mid = (overlap_lo + overlap_hi) / 2;
        (mid, mid)
    } else {
        (
            clamp(minor_center(dst, dir), slo, shi),
            clamp(minor_center(src, dir), dlo, dhi),
        )
    }
}

/// Adjust a sorted-by-key list of edge ports so they're strictly increasing
/// and fall inside `[lo, hi]`. Operates in place.
fn spread_ports(ports: &mut [usize], lo: usize, hi: usize) {
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
}

fn compute_endpoints(g: &Graph) -> Vec<(usize, usize, usize, usize)> {
    let dir = g.dir;
    let n_edges = g.edges.len();

    // Initial per-edge minor-axis ports (TD=x, LR=y).
    let mut exit_minor: Vec<usize> = vec![0; n_edges];
    let mut entry_minor: Vec<usize> = vec![0; n_edges];
    for (i, e) in g.edges.iter().enumerate() {
        let (ex, en) = preferred_endpoints(&g.nodes[e.from], &g.nodes[e.to], dir);
        exit_minor[i] = ex;
        entry_minor[i] = en;
    }

    // Group edges by source / target and spread colliding ports.
    let mut out_by_node: HashMap<NodeId, Vec<usize>> = HashMap::new();
    let mut in_by_node: HashMap<NodeId, Vec<usize>> = HashMap::new();
    for (i, e) in g.edges.iter().enumerate() {
        out_by_node.entry(e.from).or_default().push(i);
        in_by_node.entry(e.to).or_default().push(i);
    }

    let spread = |by_node: &HashMap<NodeId, Vec<usize>>,
                  ports: &mut [usize],
                  other_end: fn(&Graph, usize) -> NodeId| {
        for (&node_id, edges) in by_node {
            if edges.len() < 2 {
                continue;
            }
            let (lo, hi) = inner_range(&g.nodes[node_id], dir);
            let mut sorted = edges.clone();
            sorted.sort_by_key(|&ei| minor_center(&g.nodes[other_end(g, ei)], dir));
            let mut ps: Vec<usize> = sorted.iter().map(|ei| ports[*ei]).collect();
            spread_ports(&mut ps, lo, hi);
            for (i, ei) in sorted.iter().enumerate() {
                ports[*ei] = ps[i];
            }
        }
    };
    spread(&out_by_node, &mut exit_minor, |g, ei| g.edges[ei].to);
    spread(&in_by_node, &mut entry_minor, |g, ei| g.edges[ei].from);

    (0..n_edges)
        .map(|i| {
            let src = &g.nodes[g.edges[i].from];
            let dst = &g.nodes[g.edges[i].to];
            match dir {
                Direction::TD => (exit_minor[i], src.y + src.height, entry_minor[i], dst.y),
                Direction::LR => (src.x + src.width, exit_minor[i], dst.x, entry_minor[i]),
            }
        })
        .collect()
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

// L-shape / straight edge in (major, minor) coordinates. Works for both
// directions via `axes`.
#[allow(clippy::too_many_arguments)]
fn draw_edge(
    canvas: &mut Canvas,
    axes: Axes,
    sm: usize,
    smn: usize,
    dm: usize,
    dmn: usize,
    dst_is_dummy: bool,
    style: EdgeStyle,
) {
    if dm <= sm {
        return;
    }
    if smn == dmn {
        for m in sm..dm {
            let (x, y) = axes.xy(m, smn);
            canvas.add_sides(x, y, axes.major_sides(), style, CellKind::Edge);
        }
        if !dst_is_dummy {
            let (x, y) = axes.xy(dm - 1, smn);
            canvas.set(x, y, axes.arrow(), CellKind::Arrow);
        }
        return;
    }
    let mid_m = (sm + dm) / 2;

    for m in sm..mid_m {
        let (x, y) = axes.xy(m, smn);
        canvas.add_sides(x, y, axes.major_sides(), style, CellKind::Edge);
    }
    // Source corner: connects back (toward src) + side toward dst.
    let toward_dst = if dmn > smn { axes.minor_fwd() } else { axes.minor_back() };
    let (x, y) = axes.xy(mid_m, smn);
    canvas.add_sides(x, y, axes.major_back() | toward_dst, style, CellKind::Edge);

    let (lo, hi) = if smn < dmn { (smn, dmn) } else { (dmn, smn) };
    for mn in (lo + 1)..hi {
        let (x, y) = axes.xy(mid_m, mn);
        canvas.add_sides(x, y, axes.minor_sides(), style, CellKind::Edge);
    }

    // Target corner: connects side from src + forward (toward dst).
    let from_src = if smn < dmn { axes.minor_back() } else { axes.minor_fwd() };
    let (x, y) = axes.xy(mid_m, dmn);
    canvas.add_sides(x, y, from_src | axes.major_fwd(), style, CellKind::Edge);

    for m in (mid_m + 1)..dm {
        let (x, y) = axes.xy(m, dmn);
        canvas.add_sides(x, y, axes.major_sides(), style, CellKind::Edge);
    }

    if !dst_is_dummy && dm > mid_m + 1 {
        let (x, y) = axes.xy(dm - 1, dmn);
        canvas.set(x, y, axes.arrow(), CellKind::Arrow);
    }
}

// Multi-source merge onto a shared target via a bar on the minor axis.
fn draw_merge(
    canvas: &mut Canvas,
    axes: Axes,
    g: &Graph,
    target_id: NodeId,
    edge_ids: &[usize],
) {
    let dst = &g.nodes[target_id];
    let (dm, dmn) = match axes.dir {
        Direction::TD => (dst.y, dst.x + dst.width / 2),
        Direction::LR => (dst.x, dst.y + dst.height / 2),
    };

    let style = edge_ids
        .iter()
        .map(|&i| g.edges[i].style)
        .fold(EdgeStyle::Normal, |a, b| a.max_over(b));

    // Per-source: (major, minor, per-edge style) for its exit run.
    let mut srcs: Vec<(usize, usize, EdgeStyle)> = edge_ids
        .iter()
        .map(|&ei| {
            let s = &g.nodes[g.edges[ei].from];
            let (m, mn) = match axes.dir {
                Direction::TD => (s.y + s.height, s.x + s.width / 2),
                Direction::LR => (s.x + s.width, s.y + s.height / 2),
            };
            (m, mn, g.edges[ei].style)
        })
        .collect();
    srcs.sort_by_key(|&(_, mn, _)| mn);
    srcs.dedup_by_key(|&mut (m, mn, _)| (m, mn));

    let max_sm = srcs.iter().map(|&(m, _, _)| m).max().unwrap();
    let mid_m = if dm > max_sm + 1 {
        max_sm + (dm - max_sm) / 2
    } else {
        max_sm
    };

    // Major-axis runs: each source forward to mid_m, in its own style.
    for &(sm, smn, src_style) in &srcs {
        for m in sm..mid_m {
            let (x, y) = axes.xy(m, smn);
            canvas.add_sides(x, y, axes.major_sides(), src_style, CellKind::Edge);
        }
    }

    // Bar along minor axis at mid_m from bar_lo to bar_hi.
    let top_mn = srcs.first().unwrap().1;
    let bot_mn = srcs.last().unwrap().1;
    let bar_lo = top_mn.min(dmn);
    let bar_hi = bot_mn.max(dmn);
    for mn in bar_lo..=bar_hi {
        let mut sides = 0u8;
        if mn > bar_lo {
            sides |= axes.minor_back();
        }
        if mn < bar_hi {
            sides |= axes.minor_fwd();
        }
        if sides != 0 {
            let (x, y) = axes.xy(mid_m, mn);
            canvas.add_sides(x, y, sides, style, CellKind::Edge);
        }
    }

    // Source taps: each contributes an "incoming from back" side.
    for &(_, smn, src_style) in &srcs {
        let (x, y) = axes.xy(mid_m, smn);
        canvas.add_sides(x, y, axes.major_back(), src_style, CellKind::Edge);
    }

    // Target tap: bar drops forward at dmn.
    let (x, y) = axes.xy(mid_m, dmn);
    canvas.add_sides(x, y, axes.major_fwd(), style, CellKind::Edge);

    // Drop from bar to target.
    for m in (mid_m + 1)..dm {
        let (x, y) = axes.xy(m, dmn);
        canvas.add_sides(x, y, axes.major_sides(), style, CellKind::Edge);
    }
    if dm > mid_m + 1 {
        let (x, y) = axes.xy(dm - 1, dmn);
        canvas.set(x, y, axes.arrow(), CellKind::Arrow);
    } else if dm == mid_m + 1 {
        let (x, y) = axes.xy(mid_m, dmn);
        canvas.set(x, y, axes.arrow(), CellKind::Arrow);
    }
}

fn place_edge_label_lr(canvas: &mut Canvas, sx: usize, sy: usize, text: &str) {
    // Centered on the horizontal drop, leaving LR_LABEL_PAD `─` chars each side.
    if sy >= canvas.chars.len() {
        return;
    }
    let row = &canvas.chars[sy];
    let len = text.chars().count();
    if len == 0 {
        return;
    }
    let mut run_end = sx;
    while run_end < row.len() && matches!(row[run_end], '─' | '━' | '┄' | ' ') {
        run_end += 1;
    }
    let run_len = run_end - sx;
    let pad = crate::layout::LR_LABEL_PAD;
    if run_len < len + 2 * pad {
        return;
    }
    let extra = run_len - len - 2 * pad;
    let start = sx + pad + extra / 2;
    for k in 0..len {
        let c = row[start + k];
        if c != ' ' && c != '─' && c != '━' && c != '┄' {
            return;
        }
    }
    for (k, ch) in text.chars().enumerate() {
        canvas.set(start + k, sy, ch, CellKind::Label);
    }
}

