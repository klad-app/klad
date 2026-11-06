use super::Editor;
use crate::commands::Cmd;
use crate::messages::Tone;
use crate::theme::{self, Hint, icon};
use egui::RichText;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct FirstSteps {
    pub open: bool,
    pub drew: bool,
    pub filled: bool,
    pub handed: bool,
}

pub(super) fn prompt(path: &Path) -> String {
    format!(
        "I am drawing in klad; my drawing is {path}. Run `klad skill` and read it first. \
         Then look at the drawing with `klad render \"{path}\"` and add a soft shadow under the shape I drew, on a new layer below it. \
         I have the drawing open in the klad editor, so I see your edits as they land.",
        path = path.display()
    )
}

fn step(ui: &mut egui::Ui, done: bool, number: &str, text: &str) {
    ui.horizontal_top(|ui| {
        let (mark, colour) = match done {
            true => (icon::CHECK, theme::DONE),
            false => (number, theme::TEXT),
        };
        ui.label(RichText::new(mark).strong().color(colour));
        ui.add(
            egui::Label::new(RichText::new(text).color(match done {
                true => theme::WEAK,
                false => theme::TEXT,
            }))
            .wrap(),
        );
    });
}

impl Editor {
    pub(crate) fn start_first_steps(&mut self) {
        self.first = FirstSteps {
            open: true,
            ..FirstSteps::default()
        };
    }

    pub(super) fn first_steps_window(&mut self, ctx: &egui::Context) {
        if !self.first.open {
            return;
        }
        let mut open = true;
        let corner = self.canvas_rect.right_top() + egui::vec2(-384.0, 12.0);
        Self::window(
            ctx,
            "first steps",
            (format!("{} First steps", icon::INFO), corner),
            &mut open,
            |ui| {
                ui.set_width(340.0);
                step(
                    ui,
                    self.first.drew,
                    "1",
                    "Draw a closed shape with the brush (B): a stone, a leaf, a cloud. A pen draws with its pressure; a mouse works too.",
                );
                step(
                    ui,
                    self.first.filled,
                    "2",
                    "Fill it: pick the fill tool (G) and click inside the shape.",
                );
                step(
                    ui,
                    self.first.handed,
                    "3",
                    "Hand it to an agent that works in a terminal (Claude Code, Codex and others): install the klad command, then copy a first request and paste it into your agent. Its edits show up here as they land.",
                );
                ui.horizontal(|ui| {
                    self.command_button(ui, Cmd::InstallCommand, Some("install the klad command"));
                    if ui
                        .button(format!("{} copy a request", icon::COPY))
                        .hint("a first request for your agent, with this drawing's path in it")
                        .clicked()
                    {
                        ui.ctx().copy_text(prompt(&self.scene.bundle.root));
                        self.first.handed = true;
                        self.say(Tone::Done, "copied a request: paste it into your agent");
                    }
                });
                if self.first.drew && self.first.filled && self.first.handed {
                    ui.add_space(4.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                "Done. File > Close the drawing shows the samples to look at; Help > Every key lists the keys.",
                            )
                            .color(theme::DONE),
                        )
                        .wrap(),
                    );
                }
            },
        );
        if !open {
            self.first.open = false;
        }
    }
}
