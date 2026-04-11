use crate::graph::{
    ArrowTip, Direction, EdgeStyle, Graph, NodeId, Shape, Subgraph, SubgraphId,
};
use crate::style::{Color, Style};

#[derive(Debug)]
struct EdgeHit {
    start: usize,
    end: usize,
    style: EdgeStyle,
    label: Option<String>,
    // Tip at the target side ("forward" direction).
    tip_fwd: ArrowTip,
    // Also an arrow back at the source (for `<-->`).
    tip_back: bool,
    /// Edge rank (1 = base length, 2 = `--->`, 3 = `---->`, …)
    length: usize,
}

// Labeled forms. Opener requires trailing space; closer requires leading space.
const LABELED_OPS: &[(&str, &str, EdgeStyle)] = &[
    ("-- ", " -->", EdgeStyle::Normal),
    ("== ", " ==>", EdgeStyle::Thick),
    ("-. ", " .->", EdgeStyle::Dotted),
];

// Scan a single simple edge token starting at `pos` and return its span
// plus the style + tip. Handles long forms (`-->`, `--->`, `---->`, `---`,
// `----`, `==>`, `===`, `-.->`, `~~~`, `--x`, `--o`, `<-->`, ...).
fn try_simple_at(s: &str, pos: usize) -> Option<EdgeHit> {
    let bytes = s.as_bytes();
    if pos >= bytes.len() {
        return None;
    }

    // Dotted: -.-> (exactly four chars, handle before dash run)
    if s[pos..].starts_with("-.->") {
        return Some(EdgeHit {
            start: pos,
            end: pos + 4,
            style: EdgeStyle::Dotted,
            label: None,
            tip_fwd: ArrowTip::Arrow,
            tip_back: false,
            length: 1,
        });
    }

    // Bidirectional: `<-->` (or longer). Leading `<`, then a dash run, then `>`.
    if bytes[pos] == b'<' {
        let mut end = pos + 1;
        while end < bytes.len() && bytes[end] == b'-' {
            end += 1;
        }
        let dashes = end - pos - 1;
        if dashes >= 2 && end < bytes.len() && bytes[end] == b'>' {
            return Some(EdgeHit {
                start: pos,
                end: end + 1,
                style: EdgeStyle::Normal,
                label: None,
                tip_fwd: ArrowTip::Arrow,
                tip_back: true,
                length: (dashes - 1).max(1),
            });
        }
    }

    // Dash run: `---`, `-->`, `--->`, `--x`, `--o`, ...
    if bytes[pos] == b'-' {
        let mut end = pos;
        while end < bytes.len() && bytes[end] == b'-' {
            end += 1;
        }
        let dash_count = end - pos;
        if dash_count >= 2 && end < bytes.len() {
            let tip = match bytes[end] {
                b'>' => Some(ArrowTip::Arrow),
                b'x' => Some(ArrowTip::Cross),
                b'o' => Some(ArrowTip::Circle),
                _ => None,
            };
            if let Some(tip_fwd) = tip {
                return Some(EdgeHit {
                    start: pos,
                    end: end + 1,
                    style: EdgeStyle::Normal,
                    label: None,
                    tip_fwd,
                    tip_back: false,
                    // 2 dashes = base length 1, 3 dashes = 2, …
                    length: (dash_count - 1).max(1),
                });
            }
        }
        if dash_count >= 3 {
            return Some(EdgeHit {
                start: pos,
                end,
                style: EdgeStyle::Normal,
                label: None,
                tip_fwd: ArrowTip::None,
                tip_back: false,
                // 3 dashes = base open, 4 = longer, …
                length: (dash_count - 2).max(1),
            });
        }
    }

    // Equals run: `==>`, `===`
    if bytes[pos] == b'=' {
        let mut end = pos;
        while end < bytes.len() && bytes[end] == b'=' {
            end += 1;
        }
        let eq_count = end - pos;
        if eq_count >= 2 && end < bytes.len() && bytes[end] == b'>' {
            return Some(EdgeHit {
                start: pos,
                end: end + 1,
                style: EdgeStyle::Thick,
                label: None,
                tip_fwd: ArrowTip::Arrow,
                tip_back: false,
                length: (eq_count - 1).max(1),
            });
        }
        if eq_count >= 3 {
            return Some(EdgeHit {
                start: pos,
                end,
                style: EdgeStyle::Thick,
                label: None,
                tip_fwd: ArrowTip::None,
                tip_back: false,
                length: (eq_count - 2).max(1),
            });
        }
    }

    // Invisible: ~~~
    if s[pos..].starts_with("~~~") {
        return Some(EdgeHit {
            start: pos,
            end: pos + 3,
            style: EdgeStyle::Invisible,
            label: None,
            length: 1,
            tip_fwd: ArrowTip::None,
            tip_back: false,
        });
    }

    None
}

fn find_edge_op(s: &str, from: usize) -> Option<EdgeHit> {
    let mut best: Option<EdgeHit> = None;

    // Labeled forms first.
    for &(open, close, style) in LABELED_OPS {
        if let Some(open_rel) = s[from..].find(open) {
            let open_start = from + open_rel;
            let open_end = open_start + open.len();
            if let Some(close_rel) = s[open_end..].find(close) {
                let close_start = open_end + close_rel;
                let close_end = close_start + close.len();
                let text = s[open_end..close_start].trim();
                if !text.is_empty()
                    && !text.contains("-->")
                    && !text.contains("==>")
                    && !text.contains(".->")
                {
                    let hit = EdgeHit {
                        start: open_start,
                        end: close_end,
                        style,
                        label: Some(text.to_string()),
                        tip_fwd: ArrowTip::Arrow,
                        tip_back: false,
                        length: 1,
                    };
                    if best.as_ref().is_none_or(|b| hit.start < b.start) {
                        best = Some(hit);
                    }
                }
            }
        }
    }

    // Simple forms: scan char boundaries for the earliest match.
    for (p, _) in s[from..].char_indices() {
        let pos = from + p;
        if let Some(hit) = try_simple_at(s, pos) {
            if best.as_ref().is_none_or(|b| hit.start < b.start) {
                best = Some(hit);
            }
            break;
        }
    }

    best
}

pub fn parse(source: &str) -> Result<Graph, String> {
    let mut g = Graph::new();
    let mut sg_stack: Vec<SubgraphId> = Vec::new();
    // Deferred directives that need to resolve node names (which might not
    // exist yet at the time of parsing), applied after a full first pass.
    let mut class_applications: Vec<(Vec<String>, String)> = Vec::new();
    let mut style_applications: Vec<(String, Style)> = Vec::new();

    for (lineno, raw) in source.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        if let Some(rest) = line
            .strip_prefix("flowchart")
            .or_else(|| line.strip_prefix("graph"))
        {
            let rest = rest.trim();
            g.dir = if rest.eq_ignore_ascii_case("LR") {
                Direction::LR
            } else if rest.eq_ignore_ascii_case("RL") {
                Direction::RL
            } else if rest.eq_ignore_ascii_case("BT") {
                Direction::BT
            } else {
                Direction::TD
            };
            continue;
        }

        // subgraph id [Label] / subgraph id / subgraph "Label"
        if let Some(rest) = line.strip_prefix("subgraph") {
            let rest = rest.trim();
            let (name, label, _) = parse_ident_label(rest)?;
            let id = g.subgraphs.len();
            g.subgraphs.push(Subgraph {
                name: if name.is_empty() { label.clone() } else { name },
                label,
                parent: sg_stack.last().copied(),
                style: Style::new(),
            });
            sg_stack.push(id);
            continue;
        }
        if line == "end" {
            sg_stack.pop();
            continue;
        }

        // classDef <name> <props>
        if let Some(rest) = line.strip_prefix("classDef") {
            let rest = rest.trim();
            let (name, props) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            let style = parse_css_props(props);
            g.class_defs.insert(name.to_string(), style);
            continue;
        }
        // class a,b,c <name>
        if let Some(rest) = line.strip_prefix("class ") {
            let rest = rest.trim();
            if let Some((node_list, class_name)) = rest.rsplit_once(char::is_whitespace) {
                let nodes: Vec<String> =
                    node_list.split(',').map(|s| s.trim().to_string()).collect();
                class_applications.push((nodes, class_name.to_string()));
            }
            continue;
        }
        // style <node> <props>
        if let Some(rest) = line.strip_prefix("style ") {
            let rest = rest.trim();
            if let Some((node, props)) = rest.split_once(char::is_whitespace) {
                style_applications.push((node.to_string(), parse_css_props(props)));
            }
            continue;
        }

        let result = if find_edge_op(line, 0).is_some() {
            parse_edge_line(&mut g, line, sg_stack.last().copied())
        } else {
            let (name, label, shape) = parse_ident_label(line)?;
            let id = g.add_node(&name, split_br(&label), shape);
            if let Some(&sg) = sg_stack.last() {
                g.nodes[id].subgraph = Some(sg);
            }
            Ok(())
        };
        result.map_err(|e| format!("line {}: {}", lineno + 1, e))?;
    }

    // Apply deferred class/style directives.
    for (nodes, class_name) in class_applications {
        let Some(style) = g.class_defs.get(&class_name).copied() else {
            continue;
        };
        for name in nodes {
            if let Some(&id) = g.name_to_id.get(&name) {
                g.nodes[id].style = merge_style(g.nodes[id].style, style);
            }
        }
    }
    for (name, style) in style_applications {
        if let Some(&id) = g.name_to_id.get(&name) {
            g.nodes[id].style = merge_style(g.nodes[id].style, style);
        }
    }

    if g.nodes.is_empty() {
        return Err("no nodes found".to_string());
    }
    Ok(g)
}

fn merge_style(mut base: Style, over: Style) -> Style {
    if over.fg.is_some() {
        base.fg = over.fg;
    }
    base.bold |= over.bold;
    base.italic |= over.italic;
    base.dim |= over.dim;
    base
}

/// Parse Mermaid CSS-style properties like `fill:#fdd,stroke:#c00,color:#fff`.
/// Only `fill`, `stroke`, `color` are recognised. Because we never paint box
/// interiors (rectangular backgrounds would break rounded corners), all three
/// map to `fg` — `stroke`/`color` win, `fill` is a fallback.
fn parse_css_props(s: &str) -> Style {
    let mut style = Style::new();
    let mut fill: Option<Color> = None;
    for part in s.split(',') {
        let Some((key, val)) = part.split_once(':') else {
            continue;
        };
        let Some(color) = Color::parse_hex(val.trim()) else {
            continue;
        };
        match key.trim() {
            "stroke" | "color" => style.fg = Some(color),
            "fill" => fill = Some(color),
            _ => {}
        }
    }
    if style.fg.is_none() {
        style.fg = fill;
    }
    style
}

fn parse_edge_line(g: &mut Graph, line: &str, sg: Option<SubgraphId>) -> Result<(), String> {
    // Split into segments by edge operator.
    let mut segments: Vec<&str> = Vec::new();
    let mut hits: Vec<EdgeHit> = Vec::new();
    let mut cursor = 0;
    loop {
        match find_edge_op(line, cursor) {
            Some(hit) => {
                segments.push(&line[cursor..hit.start]);
                cursor = hit.end;
                hits.push(hit);
            }
            None => {
                segments.push(&line[cursor..]);
                break;
            }
        }
    }
    if segments.len() < 2 {
        return Err(format!("bad edge: {}", line));
    }

    // Each segment is an `&`-separated group of nodes. Parse each group into
    // a Vec<NodeId>, then emit edges as the cross product between each
    // consecutive pair of groups.
    let mut groups: Vec<Vec<NodeId>> = Vec::with_capacity(segments.len());
    let mut pipe_labels: Vec<Option<String>> = Vec::with_capacity(segments.len());
    for (idx, raw) in segments.iter().enumerate() {
        let mut p = raw.trim();
        let mut pipe_label: Option<String> = None;
        if idx > 0
            && p.starts_with('|')
            && let Some(end) = p[1..].find('|')
        {
            let lbl = p[1..1 + end].trim().to_string();
            if !lbl.is_empty() {
                pipe_label = Some(lbl);
            }
            p = p[end + 2..].trim();
        }
        if p.is_empty() {
            return Err(format!("empty endpoint in edge: {}", line));
        }
        let mut group: Vec<NodeId> = Vec::new();
        for part in p.split('&') {
            let part = part.trim();
            if part.is_empty() {
                return Err(format!("empty endpoint in edge: {}", line));
            }
            let (name, label, shape) = parse_ident_label(part)?;
            let id = g.add_node(&name, split_br(&label), shape);
            if let Some(s) = sg
                && g.nodes[id].subgraph.is_none()
            {
                g.nodes[id].subgraph = Some(s);
            }
            group.push(id);
        }
        groups.push(group);
        pipe_labels.push(pipe_label);
    }

    for i in 1..groups.len() {
        let hit = &hits[i - 1];
        let edge_label = pipe_labels[i].clone().or_else(|| hit.label.clone());
        let left = groups[i - 1].clone();
        let right = groups[i].clone();
        for &from in &left {
            for &to in &right {
                g.add_edge(
                    from,
                    to,
                    edge_label.clone(),
                    hit.style,
                    hit.tip_fwd,
                    hit.tip_back,
                    hit.length,
                );
            }
        }
    }
    Ok(())
}

fn shape_for(open: char) -> Option<(char, Shape)> {
    match open {
        '[' => Some((']', Shape::Square)),
        '(' => Some((')', Shape::Round)),
        '{' => Some(('}', Shape::Round)),
        _ => None,
    }
}

fn parse_ident_label(s: &str) -> Result<(String, String, Shape), String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty identifier".to_string());
    }
    let Some((i, open, close, shape)) = s
        .char_indices()
        .find_map(|(i, c)| shape_for(c).map(|(cl, sh)| (i, c, cl, sh)))
    else {
        return Ok((s.to_string(), clean_label(s), Shape::Round));
    };
    let name = s[..i].trim().to_string();
    if name.is_empty() {
        return Err(format!("empty node name in '{}'", s));
    }
    let rest = &s[i + open.len_utf8()..];
    let end = rest
        .rfind(close)
        .ok_or_else(|| format!("missing closing '{}'", close))?;
    let mut label = rest[..end].trim();
    if label.len() >= 2 && label.starts_with('"') && label.ends_with('"') {
        label = &label[1..label.len() - 1];
    }
    Ok((name, clean_label(label), shape))
}

/// Strip Font Awesome prefix tokens and normalize the label. Does NOT split on
/// `<br>` — that's the caller's job via `split_br`.
fn clean_label(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for word in s.split_whitespace() {
        // Mermaid uses `fa:fa-something` or `fa:name` for FontAwesome icons;
        // we render as text so just drop them.
        if word.starts_with("fa:") {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// Split a label string on `<br>` / `<br/>` / `<br />` into rendered lines.
pub fn split_br(label: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = label;
    loop {
        // Case-insensitive search for the first `<br`
        let idx = rest
            .char_indices()
            .find(|&(i, _)| rest[i..].to_ascii_lowercase().starts_with("<br"));
        let Some((i, _)) = idx else {
            out.push(rest.trim().to_string());
            break;
        };
        out.push(rest[..i].trim().to_string());
        // Find the closing `>`.
        let after = &rest[i..];
        let Some(close) = after.find('>') else {
            // Malformed — take the rest verbatim.
            out.push(after.trim().to_string());
            break;
        };
        rest = &after[close + 1..];
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}
