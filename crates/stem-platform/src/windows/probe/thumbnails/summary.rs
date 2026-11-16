//! The summary printed at the end.

use windows::Win32::Foundation::{RECT, SIZE};

use super::emit;
use super::external::own_elevation;
use super::facts::{short_state, windows_build};
use super::geometry::{height, width};
use super::plans::Source;
use super::probe::Probe;
use super::timing::{Call, timing_lines};

/// Left-aligned text columns separated by ` | `.
pub(super) fn table(header: &[&str], rows: &[Vec<String>]) -> Vec<String> {
    let mut widths: Vec<usize> = header.iter().map(|h| h.len()).collect();
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            if let Some(width) = widths.get_mut(index) {
                *width = (*width).max(cell.len());
            }
        }
    }
    let line = |cells: Vec<&str>| {
        cells
            .iter()
            .zip(&widths)
            .map(|(cell, &width)| format!("{cell:<width$}"))
            .collect::<Vec<_>>()
            .join(" | ")
            .trim_end()
            .to_owned()
    };
    let mut out = vec![line(header.to_vec())];
    out.push(
        widths
            .iter()
            .map(|width| "-".repeat(*width))
            .collect::<Vec<_>>()
            .join("-+-"),
    );
    for row in rows {
        out.push(line(row.iter().map(String::as_str).collect()));
    }
    out
}

impl Probe {
    pub(super) fn summary(&self) {
        emit("");
        emit("==== thumbnail probe summary: copy from here to the end ====");
        say!(
            "Windows build {}; probe window DPI {} ({:.0} %); probe process {}",
            windows_build(),
            self.dpi,
            self.scale * 100.0,
            own_elevation()
        );
        let header = [
            "#",
            "source",
            "origin",
            "state",
            "register",
            "us",
            "size (DWM)",
            "window",
            "frame",
            "crop",
            "update",
            "us",
        ];
        let size_text = |size: Option<SIZE>| {
            size.map_or_else(|| "-".to_owned(), |s| format!("{}x{}", s.cx, s.cy))
        };
        let rect_size = |rect: Option<RECT>| {
            rect.map_or_else(|| "-".to_owned(), |r| format!("{}x{}", width(r), height(r)))
        };
        let rows: Vec<Vec<String>> = self
            .slots
            .iter()
            .map(|slot| {
                let origin = match (slot.plan.source, slot.source) {
                    (_, None) => "not found",
                    (Source::Own(_), Some(_)) => "own",
                    (Source::Find(_), Some(_)) => "found",
                    (Source::SecondOfFirst, Some(_)) => "slot 1",
                };
                let state = slot.source.map_or_else(|| "-".to_owned(), short_state);
                let placement = slot.placement.clone().unwrap_or_default();
                let call = |call: &Option<Call>| {
                    call.as_ref().map_or_else(
                        || ("-".to_owned(), "-".to_owned()),
                        |c| (c.short(), format!("{:.0}", c.micros)),
                    )
                };
                let (register, register_us) = call(&placement.register);
                let (update, update_us) = call(&placement.update);
                let crop = if placement.crop.is_some() {
                    "yes"
                } else if placement.register.is_some() {
                    "no"
                } else {
                    "-"
                };
                vec![
                    slot.number.to_string(),
                    slot.plan.key.to_owned(),
                    origin.to_owned(),
                    state,
                    register,
                    register_us,
                    size_text(placement.size),
                    rect_size(placement.window_rect),
                    rect_size(placement.frame),
                    crop.to_owned(),
                    update,
                    update_us,
                ]
            })
            .collect();
        for line in table(&header, &rows) {
            emit(&line);
        }
        emit(
            "(register, size, window, frame, crop and update are from each slot's last \
             registration; state is read now)",
        );
        for slot in &self.slots {
            if slot.plan.source != Source::SecondOfFirst {
                say!("slot {}: {}", slot.number, slot.origin);
            }
        }
        for line in timing_lines(&self.calls) {
            emit(&line);
        }
        for line in &self.results {
            emit(line);
        }
        if self.notes.is_empty() {
            emit("notes: none typed");
        }
        for (phase, note) in &self.notes {
            say!("note at {phase}: {note}");
        }
        emit("==== end of summary ====");
    }
}
