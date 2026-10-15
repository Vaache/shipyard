use crate::api::models::PrFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Hunk,
    Context,
    Add,
    Del,
    Meta,
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub id: usize,
    pub kind: LineKind,
    pub text: String,
    pub old_line: Option<i64>,
    pub new_line: Option<i64>,
}

impl DiffLine {
    pub fn anchor_side(&self) -> &'static str {
        if self.kind == LineKind::Del {
            "LEFT"
        } else {
            "RIGHT"
        }
    }

    pub fn anchor_line(&self) -> Option<i64> {
        if self.kind == LineKind::Del {
            self.old_line
        } else {
            self.new_line
        }
    }

    pub fn commentable(&self) -> bool {
        matches!(self.kind, LineKind::Add | LineKind::Del | LineKind::Context)
    }

    pub fn sign(&self) -> &'static str {
        match self.kind {
            LineKind::Add => "+",
            LineKind::Del => "-",
            LineKind::Context => " ",
            _ => "",
        }
    }

    pub fn gutter(&self, commentable: bool) -> String {
        let pad = |n: Option<i64>| format!("{:>5}", n.map(|n| n.to_string()).unwrap_or_default());
        format!("{} {}{}", pad(self.old_line), pad(self.new_line), if commentable && self.commentable() { " +" } else { "  " })
    }
}

#[derive(Debug, Clone)]
pub struct DiffFile {
    pub id: String,
    pub previous_filename: Option<String>,
    pub status: String,
    pub additions: i64,
    pub deletions: i64,
    pub lines: Vec<DiffLine>,
    pub has_patch: bool,
}

impl DiffFile {
    pub fn new(f: &PrFile) -> Self {
        DiffFile {
            id: f.filename.clone(),
            previous_filename: f.previous_filename.clone(),
            status: f.status.clone(),
            additions: f.additions,
            deletions: f.deletions,
            has_patch: f.patch.is_some(),
            lines: f.patch.as_deref().map(parse).unwrap_or_default(),
        }
    }

    pub fn status_letter(&self) -> &'static str {
        match self.status.as_str() {
            "added" => "A",
            "removed" => "D",
            "renamed" => "R",
            "copied" => "C",
            _ => "M",
        }
    }

    pub fn status_css(&self) -> &'static str {
        match self.status.as_str() {
            "added" => "green",
            "removed" => "red",
            "renamed" => "violet",
            _ => "amber",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiffAnchor {
    pub path: String,
    pub line: i64,
    pub side: String,
}

fn hunk_start(header: &str) -> (i64, i64) {
    let parts: Vec<&str> = header.split(' ').collect();
    let start = |token: Option<&&str>| -> i64 {
        token.and_then(|t| t[1..].split(',').next().and_then(|d| d.parse::<i64>().ok())).unwrap_or(1)
    };
    (start(parts.iter().find(|p| p.starts_with('-'))), start(parts.iter().find(|p| p.starts_with('+'))))
}

pub fn parse(patch: &str) -> Vec<DiffLine> {
    let mut result = Vec::with_capacity(patch.len() / 40);
    let (mut old, mut new) = (0i64, 0i64);
    for (idx, raw) in patch.split('\n').enumerate() {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if idx > 0 && raw.is_empty() && patch.ends_with('\n') && idx == patch.matches('\n').count() {
            break;
        }
        if raw.starts_with("@@") {
            let (o, n) = hunk_start(raw);
            old = o;
            new = n;
            result.push(DiffLine { id: idx, kind: LineKind::Hunk, text: raw.to_string(), old_line: None, new_line: None });
            continue;
        }
        let first = raw.chars().next();
        let body = if raw.is_empty() { String::new() } else { raw[first.map(|c| c.len_utf8()).unwrap_or(0)..].to_string() };
        match first {
            Some('+') => {
                result.push(DiffLine { id: idx, kind: LineKind::Add, text: body, old_line: None, new_line: Some(new) });
                new += 1;
            }
            Some('-') => {
                result.push(DiffLine { id: idx, kind: LineKind::Del, text: body, old_line: Some(old), new_line: None });
                old += 1;
            }
            Some('\\') => {
                result.push(DiffLine { id: idx, kind: LineKind::Meta, text: raw.to_string(), old_line: None, new_line: None });
            }
            _ => {
                result.push(DiffLine { id: idx, kind: LineKind::Context, text: body, old_line: Some(old), new_line: Some(new) });
                old += 1;
                new += 1;
            }
        }
    }
    result
}

pub fn parse_unified(text: &str) -> Vec<PrFile> {
    let mut files = vec![];
    let mut path: Option<String> = None;
    let mut old_path: Option<String> = None;
    let mut status = "modified".to_string();
    let mut patch: Vec<&str> = vec![];
    let (mut adds, mut dels) = (0i64, 0i64);
    let mut binary = false;

    let flush = |files: &mut Vec<PrFile>, path: &Option<String>, old_path: &Option<String>, status: &str, patch: &Vec<&str>, adds: i64, dels: i64, binary: bool| {
        if let Some(p) = path {
            let body = patch.join("\n");
            files.push(PrFile {
                filename: p.clone(),
                status: status.to_string(),
                additions: adds,
                deletions: dels,
                patch: if binary || body.is_empty() { None } else { Some(body) },
                previous_filename: if old_path.as_deref() != Some(p.as_str()) { old_path.clone() } else { None },
            });
        }
    };

    for line in text.split('\n') {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            flush(&mut files, &path, &old_path, &status, &patch, adds, dels, binary);
            path = None;
            old_path = None;
            status = "modified".into();
            patch = vec![];
            adds = 0;
            dels = 0;
            binary = false;
            let parts: Vec<&str> = rest.split(" b/").collect();
            if parts.len() >= 2 {
                old_path = Some(parts[0].get(2..).unwrap_or("").to_string());
                path = Some(parts[1..].join(" b/"));
            }
            continue;
        }
        if patch.is_empty() {
            if line.starts_with("new file") {
                status = "added".into();
                continue;
            }
            if line.starts_with("deleted file") {
                status = "removed".into();
                continue;
            }
            if let Some(from) = line.strip_prefix("rename from ") {
                status = "renamed".into();
                old_path = Some(from.to_string());
                continue;
            }
            if let Some(to) = line.strip_prefix("rename to ") {
                path = Some(to.to_string());
                continue;
            }
            if line.starts_with("Binary files") {
                binary = true;
                continue;
            }
            if line.starts_with("index ") || line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("similarity") || line.starts_with("old mode") || line.starts_with("new mode") {
                continue;
            }
        }
        if path.is_none() {
            continue;
        }
        if line.starts_with("@@") || !patch.is_empty() {
            patch.push(line);
            if line.starts_with('+') {
                adds += 1;
            } else if line.starts_with('-') {
                dels += 1;
            }
        }
    }
    flush(&mut files, &path, &old_path, &status, &patch, adds, dels, binary);
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_patch() {
        let lines = parse("@@ -10,3 +10,4 @@ fn x\n a\n-b\n+c\n+d\n e");
        assert_eq!(lines[0].kind, LineKind::Hunk);
        assert_eq!(lines[1].old_line, Some(10));
        assert_eq!(lines[2].kind, LineKind::Del);
        assert_eq!(lines[2].old_line, Some(11));
        assert_eq!(lines[3].new_line, Some(11));
        assert_eq!(lines[5].new_line, Some(13));
    }

    #[test]
    fn parses_unified() {
        let text = "diff --git a/x.txt b/x.txt\nindex 1..2 100644\n--- a/x.txt\n+++ b/x.txt\n@@ -1 +1 @@\n-a\n+b\ndiff --git a/n.txt b/n.txt\nnew file mode 100644\n--- /dev/null\n+++ b/n.txt\n@@ -0,0 +1 @@\n+hi\n";
        let files = parse_unified(text);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].additions, 1);
        assert_eq!(files[1].status, "added");
    }
}
