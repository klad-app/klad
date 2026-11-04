use super::Editor;
use crate::commands::Cmd;
use crate::messages::Tone;
use crate::theme::{self, Hint};
use anyhow::Result;
use egui::{Color32, RichText};
use klad_core::bundle::{Bundle, now_ms};
use klad_core::history::{self, Stack};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub seq: u64,
    pub who: String,
    pub what: String,
    pub at_ms: u64,
    pub undone: bool,
}

pub(super) fn who(author: &str) -> String {
    match author {
        "editor" => "you".into(),
        "cli" => "agent".into(),
        other => other.into(),
    }
}

pub(super) fn rows(bundle: &Bundle) -> Result<Vec<Row>> {
    let row = |e: history::Entry, undone: bool| Row {
        seq: e.seq,
        who: who(&e.author),
        what: e.summary,
        at_ms: e.at_ms,
        undone,
    };
    let mut redo: Vec<Row> = history::entries(bundle, Stack::Redo)?
        .into_iter()
        .map(|e| row(e, true))
        .collect();
    redo.reverse();
    Ok(redo
        .into_iter()
        .chain(
            history::entries(bundle, Stack::Undo)?
                .into_iter()
                .map(|e| row(e, false)),
        )
        .collect())
}

pub(super) fn steps_to(rows: &[Row], clicked: usize) -> Option<(Stack, usize)> {
    let undone = rows.iter().filter(|r| r.undone).count();
    match clicked.checked_sub(undone) {
        Some(0) => None,
        Some(below) if clicked < rows.len() => Some((Stack::Undo, below)),
        Some(_) => None,
        None => Some((Stack::Redo, undone - clicked)),
    }
}

pub(super) fn ago(at_ms: u64, now: u64) -> String {
    let s = now.saturating_sub(at_ms) / 1000;
    match s {
        0..=59 => format!("{s} s ago"),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86_399 => format!("{} h ago", s / 3600),
        _ => format!("{} days ago", s / 86_400),
    }
}

impl Editor {
    pub(super) fn history_rows(&mut self) -> Vec<Row> {
        let rev = self.scene.bundle.doc.rev;
        match &self.history {
            Some((r, rows)) if *r == rev => rows.clone(),
            _ => {
                let rows = rows(&self.scene.bundle).unwrap_or_default();
                self.history = Some((rev, rows.clone()));
                rows
            }
        }
    }

    pub(super) fn step_to(&mut self, stack: Stack, count: usize) -> Result<()> {
        let start = Instant::now();
        (0..count).try_for_each(|_| self.step(stack))?;
        self.history = None;
        let word = if stack == Stack::Undo { "undid" } else { "redid" };
        let text = format!(
            "{word} {count} step{} in {:.0} ms",
            if count == 1 { "" } else { "s" },
            start.elapsed().as_secs_f64() * 1000.0
        );
        self.measure("undo", text.clone());
        self.say(Tone::Done, text);
        Ok(())
    }

    pub(super) fn history_pane(&mut self, ui: &mut egui::Ui) -> Result<()> {
        ui.horizontal_wrapped(|ui| {
            self.command_button(ui, Cmd::Undo, Some("undo"));
            self.command_button(ui, Cmd::Redo, Some("redo"));
        });
        let rows = self.history_rows();
        if rows.is_empty() {
            ui.label(RichText::new("nothing done yet").color(theme::WEAK));
            return Ok(());
        }
        let now = now_ms();
        let mut clicked = None;
        let undone = rows.iter().filter(|r| r.undone).count();
        rows.iter().enumerate().for_each(|(i, r)| {
            if i == undone {
                ui.label(RichText::new("the document is here").small().color(theme::ACCENT));
            }
            let colour = match (r.undone, r.who.as_str()) {
                (true, _) => theme::WEAK,
                (_, "agent") => Color32::from_rgb(150, 200, 255),
                _ => theme::TEXT,
            };
            let text = match r.undone {
                true => format!("undone  {}  {}  {}", r.who, r.what, ago(r.at_ms, now)),
                false => format!("{}  {}  {}", r.who, r.what, ago(r.at_ms, now)),
            };
            let hint = match steps_to(&rows, i) {
                Some((Stack::Undo, n)) => format!("undo {n} step{} to come back here", if n == 1 { "" } else { "s" }),
                Some((Stack::Redo, n)) => format!("redo {n} step{} to come back here", if n == 1 { "" } else { "s" }),
                None => "the latest step, where the document is".to_string(),
            };
            if ui
                .add(egui::Button::new(RichText::new(text).color(colour)).frame(false))
                .hint(hint)
                .clicked()
            {
                clicked = Some(i);
            }
        });
        if let Some((stack, n)) = clicked.and_then(|i| steps_to(&rows, i)) {
            self.step_to(stack, n)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(seq: u64, undone: bool) -> Row {
        Row {
            seq,
            who: "agent".into(),
            what: format!("step {seq}"),
            at_ms: 0,
            undone,
        }
    }

    #[test]
    fn a_click_on_a_step_undoes_or_redoes_back_to_it() {
        let rows = vec![row(9, true), row(8, true), row(7, false), row(6, false), row(5, false)];
        assert_eq!(steps_to(&rows, 0), Some((Stack::Redo, 2)));
        assert_eq!(steps_to(&rows, 1), Some((Stack::Redo, 1)));
        assert_eq!(steps_to(&rows, 2), None);
        assert_eq!(steps_to(&rows, 3), Some((Stack::Undo, 1)));
        assert_eq!(steps_to(&rows, 4), Some((Stack::Undo, 2)));
        assert_eq!(steps_to(&rows, 5), None);
        assert_eq!(who("editor"), "you");
        assert_eq!(who("cli"), "agent");
        assert_eq!(ago(1_000, 61_000), "1 min ago");
    }
}
