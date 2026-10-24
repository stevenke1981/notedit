//! Sidebars (folder workspace tree, function list) and the two-file compare.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------ folder tree

#[derive(Clone, Debug, PartialEq)]
pub struct TreeRow { pub path: PathBuf, pub depth: usize, pub dir: bool, pub open: bool }

const SKIP: &[&str] = &[".git", "target", "node_modules", ".hg", ".svn", "__pycache__", ".idea", ".vs"];

fn children(dir: &Path) -> Vec<(PathBuf, bool)> {
    let mut v: Vec<(PathBuf, bool)> = std::fs::read_dir(dir).map(|rd| rd.flatten().filter_map(|e| {
        let name = e.file_name().to_string_lossy().to_string();
        let is_dir = e.file_type().ok()?.is_dir();
        if is_dir && SKIP.contains(&name.as_str()) { return None; }
        Some((e.path(), is_dir))
    }).collect()).unwrap_or_default();
    // folders first, then case-insensitive by name (Explorer / Notepad++ order)
    v.sort_by_key(|(p, d)| (!*d, p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default()));
    v
}

/// Flattened, visible rows of the tree under `root` (root itself excluded).
pub fn flatten(root: &Path, expanded: &HashSet<PathBuf>, limit: usize) -> Vec<TreeRow> {
    fn walk(dir: &Path, depth: usize, exp: &HashSet<PathBuf>, out: &mut Vec<TreeRow>, limit: usize) {
        for (p, d) in children(dir) {
            if out.len() >= limit { return; }
            let open = d && exp.contains(&p);
            out.push(TreeRow { path: p.clone(), depth, dir: d, open });
            if open && depth < 32 { walk(&p, depth + 1, exp, out, limit); }
        }
    }
    let mut out = Vec::new();
    walk(root, 0, expanded, &mut out, limit);
    out
}

// ------------------------------------------------------------------ function list

#[derive(Clone, Debug, PartialEq)]
pub struct Symbol { pub name: String, pub line: usize, pub depth: usize }

fn patterns(lang: &str) -> &'static [&'static str] {
    match lang {
        "rust" => &[r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+|async\s+|unsafe\s+|extern\s+\S+\s+)*fn\s+([A-Za-z_]\w*)",
                    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum|trait|union|mod)\s+([A-Za-z_]\w*)",
                    r"^\s*impl(?:<[^>]*>)?\s+([^{]+?)\s*(?:where\b.*)?\{"],
        "python" => &[r"^\s*(?:async\s+)?def\s+([A-Za-z_]\w*)", r"^\s*class\s+([A-Za-z_]\w*)"],
        "javascript" => &[r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s*\*?\s*([A-Za-z_$][\w$]*)",
                          r"^\s*(?:export\s+)?(?:default\s+)?class\s+([A-Za-z_$][\w$]*)",
                          r"^\s*(?:export\s+)?(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*(?:async\s*)?(?:\([^)]*\)|[A-Za-z_$][\w$]*)\s*=>"],
        "c" | "cpp" | "cs" | "java" => &[r"^\s*(?:(?:public|private|protected|internal|static|virtual|override|inline|extern|final|abstract|async|unsafe|const|constexpr)\s+)*[A-Za-z_][\w:<>,\*&\[\]\s]*?[\s\*&]([A-Za-z_~][\w:~]*)\s*\([^;{]*\)\s*(?:const\s*)?(?:noexcept\s*)?(?:override\s*)?(?:throws\s[^{]*)?\{?\s*$",
                                         r"^\s*(?:(?:public|private|protected|internal|static|abstract|sealed|final|partial)\s+)*(?:class|struct|interface|enum|namespace)\s+([A-Za-z_]\w*)"],
        "shell" => &[r"^\s*(?:function\s+)?([A-Za-z_][\w-]*)\s*\(\)\s*\{?", r"^\s*function\s+([A-Za-z_][\w-]*)"],
        "batch" => &[r"^:([A-Za-z_]\w*)"],
        "markdown" => &[r"^(#{1,6})\s+(.+?)\s*#*\s*$"],
        "ini" | "toml" => &[r"^\s*\[+([^\]]+)\]+"],
        "sql" => &[r"(?i)^\s*create\s+(?:or\s+replace\s+)?(?:function|procedure|table|view)\s+([\w\.\x22]+)"],
        "css" => &[r"^\s*([^\s@{}][^{}]*?)\s*\{"],
        "slint" => &[r"^\s*(?:export\s+)?(?:component|struct|global|enum)\s+([A-Za-z_][\w-]*)", r"^\s*(?:pure\s+)?(?:public\s+)?(?:function|callback)\s+([A-Za-z_][\w-]*)"],
        "yaml" => &[r"^([A-Za-z_][\w-]*):"],
        _ => &[],
    }
}

const KEYWORDS: &[&str] = &["if", "for", "while", "switch", "return", "else", "catch", "sizeof", "new", "delete", "do"];

/// Symbols of a document. `text` uses `\n` line ends.
pub fn symbols(lang: &str, text: &str) -> Vec<Symbol> {
    let res: Vec<regex::Regex> = patterns(lang).iter().filter_map(|p| regex::Regex::new(p).ok()).collect();
    if res.is_empty() { return vec![]; }
    let mut out = Vec::new();
    let mut in_fence = false;
    for (i, line) in text.split('\n').enumerate() {
        if line.len() > 2000 { continue; }
        if lang == "markdown" {
            if line.trim_start().starts_with("```") { in_fence = !in_fence; continue; }
            if in_fence { continue; }
            if let Some(c) = res[0].captures(line) {
                out.push(Symbol { name: c[2].to_string(), line: i, depth: c[1].len() - 1 });
            }
            continue;
        }
        for re in &res {
            if let Some(c) = re.captures(line) {
                let name = c.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_default();
                if name.is_empty() || (matches!(lang, "c" | "cpp" | "cs" | "java" | "javascript") && KEYWORDS.contains(&name.as_str())) { continue; }
                let indent = line.len() - line.trim_start().len();
                out.push(Symbol { name, line: i, depth: (indent / 4).min(6) });
                break;
            }
        }
    }
    out
}

// ------------------------------------------------------------------ compare

/// Per-line marks for the two sides: 1 = added (right only), 2 = removed (left only), 3 = changed.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct DiffMarks { pub left: HashMap<usize, u8>, pub right: HashMap<usize, u8>, pub hunks_left: Vec<usize>, pub hunks_right: Vec<usize>, pub stats: (usize, usize, usize) }

pub fn diff(a: &str, b: &str) -> DiffMarks {
    use similar::{DiffOp, TextDiff};
    let d = TextDiff::configure().algorithm(similar::Algorithm::Patience).diff_lines(a, b);
    let mut m = DiffMarks::default();
    for op in d.ops() {
        match *op {
            DiffOp::Equal { .. } => {}
            DiffOp::Delete { old_index, old_len, new_index } => {
                for l in old_index..old_index + old_len { m.left.insert(l, 2); }
                m.hunks_left.push(old_index); m.hunks_right.push(new_index); m.stats.1 += old_len;
            }
            DiffOp::Insert { old_index, new_index, new_len } => {
                for l in new_index..new_index + new_len { m.right.insert(l, 1); }
                m.hunks_left.push(old_index); m.hunks_right.push(new_index); m.stats.0 += new_len;
            }
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                for l in old_index..old_index + old_len { m.left.insert(l, 3); }
                for l in new_index..new_index + new_len { m.right.insert(l, 3); }
                m.hunks_left.push(old_index); m.hunks_right.push(new_index); m.stats.2 += old_len.max(new_len);
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rust_and_python_symbols() {
        let s = symbols("rust", "pub struct A;\nimpl A {\n    pub fn new() -> Self {}\n    async fn go(&self) {}\n}\nfn main() {}\n");
        let n: Vec<_> = s.iter().map(|x| (x.name.as_str(), x.line)).collect();
        assert_eq!(n, vec![("A", 0), ("A", 1), ("new", 2), ("go", 3), ("main", 5)]);
        let p = symbols("python", "class K:\n    def f(self):\n        pass\nasync def g():\n  if x(1):\n");
        assert_eq!(p.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), vec!["K", "f", "g"]);
    }
    #[test]
    fn c_and_markdown_symbols() {
        let s = symbols("c", "static int add(int a, int b) {\n  if (a) return 1;\n}\nvoid Foo::bar() const\n{\n");
        assert_eq!(s.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), vec!["add", "Foo::bar"]);
        let m = symbols("markdown", "# T\n```\n# not\n```\n## Sub ##\n");
        assert_eq!(m.iter().map(|x| (x.name.as_str(), x.depth)).collect::<Vec<_>>(), vec![("T", 0), ("Sub", 1)]);
    }
    #[test]
    fn diff_marks() {
        let m = diff("a\nb\nc\nd\n", "a\nB\nc\nd\ne\n");
        assert_eq!(m.left.get(&1), Some(&3));
        assert_eq!(m.right.get(&1), Some(&3));
        assert_eq!(m.right.get(&4), Some(&1));
        assert_eq!(m.hunks_right, vec![1, 4]);
        let m = diff("x\ny\n", "y\n");
        assert_eq!(m.left.get(&0), Some(&2));
        assert_eq!(m.stats, (0, 1, 0));
    }
    #[test]
    fn tree_sorted_dirs_first() {
        let d = std::env::temp_dir().join(format!("nt-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("zdir/inner")).unwrap();
        std::fs::create_dir_all(d.join("target")).unwrap();
        std::fs::write(d.join("a.txt"), "").unwrap();
        std::fs::write(d.join("zdir/b.rs"), "").unwrap();
        let rows = flatten(&d, &HashSet::new(), 100);
        assert_eq!(rows.iter().map(|r| r.path.file_name().unwrap().to_string_lossy().to_string()).collect::<Vec<_>>(), vec!["zdir", "a.txt"]);
        let rows = flatten(&d, &[d.join("zdir")].into_iter().collect(), 100);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[1].depth, 1);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
