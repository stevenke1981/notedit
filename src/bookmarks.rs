//! Batch line operations in character offsets, matching the editor buffer.
use std::collections::BTreeSet;
use ropey::Rope;

pub fn invert(rope: &Rope, marks: &BTreeSet<usize>) -> BTreeSet<usize> {
    (0..rope.len_lines()).filter(|line| !marks.contains(line)).collect()
}

/// Copy complete lines in document order, including their existing terminators.
/// An unterminated final line gets a separator for convenient line-wise paste.
pub fn collect(rope: &Rope, marks: &BTreeSet<usize>) -> String {
    let mut text = String::new();
    for &line in marks.range(..rope.len_lines()) {
        let part = rope.line(line).to_string();
        text.push_str(&part);
        if !part.ends_with('\n') { text.push('\n'); }
    }
    text
}

/// Merge adjacent ranges, including the separator before an unterminated last
/// line, so deleting the last line does not leave an unintended blank line.
pub fn deletions(rope: &Rope, marks: &BTreeSet<usize>) -> Vec<(usize, usize, String)> {
    let mut ranges: Vec<(usize, usize, String)> = Vec::new();
    for &line in marks.range(..rope.len_lines()) {
        let start = rope.line_to_char(line);
        let end = start + rope.line(line).len_chars();
        if let Some(last) = ranges.last_mut().filter(|last| last.1 >= start) {
            last.1 = end;
        } else if start < end || start > 0 {
            ranges.push((start, end, String::new()));
        }
    }
    if let Some(last) = ranges.last_mut() {
        let final_blank_marked = rope.line(rope.len_lines() - 1).len_chars() == 0
            && marks.contains(&(rope.len_lines() - 1));
        if last.1 == rope.len_chars() && last.0 > 0
            && (final_blank_marked || rope.char(last.1 - 1) != '\n') {
            last.0 -= 1;
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{Buffer, EditKind, Sel};

    #[test]
    fn copy_unicode_blank_and_final_lines() {
        let rope = Rope::from_str("甲\n\n乙");
        assert_eq!(collect(&rope, &BTreeSet::from([0, 1, 2, 99])), "甲\n\n乙\n");
        assert_eq!(collect(&rope, &BTreeSet::new()), "");
        assert_eq!(invert(&rope, &BTreeSet::from([1, 99])), BTreeSet::from([0, 2]));
    }

    #[test]
    fn every_subset_preserves_unmarked_lines() {
        for text in ["", "甲", "甲\n", "\n\n", "甲\n\n乙\n", "甲\n乙\n丙"] {
            let lines: Vec<_> = text.split('\n').collect();
            for mask in 0..(1 << lines.len()) {
                let marks: BTreeSet<_> = (0..lines.len()).filter(|i| mask & (1 << i) != 0).collect();
                let expected = lines.iter().enumerate().filter(|(i, _)| !marks.contains(i))
                    .map(|(_, line)| *line).collect::<Vec<_>>().join("\n");
                let mut rope = Rope::from_str(text);
                crate::editor::apply(&mut rope, &deletions(&Rope::from_str(text), &marks));
                assert_eq!(rope.to_string(), expected, "text={text:?}, marks={marks:?}");
            }
        }
    }

    #[test]
    fn delete_lines_and_undo_as_one_transaction() {
        for (text, marks, expected) in [
            ("甲\n乙\n丙\n丁", vec![0, 2], "乙\n丁"),
            ("甲\n乙\n丙", vec![1, 2], "甲"),
            ("甲\n乙\n", vec![1], "甲\n"),
            ("甲\n", vec![1], "甲"),
            ("甲\n乙", vec![0, 1], ""),
            ("", vec![0], ""),
        ] {
            let mut buffer = Buffer::from_str(text);
            let mut sels = vec![Sel::caret(0)];
            let edits = deletions(&buffer.rope, &marks.into_iter().collect());
            buffer.edit(&sels, edits, EditKind::Other);
            assert_eq!(buffer.text(), expected);
            if text != expected {
                assert!(buffer.undo(&mut sels));
                assert_eq!(buffer.text(), text);
                assert!(!buffer.can_undo());
                assert!(buffer.redo(&mut sels));
                assert_eq!(buffer.text(), expected);
            } else { assert!(!buffer.can_undo()); }
        }
    }
}
