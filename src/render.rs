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

#[derive(Debug, Clone, Copy)]
struct GlyphSet {
    vert: char,
    horiz: char,
    corner_dl: char, // ╰ (comes from up, goes right)
    corner_dr: char, // ╯ (comes from up, goes left)
    corner_ur: char, // ╭ (comes from right, goes down)
    corner_ul: char, // ╮ (comes from left, goes down)
    tap_up: char,    // ┴
    tap_down: char,  // ┬
}

const GLYPH_NORMAL: GlyphSet = GlyphSet {
    vert: '│',
    horiz: '─',
    corner_dl: '╰',
    corner_dr: '╯',
    corner_ur: '╭',
    corner_ul: '╮',
    tap_up: '┴',
    tap_down: '┬',
};

const GLYPH_THICK: GlyphSet = GlyphSet {
    vert: '┃',
    horiz: '━',
    corner_dl: '┗',
    corner_dr: '┛',
    corner_ur: '┏',
    corner_ul: '┓',
    tap_up: '┻',
    tap_down: '┳',
};

const GLYPH_DOTTED: GlyphSet = GlyphSet {
    vert: '┊',
    horiz: '┄',
    corner_dl: '╰',
    corner_dr: '╯',
    corner_ur: '╭',
    corner_ul: '╮',
    tap_up: '┴',
    tap_down: '┬',
};

fn glyphs_for(style: EdgeStyle) -> GlyphSet {
    match style {
        EdgeStyle::Normal => GLYPH_NORMAL,
        EdgeStyle::Thick => GLYPH_THICK,
        EdgeStyle::Dotted => GLYPH_DOTTED,
        EdgeStyle::Invisible => GLYPH_NORMAL,
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

    // Merge a new corner onto an existing cell. Two corners sharing a side
    // collapse into a tap (`┤`, `├`, `┬`, `┴`) and two opposite corners into
    // a cross. Used for fan-out/fan-in where multiple edges share a turn.
    fn set_corner(&mut self, x: usize, y: usize, ch: char, kind: CellKind) {
        let cur = self.get_char(x, y);
        if cur == ' ' || cur == '─' || cur == '│' || cur == '━' || cur == '┃'
            || cur == '┄' || cur == '┊'
        {
            self.set(x, y, ch, kind);
            return;
        }
        let merged = match (cur, ch) {
            // Up-left corner + up-right corner → tap up (two verticals meet on bottom)
            ('╯', '╰') | ('╰', '╯') => Some('┴'),
            // Down-left corner + down-right corner → tap down
            ('╭', '╮') | ('╮', '╭') => Some('┬'),
            // Two vertical corners sharing "vertical-above" and "vertical-below" on left
            ('╯', '╮') | ('╮', '╯') => Some('┤'),
            ('╰', '╭') | ('╭', '╰') => Some('├'),
            _ => None,
        };
        if let Some(m) = merged {
            self.set(x, y, m, kind);
        } else {
            self.set(x, y, ch, kind);
        }
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

    // Dummies (pass-throughs) — use the style of the edge that enters the
    // dummy, falling back to Normal. Invisible dummies render as blank.
    for n in &g.nodes {
        if !n.is_dummy {
            continue;
        }
        let incoming_style = g
            .edges
            .iter()
            .find(|e| e.to == n.id)
            .map(|e| e.style)
            .unwrap_or(EdgeStyle::Normal);
        if incoming_style == EdgeStyle::Invisible {
            continue;
        }
        let gl = glyphs_for(incoming_style);
        match g.dir {
            Direction::TD => {
                for d in 0..n.height {
                    canvas.set(n.x, n.y + d, gl.vert, CellKind::Edge);
                }
            }
            Direction::LR => {
                for d in 0..n.width {
                    canvas.set(n.x + d, n.y, gl.horiz, CellKind::Edge);
                }
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
                let (sx, sy, dx, dy) = endpoints[&i];
                match g.dir {
                    Direction::TD => draw_edge_td(
                        &mut canvas,
                        sx,
                        sy,
                        dx,
                        dy,
                        target.is_dummy,
                        g.edges[i].style,
                    ),
                    Direction::LR => draw_edge_lr(
                        &mut canvas,
                        sx,
                        sy,
                        dx,
                        dy,
                        target.is_dummy,
                        g.edges[i].style,
                    ),
                }
            }
        } else {
            match g.dir {
                Direction::TD => draw_merge_td(&mut canvas, g, tid, &visible),
                Direction::LR => draw_merge_lr(&mut canvas, g, tid, &visible),
            }
        }
    }

    // Edge labels
    for (i, e) in g.edges.iter().enumerate() {
        let Some(text) = e.label.as_deref() else { continue };
        if text.is_empty() {
            continue;
        }
        let (sx, sy, _dx, _dy) = endpoints[&i];
        match g.dir {
            Direction::TD => place_edge_label_td(&mut canvas, sx, sy, text),
            Direction::LR => place_edge_label_lr(&mut canvas, sx, sy, text),
        }
    }

    emit(&canvas, theme)
}

fn place_edge_label_td(canvas: &mut Canvas, sx: usize, sy: usize, text: &str) {
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

// Inner range along the minor (cross) axis for a node.
// TD minor = x, LR minor = y.
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

fn compute_endpoints(g: &Graph) -> HashMap<usize, (usize, usize, usize, usize)> {
    let dir = g.dir;
    let mut out_by_node: HashMap<NodeId, Vec<usize>> = HashMap::new();
    let mut in_by_node: HashMap<NodeId, Vec<usize>> = HashMap::new();
    for (i, e) in g.edges.iter().enumerate() {
        out_by_node.entry(e.from).or_default().push(i);
        in_by_node.entry(e.to).or_default().push(i);
    }

    // `exit_minor`/`entry_minor` are positions on the MINOR axis:
    //   TD: minor = x (column along the box bottom/top)
    //   LR: minor = y (row along the box right/left)
    let mut exit_minor: HashMap<usize, usize> = HashMap::new();
    let mut entry_minor: HashMap<usize, usize> = HashMap::new();
    for (i, e) in g.edges.iter().enumerate() {
        let (ex, en) = preferred_endpoints(&g.nodes[e.from], &g.nodes[e.to], dir);
        exit_minor.insert(i, ex);
        entry_minor.insert(i, en);
    }

    // Spread out-edges with collisions on the source's minor axis
    for (&node_id, edges) in &out_by_node {
        if edges.len() < 2 {
            continue;
        }
        let node = &g.nodes[node_id];
        let (lo, hi) = inner_range(node, dir);
        let mut sorted = edges.clone();
        sorted.sort_by_key(|&ei| minor_center(&g.nodes[g.edges[ei].to], dir));
        let mut ports: Vec<usize> = sorted.iter().map(|ei| exit_minor[ei]).collect();
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
            exit_minor.insert(*ei, ports[i]);
        }
    }

    // Same for in-edges
    for (&node_id, edges) in &in_by_node {
        if edges.len() < 2 {
            continue;
        }
        let node = &g.nodes[node_id];
        let (lo, hi) = inner_range(node, dir);
        let mut sorted = edges.clone();
        sorted.sort_by_key(|&ei| minor_center(&g.nodes[g.edges[ei].from], dir));
        let mut ports: Vec<usize> = sorted.iter().map(|ei| entry_minor[ei]).collect();
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
            entry_minor.insert(*ei, ports[i]);
        }
    }

    let mut result = HashMap::new();
    for (i, _e) in g.edges.iter().enumerate() {
        let src = &g.nodes[g.edges[i].from];
        let dst = &g.nodes[g.edges[i].to];
        let s_minor = *exit_minor.get(&i).unwrap();
        let e_minor = *entry_minor.get(&i).unwrap();
        let (sx, sy, dx, dy) = match dir {
            Direction::TD => (s_minor, src.y + src.height, e_minor, dst.y),
            Direction::LR => (src.x + src.width, s_minor, dst.x, e_minor),
        };
        result.insert(i, (sx, sy, dx, dy));
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

// LR merge: sources on the left flow into a target on the right. The "bar"
// is a vertical segment gathering all source y-positions, then a single
// horizontal drop to the target's entry point.
fn draw_merge_lr(canvas: &mut Canvas, g: &Graph, target_id: NodeId, edge_ids: &[usize]) {
    let dst = &g.nodes[target_id];
    let dy = dst.y + dst.height / 2;
    let dx = dst.x;

    let style = if edge_ids.iter().any(|&i| g.edges[i].style == EdgeStyle::Thick) {
        EdgeStyle::Thick
    } else if edge_ids
        .iter()
        .any(|&i| g.edges[i].style == EdgeStyle::Dotted)
    {
        EdgeStyle::Dotted
    } else {
        EdgeStyle::Normal
    };
    let gl = glyphs_for(style);

    // Source points: (exit_x = src.x+width, exit_y = src center row)
    let mut srcs: Vec<(usize, usize)> = edge_ids
        .iter()
        .map(|&ei| {
            let s = &g.nodes[g.edges[ei].from];
            (s.x + s.width, s.y + s.height / 2)
        })
        .collect();
    srcs.sort_by_key(|&(_, y)| y);
    srcs.dedup();

    let max_sx = srcs.iter().map(|&(sx, _)| sx).max().unwrap();
    let mid_x = if dx > max_sx + 1 {
        max_sx + (dx - max_sx) / 2
    } else {
        max_sx
    };

    // Horizontal runs from each source to mid_x
    for &(sx, sy) in &srcs {
        for x in sx..mid_x {
            canvas.set_overlay(x, sy, gl.horiz, CellKind::Edge);
        }
    }

    let topmost_src = srcs.first().unwrap().1;
    let bottommost_src = srcs.last().unwrap().1;
    let bar_lo = topmost_src.min(dy);
    let bar_hi = bottommost_src.max(dy);

    // Vertical bar
    for y in bar_lo..=bar_hi {
        canvas.set_overlay(mid_x, y, gl.vert, CellKind::Edge);
    }

    // Source sites on the bar (horizontal enters from left).
    //   bar_lo (top):       ╮  (horizontal-left + vertical-below)
    //   bar_hi (bottom):    ╯  (horizontal-left + vertical-above)
    //   intermediate:       ┤  (horizontal-left + vertical-both)
    for &(_, sy) in &srcs {
        let ch = if sy == bar_lo {
            gl.corner_ul // ╮
        } else if sy == bar_hi {
            gl.corner_dr // ╯
        } else {
            '┤'
        };
        canvas.set(mid_x, sy, ch, CellKind::Edge);
    }

    // Target site on the bar (horizontal extends right to the target).
    //   bar_lo (top):       ╭  (horizontal-right + vertical-below)
    //   bar_hi (bottom):    ╰  (horizontal-right + vertical-above)
    //   intermediate:       ├  (horizontal-right + vertical-both)
    let target_ch = if dy == bar_lo && dy != topmost_src {
        Some(gl.corner_ur)
    } else if dy == bar_hi && dy != bottommost_src {
        Some(gl.corner_dl)
    } else if dy >= bar_lo && dy <= bar_hi && srcs.iter().all(|&(_, sy)| sy != dy) {
        Some('├')
    } else {
        None
    };
    if let Some(ch) = target_ch {
        canvas.set(mid_x, dy, ch, CellKind::Edge);
    }

    // Drop from bar to target
    for x in (mid_x + 1)..dx {
        canvas.set_overlay(x, dy, gl.horiz, CellKind::Edge);
    }
    if dx > mid_x + 1 {
        canvas.set(dx - 1, dy, '▶', CellKind::Arrow);
    } else if dx == mid_x + 1 {
        canvas.set(mid_x, dy, '▶', CellKind::Arrow);
    }
}

fn place_edge_label_lr(canvas: &mut Canvas, sx: usize, sy: usize, text: &str) {
    // Inline placement on the horizontal drop. Layout already widened the
    // channel for us to fit: `─` * PAD + label + `─` * PAD + `▶`. Place the
    // label leaving LR_LABEL_PAD horizontal line chars on each side.
    if sy >= canvas.chars.len() {
        return;
    }
    let row = &canvas.chars[sy];
    let len = text.chars().count();
    if len == 0 {
        return;
    }
    // Walk right from sx over horizontal-line / space cells to find the run.
    let mut run_end = sx;
    while run_end < row.len() && matches!(row[run_end], '─' | '━' | '┄' | ' ') {
        run_end += 1;
    }
    let run_len = run_end - sx;
    let pad = crate::layout::LR_LABEL_PAD;
    // Need at minimum: pad + len + pad chars of run before any non-run char.
    if run_len < len + 2 * pad {
        return;
    }
    // Center the label within the run, but never closer than `pad` to the
    // start.
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

fn draw_merge_td(canvas: &mut Canvas, g: &Graph, target_id: NodeId, edge_ids: &[usize]) {
    let dst = &g.nodes[target_id];
    let dx = dst.x + dst.width / 2;
    let dy = dst.y;

    // Pick glyphs: if any contributing edge is thick, render the whole merge
    // thick; otherwise dotted if any is dotted; otherwise normal.
    let style = if edge_ids.iter().any(|&i| g.edges[i].style == EdgeStyle::Thick) {
        EdgeStyle::Thick
    } else if edge_ids
        .iter()
        .any(|&i| g.edges[i].style == EdgeStyle::Dotted)
    {
        EdgeStyle::Dotted
    } else {
        EdgeStyle::Normal
    };
    let gl = glyphs_for(style);

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
            canvas.set_overlay(sx, y, gl.vert, CellKind::Edge);
        }
    }

    let leftmost_src = srcs.first().unwrap().0;
    let rightmost_src = srcs.last().unwrap().0;
    let bar_lo = leftmost_src.min(dx);
    let bar_hi = rightmost_src.max(dx);

    for x in bar_lo..=bar_hi {
        canvas.set_overlay(x, mid_y, gl.horiz, CellKind::Edge);
    }

    for &(sx, _) in &srcs {
        let ch = if sx == bar_lo {
            gl.corner_dl
        } else if sx == bar_hi {
            gl.corner_dr
        } else {
            gl.tap_up
        };
        canvas.set(sx, mid_y, ch, CellKind::Edge);
    }

    let target_ch = if dx == bar_lo && dx != leftmost_src {
        Some(gl.corner_ur)
    } else if dx == bar_hi && dx != rightmost_src {
        Some(gl.corner_ul)
    } else if dx >= bar_lo && dx <= bar_hi && srcs.iter().all(|&(sx, _)| sx != dx) {
        Some(gl.tap_down)
    } else {
        None
    };
    if let Some(ch) = target_ch {
        canvas.set(dx, mid_y, ch, CellKind::Edge);
    }

    for y in (mid_y + 1)..dy {
        canvas.set_overlay(dx, y, gl.vert, CellKind::Edge);
    }
    if dy > mid_y + 1 {
        canvas.set(dx, dy - 1, '▼', CellKind::Arrow);
    } else if dy == mid_y + 1 {
        canvas.set(dx, mid_y, '▼', CellKind::Arrow);
    }
}

fn draw_edge_td(
    canvas: &mut Canvas,
    sx: usize,
    sy: usize,
    dx: usize,
    dy: usize,
    dst_is_dummy: bool,
    style: EdgeStyle,
) {
    if dy <= sy {
        return;
    }
    let gl = glyphs_for(style);
    if sx == dx {
        for y in sy..dy {
            canvas.set_overlay(sx, y, gl.vert, CellKind::Edge);
        }
        if !dst_is_dummy {
            canvas.set(sx, dy - 1, '▼', CellKind::Arrow);
        }
        return;
    }
    let mid_y = (sy + dy) / 2;
    for y in sy..mid_y {
        canvas.set_overlay(sx, y, gl.vert, CellKind::Edge);
    }
    let (lo, hi) = if sx < dx { (sx, dx) } else { (dx, sx) };
    for x in (lo + 1)..hi {
        canvas.set_overlay(x, mid_y, gl.horiz, CellKind::Edge);
    }
    if sx < dx {
        canvas.set(sx, mid_y, gl.corner_dl, CellKind::Edge);
        canvas.set(dx, mid_y, gl.corner_ul, CellKind::Edge);
    } else {
        canvas.set(sx, mid_y, gl.corner_dr, CellKind::Edge);
        canvas.set(dx, mid_y, gl.corner_ur, CellKind::Edge);
    }
    for y in (mid_y + 1)..dy {
        canvas.set_overlay(dx, y, gl.vert, CellKind::Edge);
    }
    if !dst_is_dummy && dy > mid_y + 1 {
        canvas.set(dx, dy - 1, '▼', CellKind::Arrow);
    }
}

// LR variant: edges flow left-to-right. sx/sy = exit point (just right of
// source box), dx/dy = entry point (just left of target). The "channel" is
// horizontal space between source's right edge and target's left edge.
fn draw_edge_lr(
    canvas: &mut Canvas,
    sx: usize,
    sy: usize,
    dx: usize,
    dy: usize,
    dst_is_dummy: bool,
    style: EdgeStyle,
) {
    if dx <= sx {
        return;
    }
    let gl = glyphs_for(style);
    if sy == dy {
        for x in sx..dx {
            canvas.set_overlay(x, sy, gl.horiz, CellKind::Edge);
        }
        if !dst_is_dummy {
            canvas.set(dx - 1, sy, '▶', CellKind::Arrow);
        }
        return;
    }
    let mid_x = (sx + dx) / 2;
    for x in sx..mid_x {
        canvas.set_overlay(x, sy, gl.horiz, CellKind::Edge);
    }
    let (lo, hi) = if sy < dy { (sy, dy) } else { (dy, sy) };
    for y in (lo + 1)..hi {
        canvas.set_overlay(mid_x, y, gl.vert, CellKind::Edge);
    }
    if sy < dy {
        canvas.set_corner(mid_x, sy, gl.corner_ul, CellKind::Edge); // ╮
        canvas.set_corner(mid_x, dy, gl.corner_dl, CellKind::Edge); // ╰
    } else {
        canvas.set_corner(mid_x, sy, gl.corner_dr, CellKind::Edge); // ╯
        canvas.set_corner(mid_x, dy, gl.corner_ur, CellKind::Edge); // ╭
    }
    for x in (mid_x + 1)..dx {
        canvas.set_overlay(x, dy, gl.horiz, CellKind::Edge);
    }
    if !dst_is_dummy && dx > mid_x + 1 {
        canvas.set(dx - 1, dy, '▶', CellKind::Arrow);
    }
}
