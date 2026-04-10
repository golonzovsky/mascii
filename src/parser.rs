use crate::graph::{Graph, NodeId};

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
        if line.starts_with("flowchart") || line.starts_with("graph") {
            continue;
        }

        if line.contains("-->") {
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
    let (name, label) = parse_ident_label(line)?;
    Ok(g.add_node(&name, &label))
}

fn parse_edge_line(g: &mut Graph, line: &str) -> Result<(), String> {
    let parts: Vec<&str> = line.split("-->").collect();
    if parts.len() < 2 {
        return Err(format!("bad edge: {}", line));
    }
    let mut prev_id: Option<NodeId> = None;
    for (idx, p) in parts.iter().enumerate() {
        let mut p = p.trim();
        // A Mermaid edge label `|text|` attaches to the preceding edge and
        // appears as a prefix on the TARGET endpoint text after splitting on
        // `-->`. Strip it (we don't yet render edge labels).
        if idx > 0 && p.starts_with('|') {
            if let Some(end) = p[1..].find('|') {
                p = p[end + 2..].trim();
            }
        }
        if p.is_empty() {
            return Err(format!("empty endpoint in edge: {}", line));
        }
        let (name, label) = parse_ident_label(p)?;
        let id = g.add_node(&name, &label);
        if let Some(pi) = prev_id {
            g.add_edge(pi, id);
        }
        prev_id = Some(id);
    }
    Ok(())
}

// Mermaid node shape brackets. All render as rectangles in v1.
const SHAPE_BRACKETS: &[(char, char)] = &[
    ('[', ']'), // square
    ('(', ')'), // round
    ('{', '}'), // diamond
];

fn parse_ident_label(s: &str) -> Result<(String, String), String> {
    let s = s.trim();
    // Find the first occurrence of any opening shape bracket.
    let first_open = s
        .char_indices()
        .find(|(_, c)| SHAPE_BRACKETS.iter().any(|(o, _)| *o == *c));
    if let Some((i, open)) = first_open {
        let close = SHAPE_BRACKETS
            .iter()
            .find(|(o, _)| *o == open)
            .map(|(_, c)| *c)
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
        Ok((name, label.to_string()))
    } else {
        if s.is_empty() {
            return Err("empty identifier".to_string());
        }
        Ok((s.to_string(), String::new()))
    }
}
