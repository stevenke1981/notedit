//! Markdown / HTML -> simple block model for the preview pane.
use crate::Block;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

fn blk(text: String, kind: i32, indent: i32) -> Block {
    Block { text: text.into(), kind, indent, bold: false, italic: false, code: false, link: false }
}

/// kind: 0 paragraph, 1-6 heading, 7 list item, 10 code block, 11 rule, 12 quote
pub fn markdown(src: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut kind = 0;
    let mut list_depth: i32 = 0;
    let mut ordered: Vec<Option<u64>> = Vec::new();
    let mut quote = 0;
    let (mut bold, mut italic, mut link, mut all_code) = (false, false, false, true);
    let flush = |out: &mut Vec<Block>, cur: &mut String, kind: i32, indent: i32, b: bool, i: bool, l: bool, c: bool| {
        let t = cur.trim_end().to_string();
        if !t.is_empty() {
            let mut bl = blk(t, kind, indent);
            bl.bold = b; bl.italic = i; bl.link = l; bl.code = c && kind != 10;
            out.push(bl);
        }
        cur.clear();
    };
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for ev in Parser::new_ext(src, opts) {
        match ev {
            Event::Start(Tag::Heading { level, .. }) => {
                kind = match level { HeadingLevel::H1 => 1, HeadingLevel::H2 => 2, HeadingLevel::H3 => 3, HeadingLevel::H4 => 4, HeadingLevel::H5 => 5, HeadingLevel::H6 => 6 };
            }
            Event::Start(Tag::Paragraph) => { if kind != 7 { kind = if quote > 0 { 12 } else { 0 }; } bold = false; italic = false; link = false; all_code = true; }
            Event::Start(Tag::CodeBlock(_)) => { kind = 10; }
            Event::Start(Tag::BlockQuote(_)) => { quote += 1; }
            Event::End(TagEnd::BlockQuote(_)) => { quote -= 1; }
            Event::Start(Tag::List(start)) => {
                if !cur.trim().is_empty() { flush(&mut out, &mut cur, kind, list_depth - 1, bold, italic, link, false); }
                list_depth += 1; ordered.push(start);
            }
            Event::End(TagEnd::List(_)) => { list_depth -= 1; ordered.pop(); kind = 0; }
            Event::Start(Tag::Item) => { bold = false; italic = false; link = false;
                kind = 7;
                let marker = match ordered.last_mut() { Some(Some(n)) => { let s = format!("{}. ", n); *n += 1; s } _ => "• ".to_string() };
                cur.push_str(&marker);
            }
            Event::End(TagEnd::Item) => { flush(&mut out, &mut cur, 7, list_depth - 1, bold, italic, link, false); }
            Event::Start(Tag::Strong) => bold = true,
            Event::Start(Tag::Emphasis) => italic = true,
            Event::Start(Tag::Link { .. }) => link = true,
            Event::End(TagEnd::Heading(_)) | Event::End(TagEnd::Paragraph) | Event::End(TagEnd::CodeBlock) => {
                let k = kind;
                let c = all_code && k == 0;
                flush(&mut out, &mut cur, k, list_depth.max(0) - if k == 7 { 1 } else { 0 }, bold, italic, link, c);
                if k != 7 { kind = 0; }
            }
            Event::Start(Tag::TableRow) | Event::Start(Tag::TableHead) => {}
            Event::End(TagEnd::TableRow) | Event::End(TagEnd::TableHead) => { flush(&mut out, &mut cur, 10, 0, false, false, false, false); }
            Event::End(TagEnd::TableCell) => cur.push_str(" │ "),
            Event::Text(t) => { all_code = false; cur.push_str(&t); }
            Event::Code(t) => { cur.push('`'); cur.push_str(&t); cur.push('`'); }
            Event::TaskListMarker(done) => cur.push_str(if done { "☑ " } else { "☐ " }),
            Event::SoftBreak => cur.push(' '),
            Event::HardBreak => cur.push('\n'),
            Event::Rule => out.push(blk(String::new(), 11, 0)),
            Event::Html(h) | Event::InlineHtml(h) => cur.push_str(&strip_tags(&h)),
            _ => {}
        }
    }
    flush(&mut out, &mut cur, 0, 0, false, false, false, false);
    out
}

fn strip_tags(s: &str) -> String {
    let mut o = String::new();
    let mut intag = false;
    for c in s.chars() {
        match c { '<' => intag = true, '>' => intag = false, _ if !intag => o.push(c), _ => {} }
    }
    o
}

fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"")
        .replace("&#39;", "'").replace("&copy;", "©").replace("&amp;", "&")
}

/// Very small HTML renderer: headings, paragraphs, lists, links, bold/italic, code, pre, hr, blockquote.
pub fn html(src: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut kind = 0;
    let mut depth: i32 = 0;
    let mut ol: Vec<Option<u32>> = Vec::new();
    let (mut bold, mut italic, mut link) = (false, false, false);
    let mut pre = false;
    let mut skip = 0; // inside <script>/<style>/<head>
    let mut title = String::new();
    let mut in_title = false;
    let push = |out: &mut Vec<Block>, cur: &mut String, kind: i32, indent: i32, b: bool, i: bool, l: bool| {
        let t = if kind == 10 { cur.trim_matches('\n').to_string() } else { cur.split_whitespace().collect::<Vec<_>>().join(" ") };
        if !t.is_empty() {
            let mut bl = blk(decode_entities(&t), kind, indent.max(0));
            bl.bold = b; bl.italic = i; bl.link = l;
            out.push(bl);
        }
        cur.clear();
    };
    let mut rest = src;
    while !rest.is_empty() {
        if let Some(lt) = rest.find('<') {
            let text = &rest[..lt];
            if in_title { title.push_str(text); }
            else if skip == 0 { cur.push_str(text); }
            let Some(gt) = rest[lt..].find('>') else { break };
            let tag_full = &rest[lt + 1..lt + gt];
            rest = &rest[lt + gt + 1..];
            if tag_full.starts_with("!--") {
                if let Some(e) = rest.find("-->") { rest = &rest[e + 3..]; }
                continue;
            }
            let closing = tag_full.starts_with('/');
            let name: String = tag_full.trim_start_matches('/').chars().take_while(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
            match (name.as_str(), closing) {
                ("script" | "style", false) => skip += 1,
                ("script" | "style", true) => skip = (skip - 1).max(0),
                ("title", c) => in_title = !c,
                ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", false) => { push(&mut out, &mut cur, kind, depth, bold, italic, link); kind = name[1..].parse().unwrap_or(1); }
                ("p" | "div" | "section" | "article" | "header" | "footer" | "tr" | "table", _) | ("br", _) => {
                    if name == "br" && !pre { cur.push('\n'); push(&mut out, &mut cur, kind, depth, bold, italic, link); }
                    else { push(&mut out, &mut cur, kind, if kind == 7 { depth - 1 } else { 0 }, bold, italic, link); if !closing { bold = false; italic = false; link = false; } }
                }
                ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", true) => { push(&mut out, &mut cur, kind, 0, bold, italic, link); kind = 0; }
                ("ul" | "ol", false) => { push(&mut out, &mut cur, kind, depth - 1, bold, italic, link); depth += 1; ol.push(if name == "ol" { Some(1) } else { None }); }
                ("ul" | "ol", true) => { push(&mut out, &mut cur, kind, depth - 1, bold, italic, link); depth -= 1; ol.pop(); kind = 0; }
                ("li", false) => {
                    push(&mut out, &mut cur, kind, depth - 1, bold, italic, link);
                    kind = 7;
                    match ol.last_mut() { Some(Some(n)) => { cur.push_str(&format!("{}. ", n)); *n += 1; } _ => cur.push_str("• ") }
                }
                ("li", true) => push(&mut out, &mut cur, 7, depth - 1, bold, italic, link),
                ("pre", false) => { push(&mut out, &mut cur, kind, 0, bold, italic, link); pre = true; kind = 10; }
                ("pre", true) => { push(&mut out, &mut cur, 10, 0, false, false, false); pre = false; kind = 0; }
                ("blockquote", false) => { push(&mut out, &mut cur, kind, 0, bold, italic, link); kind = 12; }
                ("blockquote", true) => { push(&mut out, &mut cur, 12, 0, bold, italic, link); kind = 0; }
                ("hr", _) => { push(&mut out, &mut cur, kind, 0, bold, italic, link); out.push(blk(String::new(), 11, 0)); }
                ("b" | "strong", c) => bold = !c,
                ("i" | "em", c) => italic = !c,
                ("a", c) => link = !c,
                ("td" | "th", true) => cur.push_str(" │ "),
                ("img", _) => cur.push_str("[圖片]"),
                ("code", _) if !pre => cur.push('`'),
                _ => {}
            }
        } else {
            if skip == 0 { cur.push_str(rest); }
            break;
        }
    }
    push(&mut out, &mut cur, kind, 0, bold, italic, link);
    if !title.trim().is_empty() {
        out.insert(0, blk(format!("《{}》", decode_entities(title.trim())), 3, 0));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn md_basic() {
        let b = markdown("# Title\n\nHello **world**\n\n- a\n- b\n\n```\ncode\n```\n");
        assert_eq!(b[0].kind, 1);
        assert_eq!(b[0].text.as_str(), "Title");
        assert!(b.iter().any(|x| x.kind == 7 && x.text.as_str() == "• a"));
        assert!(b.iter().any(|x| x.kind == 10 && x.text.as_str() == "code"));
    }
    #[test]
    fn html_basic() {
        let b = html("<html><head><title>T</title><style>x{}</style></head><body><h1>Hi</h1><p>Some <b>bold</b> text</p><ol><li>one</li><li>two</li></ol></body></html>");
        assert!(b.iter().any(|x| x.kind == 1 && x.text.as_str() == "Hi"));
        assert!(b.iter().any(|x| x.text.as_str() == "2. two"));
        assert!(!b.iter().any(|x| x.text.contains("x{}")));
    }
}
