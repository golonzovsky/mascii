use crate::graph::{ArrowTip as GArrowTip, Direction, EdgeStyle, Graph, NodeId, Shape};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArrowTip {
    None,    // open link: ---
    Arrow,   // --> / ==> / -.-> / longer
    Cross,   // --x
    Circle,  // --o
}

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
            g.dir = if rest.eq_ignore_ascii_case("LR") {
                Direction::LR
            } else if rest.eq_ignore_ascii_case("RL") {
                Direction::RL
            } else if rest.eq_ignore_ascii_case("BT") {
                Direction::BT
            } else {
                // TD / TB / anything else → top-down
                Direction::TD
            };
            continue;
        }

        let result = if find_edge_op(line, 0).is_some() {
            parse_edge_line(&mut g, line)
        } else {
            let (name, label, shape) = parse_ident_label(line)?;
            g.add_node(&name, split_br(&label), shape);
            Ok(())
        };
        result.map_err(|e| format!("line {}: {}", lineno + 1, e))?;
    }
    if g.nodes.is_empty() {
        return Err("no nodes found".to_string());
    }
    Ok(g)
}

fn parse_edge_line(g: &mut Graph, line: &str) -> Result<(), String> {
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
            group.push(g.add_node(&name, split_br(&label), shape));
        }
        groups.push(group);
        pipe_labels.push(pipe_label);
    }

    for i in 1..groups.len() {
        let hit = &hits[i - 1];
        let edge_label = pipe_labels[i].clone().or_else(|| hit.label.clone());
        let tip_fwd = match hit.tip_fwd {
            ArrowTip::None => GArrowTip::None,
            ArrowTip::Arrow => GArrowTip::Arrow,
            ArrowTip::Cross => GArrowTip::Cross,
            ArrowTip::Circle => GArrowTip::Circle,
        };
        let left = groups[i - 1].clone();
        let right = groups[i].clone();
        for &from in &left {
            for &to in &right {
                g.add_edge(from, to, edge_label.clone(), hit.style, tip_fwd, hit.tip_back, hit.length);
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
