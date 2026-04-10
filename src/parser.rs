use crate::graph::{Direction, EdgeStyle, Graph, NodeId, Shape};

#[derive(Debug)]
struct EdgeHit {
    start: usize,
    end: usize,
    style: EdgeStyle,
    label: Option<String>,
}

const SIMPLE_OPS: &[(&str, EdgeStyle)] = &[
    ("-.->", EdgeStyle::Dotted),
    ("==>", EdgeStyle::Thick),
    ("-->", EdgeStyle::Normal),
    ("~~~", EdgeStyle::Invisible),
];

// Labeled edge forms. Opener requires a trailing space; closer requires a
// leading space — this disambiguates from the simple terminators.
const LABELED_OPS: &[(&str, &str, EdgeStyle)] = &[
    ("-- ", " -->", EdgeStyle::Normal),
    ("== ", " ==>", EdgeStyle::Thick),
    ("-. ", " .->", EdgeStyle::Dotted),
];

fn find_edge_op(s: &str, from: usize) -> Option<EdgeHit> {
    let mut best: Option<EdgeHit> = None;

    // Labeled forms first (they subsume simple terminators when present).
    for &(open, close, style) in LABELED_OPS {
        if let Some(open_rel) = s[from..].find(open) {
            let open_start = from + open_rel;
            let open_end = open_start + open.len();
            if let Some(close_rel) = s[open_end..].find(close) {
                let close_start = open_end + close_rel;
                let close_end = close_start + close.len();
                let text = s[open_end..close_start].trim();
                // Reject if the text itself contains another edge terminator —
                // that means we matched across two edges.
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
                    };
                    if best.as_ref().is_none_or(|b| hit.start < b.start) {
                        best = Some(hit);
                    }
                }
            }
        }
    }

    // Simple terminators
    for &(op, style) in SIMPLE_OPS {
        if let Some(rel) = s[from..].find(op) {
            let start = from + rel;
            let end = start + op.len();
            let hit = EdgeHit {
                start,
                end,
                style,
                label: None,
            };
            if best.as_ref().is_none_or(|b| hit.start < b.start) {
                best = Some(hit);
            }
        }
    }

    best
}

fn line_has_edge_op(s: &str) -> bool {
    find_edge_op(s, 0).is_some()
}

pub fn parse(source: &str) -> Result<Graph, String> {
    let mut g = Graph::new();
    for (lineno, raw) in source.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("%%") {
            continue;
        }
        if let Some(rest) = line
            .strip_prefix("flowchart")
            .or_else(|| line.strip_prefix("graph"))
        {
            let rest = rest.trim();
            if rest.eq_ignore_ascii_case("LR") || rest.eq_ignore_ascii_case("RL") {
                g.dir = Direction::LR;
            } else {
                g.dir = Direction::TD;
            }
            continue;
        }

        if line_has_edge_op(line) {
            parse_edge_line(&mut g, line)
                .map_err(|e| format!("line {}: {}", lineno + 1, e))?;
        } else {
            parse_node_decl(&mut g, line)
                .map_err(|e| format!("line {}: {}", lineno + 1, e))?;
        }
    }
    if g.nodes.is_empty() {
        return Err("no nodes found".to_string());
    }
    Ok(g)
}

fn parse_node_decl(g: &mut Graph, line: &str) -> Result<NodeId, String> {
    let (name, label, shape) = parse_ident_label(line)?;
    Ok(g.add_node(&name, &label, shape))
}

fn parse_edge_line(g: &mut Graph, line: &str) -> Result<(), String> {
    // Walk the line, splitting at any edge operator. Record both the node
    // segments and the edge hit (style + optional embedded label).
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

    let mut prev_id: Option<NodeId> = None;
    for (idx, raw) in segments.iter().enumerate() {
        let mut p = raw.trim();
        // Pipe-form edge label `|text|` prefixes the target of the previous edge.
        let mut pipe_label: Option<String> = None;
        if idx > 0 && p.starts_with('|') {
            if let Some(end) = p[1..].find('|') {
                let lbl = p[1..1 + end].trim().to_string();
                if !lbl.is_empty() {
                    pipe_label = Some(lbl);
                }
                p = p[end + 2..].trim();
            }
        }
        if p.is_empty() {
            return Err(format!("empty endpoint in edge: {}", line));
        }
        let (name, label, shape) = parse_ident_label(p)?;
        let id = g.add_node(&name, &label, shape);
        if let Some(pi) = prev_id {
            let hit = &hits[idx - 1];
            // Pipe form wins over embedded form if both somehow appear.
            let edge_label = pipe_label.or_else(|| hit.label.clone());
            g.add_edge(pi, id, edge_label, hit.style);
        }
        prev_id = Some(id);
    }
    Ok(())
}

// Mermaid node shape brackets → (open, close, Shape).
// Diamond `{...}` still renders Round per request.
const SHAPE_BRACKETS: &[(char, char, Shape)] = &[
    ('[', ']', Shape::Square),
    ('(', ')', Shape::Round),
    ('{', '}', Shape::Round),
];

fn parse_ident_label(s: &str) -> Result<(String, String, Shape), String> {
    let s = s.trim();
    let first_open = s
        .char_indices()
        .find(|(_, c)| SHAPE_BRACKETS.iter().any(|(o, _, _)| *o == *c));
    if let Some((i, open)) = first_open {
        let (_, close, shape) = *SHAPE_BRACKETS
            .iter()
            .find(|(o, _, _)| *o == open)
            .unwrap();
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
        Ok((name, label.to_string(), shape))
    } else {
        if s.is_empty() {
            return Err("empty identifier".to_string());
        }
        Ok((s.to_string(), String::new(), Shape::Round))
    }
}
