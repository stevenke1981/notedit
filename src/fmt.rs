//! JSON / XML pretty-print and minify. Key order and text are preserved; invalid
//! input yields a message containing the line (and column) of the problem.

fn line_col(s: &str, byte: usize) -> (usize, usize) {
    let pre = &s[..byte.min(s.len())];
    (pre.matches('\n').count() + 1, pre.rsplit('\n').next().map(|l| l.chars().count()).unwrap_or(0) + 1)
}

/// Pretty-print (`indent = Some(n)`) or minify (`None`) JSON.
pub fn json(s: &str, indent: Option<usize>) -> Result<String, String> {
    if let Err(e) = serde_json::from_str::<serde::de::IgnoredAny>(s) {
        return Err(format!("JSON 格式錯誤：第 {} 行，第 {} 欄：{}", e.line(), e.column(), e.to_string().split(" at line").next().unwrap_or("")));
    }
    let pad = |n: usize| " ".repeat(n * indent.unwrap_or(0));
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    let mut chars = s.chars().peekable();
    let nl = |out: &mut String, d: usize| if indent.is_some() { out.push('\n'); out.push_str(&pad(d)); };
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push('"');
                while let Some(c) = chars.next() {
                    out.push(c);
                    if c == '\\' { if let Some(n) = chars.next() { out.push(n); } } else if c == '"' { break; }
                }
            }
            '{' | '[' => {
                let close = if c == '{' { '}' } else { ']' };
                while chars.peek().is_some_and(|c| c.is_whitespace()) { chars.next(); }
                if chars.peek() == Some(&close) { chars.next(); out.push(c); out.push(close); continue; }
                depth += 1; out.push(c); nl(&mut out, depth);
            }
            '}' | ']' => { depth = depth.saturating_sub(1); nl(&mut out, depth); out.push(c); }
            ',' => { out.push(','); nl(&mut out, depth); }
            ':' => { out.push(':'); if indent.is_some() { out.push(' '); } }
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    if indent.is_some() && s.ends_with('\n') { out.push('\n'); }
    Ok(out)
}

enum Tok<'a> { Open(&'a str), Close(&'a str), Empty(&'a str), Other(&'a str), Text(&'a str) }

fn xml_tokens(s: &str) -> Result<Vec<(usize, Tok<'_>)>, String> {
    let err = |at: usize, m: &str| { let (l, c) = line_col(s, at); format!("XML 格式錯誤：第 {l} 行，第 {c} 欄：{m}") };
    let mut toks = vec![];
    let mut stack: Vec<(&str, usize)> = vec![];
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        if !rest.starts_with('<') {
            let end = rest.find('<').map(|n| i + n).unwrap_or(s.len());
            toks.push((i, Tok::Text(&s[i..end])));
            i = end; continue;
        }
        let (term, other) = if rest.starts_with("<!--") { ("-->", true) } else if rest.starts_with("<![CDATA[") { ("]]>", true) }
            else if rest.starts_with("<?") { ("?>", true) } else if rest.starts_with("<!") { (">", true) } else { (">", false) };
        let Some(n) = rest.find(term) else { return Err(err(i, "標籤未結束")) };
        let end = i + n + term.len();
        let raw = &s[i..end];
        if other { toks.push((i, Tok::Other(raw))); i = end; continue; }
        let inner = &raw[1..raw.len() - 1];
        if let Some(name) = inner.strip_prefix('/') {
            let name = name.trim();
            match stack.pop() {
                Some((o, _)) if o == name => {}
                Some((o, at)) => { let (l, _) = line_col(s, at); return Err(err(i, &format!("</{name}> 與第 {l} 行的 <{o}> 不對應"))); }
                None => return Err(err(i, &format!("多餘的結束標籤 </{name}>"))),
            }
            toks.push((i, Tok::Close(raw)));
        } else {
            let name = inner.split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
            if name.is_empty() { return Err(err(i, "缺少標籤名稱")); }
            if inner.ends_with('/') { toks.push((i, Tok::Empty(raw))); } else { stack.push((name, i)); toks.push((i, Tok::Open(raw))); }
        }
        i = end;
    }
    if let Some((o, at)) = stack.pop() { return Err(err(at, &format!("<{o}> 沒有結束標籤"))); }
    Ok(toks)
}

/// Pretty-print (`indent = Some(n)`) or minify (`None`) XML.
pub fn xml(s: &str, indent: Option<usize>) -> Result<String, String> {
    let toks = xml_tokens(s)?;
    let mut out = String::with_capacity(s.len());
    let pad = |d: usize| " ".repeat(d * indent.unwrap_or(0));
    let mut depth = 0usize;
    let mut k = 0;
    let line = |out: &mut String, d: usize, t: &str| { if indent.is_some() && !out.is_empty() { out.push('\n'); out.push_str(&pad(d)); } out.push_str(t); };
    while k < toks.len() {
        match &toks[k].1 {
            Tok::Open(raw) => {
                // <a>text</a> stays on one line
                if let (Some((_, Tok::Text(t))), Some((_, Tok::Close(c)))) = (toks.get(k + 1), toks.get(k + 2)) {
                    line(&mut out, depth, raw); out.push_str(t.trim()); out.push_str(c); k += 3; continue;
                }
                if let Some((_, Tok::Close(c))) = toks.get(k + 1) { line(&mut out, depth, raw); out.push_str(c); k += 2; continue; }
                line(&mut out, depth, raw); depth += 1;
            }
            Tok::Close(raw) => { depth = depth.saturating_sub(1); line(&mut out, depth, raw); }
            Tok::Empty(raw) | Tok::Other(raw) => line(&mut out, depth, raw),
            Tok::Text(t) => { let t = t.trim(); if !t.is_empty() { line(&mut out, depth, t); } }
        }
        k += 1;
    }
    if indent.is_some() && s.ends_with('\n') { out.push('\n'); }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_roundtrip() {
        let src = r#"{"b":1,"a":[1,2,{"x":"q\"}"}],"e":{}}"#;
        let p = json(src, Some(2)).unwrap();
        assert_eq!(p, "{\n  \"b\": 1,\n  \"a\": [\n    1,\n    2,\n    {\n      \"x\": \"q\\\"}\"\n    }\n  ],\n  \"e\": {}\n}");
        assert_eq!(json(&p, None).unwrap(), src);
        assert!(json("{\n\"a\": 1,\n}", None).unwrap_err().contains("第 3 行"));
    }
    #[test]
    fn xml_roundtrip() {
        let src = "<?xml version=\"1.0\"?><r a=\"1\"><b>hi</b><c/><d><e></e></d><!-- x --></r>";
        let p = xml(src, Some(2)).unwrap();
        assert_eq!(p, "<?xml version=\"1.0\"?>\n<r a=\"1\">\n  <b>hi</b>\n  <c/>\n  <d>\n    <e></e>\n  </d>\n  <!-- x -->\n</r>");
        assert_eq!(xml(&p, None).unwrap(), src);
        let e = xml("<a>\n<b>\n</a>", None).unwrap_err();
        assert!(e.contains("第 3 行") && e.contains("第 2 行"), "{e}");
    }
}
