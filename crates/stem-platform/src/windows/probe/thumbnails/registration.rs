//! Registering the slots' thumbnails, and updating them again.

use windows::Win32::Foundation::RECT;

use super::dwm::{register_into, unregister, update_again};
use super::emit;
use super::facts::facts;
use super::geometry::rect_text;
use super::plans::Source;
use super::probe::Probe;

impl Probe {
    /// Unregisters the slot's thumbnail, if any, and registers its source
    /// again; prints what happened.
    pub(super) fn place(&mut self, index: usize) {
        let clock = self.clock;
        let dest = self.dest();
        let Some(slot) = self.slots.get_mut(index) else {
            return;
        };
        let Some(source) = slot.source else {
            say!("[{}] {}: {}", slot.number, slot.plan.key, slot.origin);
            return;
        };
        if let Some(old) = slot.placement.as_mut().and_then(|p| p.thumb.take()) {
            let call = unregister(clock, old);
            say!("[{}] {}", slot.number, call.describe());
            self.calls.push(call);
        }
        let state = facts(source);
        let placement = register_into(clock, dest, source, slot.rects.image, slot.plan.crop);
        self.calls.extend(placement.calls());
        say!("[{}] {}", slot.number, slot.title());
        say!("    source {:#x}: {}", source.0 as usize, slot.origin);
        say!("    state now: {state}");
        for line in &slot.setup {
            say!("    setup: {line}");
        }
        for call in placement.calls() {
            say!("    {}", call.describe());
        }
        if let Some(size) = placement.size {
            say!("    source size (DWM): {}x{}", size.cx, size.cy);
        }
        let rect_or = |rect: Option<RECT>| rect.map_or_else(|| "?".to_owned(), rect_text);
        say!(
            "    GetWindowRect {}; extended frame bounds {}",
            rect_or(placement.window_rect),
            rect_or(placement.frame)
        );
        say!("    crop: {}", placement.crop_note);
        if let Some(placed) = placement.placed {
            say!(
                "    rcDestination {} (client coordinates)",
                rect_text(placed)
            );
        }
        slot.placement = Some(placement);
    }

    /// Every slot but the second registration's (phase 4).
    pub(super) fn primary_slots(&self) -> Vec<usize> {
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.plan.source != Source::SecondOfFirst)
            .map(|(index, _)| index)
            .collect()
    }

    pub(super) fn registration_count(&self) -> String {
        let with_source = self
            .slots
            .iter()
            .filter(|slot| slot.source.is_some() && slot.plan.source != Source::SecondOfFirst)
            .count();
        let registered = self
            .slots
            .iter()
            .filter(|slot| {
                slot.placement
                    .as_ref()
                    .is_some_and(|placement| placement.thumb.is_some())
            })
            .count();
        format!("{registered} of {with_source} sources registered")
    }

    pub(super) fn update_all(&mut self) -> String {
        let clock = self.clock;
        let mut total = 0;
        let mut failed = Vec::new();
        for slot in &self.slots {
            let Some(call) = slot
                .placement
                .as_ref()
                .and_then(|placement| update_again(clock, placement))
            else {
                continue;
            };
            total += 1;
            say!("[{}] {}", slot.number, call.describe());
            if !call.ok() {
                failed.push(format!("slot {} {}", slot.number, call.short()));
            }
            self.calls.push(call);
        }
        if failed.is_empty() {
            format!("{total} of {total} updates ok")
        } else {
            format!(
                "{} of {total} updates ok; failed: {}",
                total - failed.len(),
                failed.join(", ")
            )
        }
    }
}
