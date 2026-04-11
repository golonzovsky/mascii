use crate::graph::{ArrowTip, Direction, EdgeStyle, Graph, NodeId, Shape, SubgraphId};
use crate::style::{Color, Style};
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
    pub border: Style,
    pub label: Style,
    pub edge: Style,
    pub arrow: Style,
    pub crossing: Style,
}

impl Theme {
    pub const fn plain() -> Self {
        Self {
            border: Style::new(),
            label: Style::new(),
            edge: Style::new(),
            arrow: Style::new(),
            crossing: Style::new(),
        }
    }

    pub const fn grey() -> Self {
        let g = Style::fg(Color::GREY);
        Self {
            border: g,
            label: Style::new(),
            edge: g,
            arrow: g,
            crossing: g,
        }
    }

    pub const fn mono() -> Self {
        Self {
            border: Style::fg(Color::GREY),
            label: Style::new(),
            edge: Style::fg(Color::GREY),
            arrow: Style::fg(Color::BRIGHT_WHITE),
            crossing: Style::fg(Color::BRIGHT_YELLOW),
        }
    }

    pub const fn neon() -> Self {
        // Violet borders, muted green lines, white labels, pink crossings.
        Self {
            border: Style::fg(Color::VIOLET),
            label: Style::fg(Color::WHITE),
            edge: Style::fg(Color::NEON_GREEN),
            arrow: Style::fg(Color::NEON_GREEN),
            crossing: Style::fg(Color::HOT_PINK),
        }
    }

    pub const fn dim() -> Self {
        Self {
            border: Style::dim(),
            label: Style::dim(),
            edge: Style::dim(),
            arrow: Style::dim(),
            crossing: Style::dim(),
        }
    }

    fn style_for(&self, kind: CellKind) -> Style {
        match kind {
            CellKind::Empty => Style::new(),
            CellKind::Border => self.border,
            CellKind::Label => self.label,
            CellKind::Edge => self.edge,
            CellKind::Arrow => self.arrow,
            CellKind::Crossing => self.crossing,
        }
    }

    fn is_plain(&self) -> bool {
        self.border.is_empty()
            && self.label.is_empty()
            && self.edge.is_empty()
            && self.arrow.is_empty()
            && self.crossing.is_empty()
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

// Internally `Axes` always flows TD or LR; BT/RL are emitted by flipping the
// whole canvas at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InnerDir {
    TD,
    LR,
}

fn inner_dir(dir: Direction) -> InnerDir {
    if dir.is_vertical() {
        InnerDir::TD
    } else {
        InnerDir::LR
    }
}

fn flip_glyph_v(ch: char) -> char {
    match ch {
        '▼' => '▲',
        '▲' => '▼',
        '╭' => '╰',
        '╮' => '╯',
        '╰' => '╭',
        '╯' => '╮',
        '┌' => '└',
        '┐' => '┘',
        '└' => '┌',
        '┘' => '┐',
        '┏' => '┗',
        '┓' => '┛',
        '┗' => '┏',
        '┛' => '┓',
        '┬' => '┴',
        '┴' => '┬',
        '┳' => '┻',
        '┻' => '┳',
        '╱' => '╲',
        '╲' => '╱',
        _ => ch,
    }
}

fn flip_glyph_h(ch: char) -> char {
    match ch {
        '▶' => '◀',
        '◀' => '▶',
        '╭' => '╮',
        '╮' => '╭',
        '╰' => '╯',
        '╯' => '╰',
        '┌' => '┐',
        '┐' => '┌',
        '└' => '┘',
        '┘' => '└',
        '┏' => '┓',
        '┓' => '┏',
        '┗' => '┛',
        '┛' => '┗',
        '├' => '┤',
        '┤' => '├',
        '┣' => '┫',
        '┫' => '┣',
        '╱' => '╲',
        '╲' => '╱',
        _ => ch,
    }
}

#[derive(Debug, Clone, Copy)]
struct Axes {
    dir: InnerDir,
}

impl Axes {
    fn xy(self, major: usize, minor: usize) -> (usize, usize) {
        match self.dir {
            InnerDir::TD => (minor, major),
            InnerDir::LR => (major, minor),
        }
    }
    fn major_sides(self) -> u8 {
        match self.dir {
            InnerDir::TD => UP | DOWN,
            InnerDir::LR => LEFT | RIGHT,
        }
    }
    fn minor_sides(self) -> u8 {
        match self.dir {
            InnerDir::TD => LEFT | RIGHT,
            InnerDir::LR => UP | DOWN,
        }
    }
    fn major_back(self) -> u8 {
        match self.dir {
            InnerDir::TD => UP,
            InnerDir::LR => LEFT,
        }
    }
    fn major_fwd(self) -> u8 {
        match self.dir {
            InnerDir::TD => DOWN,
            InnerDir::LR => RIGHT,
        }
    }
    fn minor_back(self) -> u8 {
        match self.dir {
            InnerDir::TD => LEFT,
            InnerDir::LR => UP,
        }
    }
    fn minor_fwd(self) -> u8 {
        match self.dir {
            InnerDir::TD => RIGHT,
            InnerDir::LR => DOWN,
        }
    }
    fn arrow(self) -> char {
        match self.dir {
            InnerDir::TD => '▼',
            InnerDir::LR => '▶',
        }
    }
    fn back_arrow(self) -> char {
        match self.dir {
            InnerDir::TD => '▲',
            InnerDir::LR => '◀',
        }
    }
}

fn tip_char(tip: ArrowTip, axes: Axes) -> Option<char> {
    match tip {
        ArrowTip::None => None,
        ArrowTip::Arrow => Some(axes.arrow()),
        ArrowTip::Cross => Some('×'),
        ArrowTip::Circle => Some('○'),
    }
}

struct Canvas {
    chars: Vec<Vec<char>>,
    kinds: Vec<Vec<CellKind>>,
    sides: Vec<Vec<u8>>,
    cell_style: Vec<Vec<EdgeStyle>>,
    // Per-cell style override: non-empty values win over the theme.
    override_style: Vec<Vec<Style>>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        Self {
            chars: vec![vec![' '; w]; h],
            kinds: vec![vec![CellKind::Empty; w]; h],
            sides: vec![vec![0u8; w]; h],
            cell_style: vec![vec![EdgeStyle::Normal; w]; h],
            override_style: vec![vec![Style::new(); w]; h],
        }
    }

    fn set_override(&mut self, x: usize, y: usize, style: Style) {
        if self.in_bounds(x, y) {
            self.override_style[y][x] = style;
        }
    }

    /// Grow the canvas to at least `w × h`.
    fn ensure(&mut self, w: usize, h: usize) {
        while self.chars.len() < h {
            self.chars.push(vec![' '; w.max(self.width())]);
            self.kinds.push(vec![CellKind::Empty; w.max(self.width())]);
            self.sides.push(vec![0u8; w.max(self.width())]);
            self.cell_style.push(vec![EdgeStyle::Normal; w.max(self.width())]);
            self.override_style.push(vec![Style::new(); w.max(self.width())]);
        }
        for row in &mut self.chars {
            while row.len() < w {
                row.push(' ');
            }
        }
        for row in &mut self.kinds {
            while row.len() < w {
                row.push(CellKind::Empty);
            }
        }
        for row in &mut self.sides {
            while row.len() < w {
                row.push(0);
            }
        }
        for row in &mut self.cell_style {
            while row.len() < w {
                row.push(EdgeStyle::Normal);
            }
        }
        for row in &mut self.override_style {
            while row.len() < w {
                row.push(Style::new());
            }
        }
    }

    fn width(&self) -> usize {
        self.chars.first().map(|r| r.len()).unwrap_or(0)
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

    /// Reverse row order and vertically mirror every directional glyph.
    fn flip_v(&mut self) {
        self.chars.reverse();
        self.kinds.reverse();
        self.sides.reverse();
        self.cell_style.reverse();
        self.override_style.reverse();
        for row in &mut self.chars {
            for c in row.iter_mut() {
                *c = flip_glyph_v(*c);
            }
        }
    }

    /// Reverse column order within each row and horizontally mirror every
    /// directional glyph. Label runs are re-reversed so text reads forward.
    fn flip_h(&mut self) {
        for row in &mut self.chars {
            row.reverse();
            for c in row.iter_mut() {
                *c = flip_glyph_h(*c);
            }
        }
        for row in &mut self.kinds {
            row.reverse();
        }
        for row in &mut self.sides {
            row.reverse();
        }
        for row in &mut self.cell_style {
            row.reverse();
        }
        for row in &mut self.override_style {
            row.reverse();
        }
        // Re-reverse any contiguous run of Label cells so text reads L→R.
        for (chars, kinds) in self.chars.iter_mut().zip(self.kinds.iter()) {
            let mut i = 0;
            while i < kinds.len() {
                if kinds[i] == CellKind::Label {
                    let start = i;
                    while i < kinds.len() && kinds[i] == CellKind::Label {
                        i += 1;
                    }
                    chars[start..i].reverse();
                } else {
                    i += 1;
                }
            }
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

    // Boxes first. For directions that require a final canvas flip (BT / RL),
    // pre-reverse the label lines so they read naturally after the flip.
    let reverse_label_rows = g.dir == Direction::BT;
    for n in &g.nodes {
        if n.is_dummy {
            continue;
        }
        let lines: Vec<String> = if reverse_label_rows {
            n.label_lines.iter().rev().cloned().collect()
        } else {
            n.label_lines.clone()
        };
        draw_box(&mut canvas, n.x, n.y, n.width, n.height, &lines, n.shape);
        // Apply the node's user style as a per-cell override over the whole
        // box rectangle so borders get `stroke`/`color` and the interior gets
        // `fill` as a background.
        if !n.style.is_empty() {
            for dy in 0..n.height {
                for dx in 0..n.width {
                    canvas.set_override(n.x + dx, n.y + dy, n.style);
                }
            }
        }
    }

    let axes = Axes { dir: inner_dir(g.dir) };

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
        let (major_start, major_end, minor) = match axes.dir {
            InnerDir::TD => (n.y, n.y + n.height, n.x),
            InnerDir::LR => (n.x, n.x + n.width, n.y),
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
                let (sm, smn, dm, dmn) = match axes.dir {
                    InnerDir::TD => (sy, sx, dy, dx),
                    InnerDir::LR => (sx, sy, dx, dy),
                };
                let e = &g.edges[i];
                // Dummy targets never get a visible tip.
                let tip = if target.is_dummy {
                    ArrowTip::None
                } else {
                    e.tip_fwd
                };
                draw_edge(
                    &mut canvas,
                    axes,
                    sm,
                    smn,
                    dm,
                    dmn,
                    tip,
                    e.tip_back,
                    e.style,
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
        match axes.dir {
            InnerDir::TD => place_edge_label_td(&mut canvas, sx, sy, text),
            InnerDir::LR => place_edge_label_lr(&mut canvas, sx, sy, text),
        }
    }

    // Subgraph containers: draw a bordered box around all nodes tagged with
    // each subgraph, grown to cover their bounding rectangle plus padding.
    // We draw containers AFTER the node boxes/edges so their borders overlay
    // anything that strays outside. Innermost subgraphs drawn last.
    draw_subgraph_containers(&mut canvas, g);

    match g.dir {
        Direction::BT => canvas.flip_v(),
        Direction::RL => canvas.flip_h(),
        _ => {}
    }

    emit(&canvas, theme)
}

fn draw_subgraph_containers(canvas: &mut Canvas, g: &Graph) {
    // Pad around contained nodes — more vertical than horizontal since the
    // title bar lives on top.
    const PAD_X: usize = 2;
    const PAD_TOP: usize = 2;
    const PAD_BOTTOM: usize = 1;

    // Order by nesting depth (outer first, inner last) so inner borders end
    // up visible above outer ones.
    let mut ordered: Vec<usize> = (0..g.subgraphs.len()).collect();
    ordered.sort_by_key(|&sid| depth(g, sid));

    for sid in ordered {
        let mut min_x = usize::MAX;
        let mut max_x = 0usize;
        let mut min_y = usize::MAX;
        let mut max_y = 0usize;
        let mut any = false;
        for n in &g.nodes {
            if !g.node_in_subgraph(n.id, sid) {
                continue;
            }
            any = true;
            min_x = min_x.min(n.x);
            max_x = max_x.max(n.x + n.width - 1);
            min_y = min_y.min(n.y);
            max_y = max_y.max(n.y + n.height - 1);
        }
        if !any {
            continue;
        }
        let left = min_x.saturating_sub(PAD_X);
        let right = max_x + PAD_X;
        let top = min_y.saturating_sub(PAD_TOP);
        let bottom = max_y + PAD_BOTTOM;

        // Expand canvas if the container ran past its current bounds.
        canvas.ensure(right + 1, bottom + 1);

        let sg = &g.subgraphs[sid];
        // Draw container cells, but never overwrite an adjacent node's box
        // border. Edges and empty cells are fair game.
        let mut put = |cx: usize, cy: usize, ch: char| {
            let kind = canvas.kinds.get(cy).and_then(|r| r.get(cx)).copied();
            if !matches!(kind, Some(CellKind::Border)) {
                canvas.set(cx, cy, ch, CellKind::Border);
            }
        };
        for x in left + 1..right {
            put(x, top, '─');
            put(x, bottom, '─');
        }
        for y in top + 1..bottom {
            put(left, y, '│');
            put(right, y, '│');
        }
        put(left, top, '┌');
        put(right, top, '┐');
        put(left, bottom, '└');
        put(right, bottom, '┘');

        // Title: "── Label ──" on the top border, left-anchored.
        let label = if !sg.label.is_empty() {
            sg.label.as_str()
        } else {
            sg.name.as_str()
        };
        if !label.is_empty() {
            let title_len = label.chars().count();
            let inner = (right - left).saturating_sub(2);
            if title_len <= inner.saturating_sub(2) {
                let start = left + 2;
                // Leading space, then label, then trailing space before the
                // border continues.
                canvas.set(start - 1, top, ' ', CellKind::Border);
                for (k, ch) in label.chars().enumerate() {
                    canvas.set(start + k, top, ch, CellKind::Label);
                }
                canvas.set(start + title_len, top, ' ', CellKind::Border);
            }
        }

        if !sg.style.is_empty() {
            for y in top..=bottom {
                for x in left..=right {
                    if y == top || y == bottom || x == left || x == right {
                        canvas.set_override(x, y, sg.style);
                    }
                }
            }
        }
    }
}

fn depth(g: &Graph, sid: SubgraphId) -> usize {
    let mut d = 0;
    let mut cur = g.subgraphs[sid].parent;
    while let Some(p) = cur {
        d += 1;
        cur = g.subgraphs[p].parent;
    }
    d
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
    let apply_overrides = !theme.is_plain();
    let mut out = String::new();
    for ((row_chars, row_kinds), row_override) in canvas
        .chars
        .iter()
        .zip(canvas.kinds.iter())
        .zip(canvas.override_style.iter())
    {
        // Row length for trimming: include any cell with non-space char OR a
        // non-empty override (background colors matter even for space cells).
        let end = (0..row_chars.len())
            .rev()
            .find(|&i| {
                row_chars[i] != ' ' || (apply_overrides && !row_override[i].is_empty())
            })
            .map(|i| i + 1)
            .unwrap_or(0);
        if end == 0 {
            out.push('\n');
            continue;
        }
        let mut current = Style::new();
        for i in 0..end {
            let ch = row_chars[i];
            let kind = row_kinds[i];
            let base = theme.style_for(kind);
            let want = if apply_overrides {
                combine_style(base, row_override[i], kind)
            } else {
                base
            };
            if want != current {
                if !current.is_empty() {
                    out.push_str(RESET);
                }
                want.write(&mut out);
                current = want;
            }
            out.push(ch);
        }
        if !current.is_empty() {
            out.push_str(RESET);
        }
        out.push('\n');
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    while out.starts_with('\n') {
        out.remove(0);
    }
    out
}

/// Merge a per-cell override onto the theme's style. Only border cells pick
/// up the override's `fg` (stroke); box interiors stay untouched because a
/// rectangular background can't follow rounded corners.
fn combine_style(base: Style, over: Style, kind: CellKind) -> Style {
    if kind == CellKind::Border && let Some(fg) = over.fg {
        Style { fg: Some(fg), ..base }
    } else {
        base
    }
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
fn inner_range(n: &crate::graph::Node, dir: InnerDir) -> (usize, usize) {
    if n.is_dummy {
        match dir {
            InnerDir::TD => (n.x, n.x),
            InnerDir::LR => (n.y, n.y),
        }
    } else {
        match dir {
            InnerDir::TD => {
                if n.width >= 3 {
                    (n.x + 1, n.x + n.width - 2)
                } else {
                    (n.x, n.x + n.width.saturating_sub(1))
                }
            }
            InnerDir::LR => {
                if n.height >= 3 {
                    (n.y + 1, n.y + n.height - 2)
                } else {
                    (n.y, n.y + n.height.saturating_sub(1))
                }
            }
        }
    }
}

fn minor_center(n: &crate::graph::Node, dir: InnerDir) -> usize {
    match dir {
        InnerDir::TD => n.x + n.width / 2,
        InnerDir::LR => n.y + n.height / 2,
    }
}

fn preferred_endpoints(
    src: &crate::graph::Node,
    dst: &crate::graph::Node,
    dir: InnerDir,
) -> (usize, usize) {
    let (slo, shi) = inner_range(src, dir);
    let (dlo, dhi) = inner_range(dst, dir);
    // Prefer dst's own center — it's the "natural" attach point and, for
    // chains of mixed widths, ensures all edges land on the same column
    // (the narrowest box in the chain). Fall back to src's center, then
    // the overlap midpoint.
    let dst_center = minor_center(dst, dir);
    if dst_center >= slo && dst_center <= shi && dst_center >= dlo && dst_center <= dhi {
        return (dst_center, dst_center);
    }
    let src_center = minor_center(src, dir);
    if src_center >= slo && src_center <= shi && src_center >= dlo && src_center <= dhi {
        return (src_center, src_center);
    }
    let overlap_lo = slo.max(dlo);
    let overlap_hi = shi.min(dhi);
    if overlap_lo <= overlap_hi {
        let mid = (overlap_lo + overlap_hi) / 2;
        (mid, mid)
    } else {
        (
            clamp(dst_center, slo, shi),
            clamp(src_center, dlo, dhi),
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
    let dir = inner_dir(g.dir);
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
                InnerDir::TD => (exit_minor[i], src.y + src.height, entry_minor[i], dst.y),
                InnerDir::LR => (src.x + src.width, exit_minor[i], dst.x, entry_minor[i]),
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
    tip_fwd: ArrowTip,
    tip_back: bool,
    style: EdgeStyle,
) {
    if dm <= sm {
        return;
    }
    let fwd_char = tip_char(tip_fwd, axes);
    if smn == dmn {
        for m in sm..dm {
            let (x, y) = axes.xy(m, smn);
            canvas.add_sides(x, y, axes.major_sides(), style, CellKind::Edge);
        }
        if let Some(ch) = fwd_char {
            let (x, y) = axes.xy(dm - 1, smn);
            canvas.set(x, y, ch, CellKind::Arrow);
        }
        if tip_back {
            let (x, y) = axes.xy(sm, smn);
            canvas.set(x, y, axes.back_arrow(), CellKind::Arrow);
        }
        return;
    }
    let mid_m = (sm + dm) / 2;

    for m in sm..mid_m {
        let (x, y) = axes.xy(m, smn);
        canvas.add_sides(x, y, axes.major_sides(), style, CellKind::Edge);
    }
    let toward_dst = if dmn > smn { axes.minor_fwd() } else { axes.minor_back() };
    let (x, y) = axes.xy(mid_m, smn);
    canvas.add_sides(x, y, axes.major_back() | toward_dst, style, CellKind::Edge);

    let (lo, hi) = if smn < dmn { (smn, dmn) } else { (dmn, smn) };
    for mn in (lo + 1)..hi {
        let (x, y) = axes.xy(mid_m, mn);
        canvas.add_sides(x, y, axes.minor_sides(), style, CellKind::Edge);
    }

    let from_src = if smn < dmn { axes.minor_back() } else { axes.minor_fwd() };
    let (x, y) = axes.xy(mid_m, dmn);
    canvas.add_sides(x, y, from_src | axes.major_fwd(), style, CellKind::Edge);

    for m in (mid_m + 1)..dm {
        let (x, y) = axes.xy(m, dmn);
        canvas.add_sides(x, y, axes.major_sides(), style, CellKind::Edge);
    }

    if let Some(ch) = fwd_char
        && dm > mid_m + 1
    {
        let (x, y) = axes.xy(dm - 1, dmn);
        canvas.set(x, y, ch, CellKind::Arrow);
    }
    if tip_back {
        let (x, y) = axes.xy(sm, smn);
        canvas.set(x, y, axes.back_arrow(), CellKind::Arrow);
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
        InnerDir::TD => (dst.y, dst.x + dst.width / 2),
        InnerDir::LR => (dst.x, dst.y + dst.height / 2),
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
                InnerDir::TD => (s.y + s.height, s.x + s.width / 2),
                InnerDir::LR => (s.x + s.width, s.y + s.height / 2),
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

    // Bar segments: one per source, each in its own style, running from the
    // source column to the target column. Overlapping cells at the target
    // column get the dominant style via `max_over` inside `add_sides`.
    for &(_, smn, src_style) in &srcs {
        let (lo, hi) = if smn < dmn { (smn, dmn) } else { (dmn, smn) };
        for mn in lo..=hi {
            let mut sides = 0u8;
            if mn > lo {
                sides |= axes.minor_back();
            }
            if mn < hi {
                sides |= axes.minor_fwd();
            }
            if sides != 0 {
                let (x, y) = axes.xy(mid_m, mn);
                canvas.add_sides(x, y, sides, src_style, CellKind::Edge);
            }
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

