//! Retain Slint model identities so caret moves do not rebuild text items.
use slint::{Model, ModelRc, VecModel};

pub fn update<T: Clone + PartialEq + 'static>(previous: ModelRc<T>, rows: Vec<T>) -> ModelRc<T> {
    if let Some(model) = previous.as_any().downcast_ref::<VecModel<T>>() {
        if model.row_count() == rows.len() {
            for (i, row) in rows.into_iter().enumerate() {
                if model.row_data(i).as_ref() != Some(&row) { model.set_row_data(i, row); }
            }
        } else {
            model.set_vec(rows);
        }
        previous
    } else {
        ModelRc::new(VecModel::from(rows))
    }
}

pub fn update_rows(previous: ModelRc<crate::EdRow>, mut rows: Vec<crate::EdRow>) -> ModelRc<crate::EdRow> {
    for (i, row) in rows.iter_mut().enumerate() {
        if let Some(old) = previous.row_data(i) {
            row.toks = update(old.toks, row.toks.iter().collect());
            row.rects = update(old.rects, row.rects.iter().collect());
        }
    }
    update(previous, rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EdRect, EdRow, EdTok};

    #[test]
    fn caret_and_selection_changes_retain_text_models() {
        let make_row = |cur| EdRow {
            cur,
            toks: ModelRc::new(VecModel::from(vec![EdTok { text: "繁體中文 fn main()".into(), ..Default::default() }])),
            ..Default::default()
        };
        let previous = update_rows(ModelRc::default(), vec![make_row(true), make_row(false)]);
        let tokens = previous.row_data(0).unwrap().toks;
        let mut next = vec![make_row(false), make_row(true)];
        next[0].rects = ModelRc::new(VecModel::from(vec![EdRect { x: 8.0, w: 16.0, kind: 0 }]));
        let updated = update_rows(previous.clone(), next);
        assert_eq!(previous, updated);
        assert_eq!(tokens, updated.row_data(0).unwrap().toks);
        assert!(!updated.row_data(0).unwrap().cur);
        assert!(updated.row_data(1).unwrap().cur);
        assert_eq!(updated.row_data(0).unwrap().rects.row_count(), 1);
        assert_eq!(tokens.row_data(0).unwrap().text, "繁體中文 fn main()");
    }

    #[test]
    fn edits_scroll_and_viewport_resize_update_contents() {
        let row = |text: &str| EdRow {
            toks: ModelRc::new(VecModel::from(vec![EdTok { text: text.into(), ..Default::default() }])),
            ..Default::default()
        };
        let model = update_rows(ModelRc::default(), vec![row("first"), row("second")]);
        let scrolled = update_rows(model.clone(), vec![row("second"), row("edited")]);
        assert_eq!(model, scrolled);
        assert_eq!(scrolled.row_data(0).unwrap().toks.row_data(0).unwrap().text, "second");
        assert_eq!(scrolled.row_data(1).unwrap().toks.row_data(0).unwrap().text, "edited");
        let resized = update_rows(scrolled, vec![row("last")]);
        assert_eq!(resized.row_count(), 1);
        assert_eq!(resized.row_data(0).unwrap().toks.row_data(0).unwrap().text, "last");
        assert_eq!(update_rows(resized, vec![]).row_count(), 0);
    }
}
