//! The four phases, and the pause after each step.

use std::sync::mpsc::Receiver;

use super::dwm::unregister;
use super::emit;
use super::input::{Step, settle_input, wait_for_line};
use super::probe::Probe;
use super::quit::{Quit, quit_text, quitting, request_quit};
use super::timing::Call;

impl Probe {
    fn pause(&mut self, input: &Receiver<Option<String>>, phase: &str, look: &[&str]) -> bool {
        self.refresh_board();
        settle_input(input);
        emit("");
        emit("Look:");
        for line in look {
            say!("  - {line}");
        }
        emit("Take a screenshot (Win+Shift+S). Type a note if you like, then press Enter;");
        emit("q and Enter stops the probe (it still cleans up).");
        match wait_for_line(input) {
            Step::Line(line) if line.eq_ignore_ascii_case("q") => {
                request_quit(Quit::Typed);
                say!("stopping: {}", quit_text());
                false
            }
            Step::Line(line) => {
                if !line.is_empty() {
                    self.notes.push((phase.to_owned(), line));
                }
                true
            }
            Step::Stop => {
                say!("stopping: {}", quit_text());
                false
            }
        }
    }

    pub(super) fn run_phases(&mut self, input: &Receiver<Option<String>>) -> bool {
        if quitting() {
            say!("stopping before phase 1: {}", quit_text());
            return false;
        }
        self.set_header("Phase 1 of 4: every source registered on the window's own thread");
        for index in self.primary_slots() {
            self.place(index);
        }
        if !self.pause(
            input,
            "phase 1",
            &[
                "each slot: a picture, a black or empty picture, or the grey 'no picture here'?",
                "own sources (1 to 6): does the clock in the picture tick, or is it frozen?",
                "slot 1 vs slot 2: slot 1 should end exactly at the source's visible frame; \
                 slot 2 may show a transparent margin (the invisible resize borders, A7)",
                "does every picture sit exactly inside its yellow outline (A2)?",
                "slots 5 and 6: compare the screen with your screenshot (a screenshot is itself \
                 a capture, A9)",
            ],
        ) {
            return false;
        }

        self.set_header("Phase 2 of 4: calls from a worker thread (A6)");
        if !self.phase_worker_thread() {
            return false;
        }
        if !self.pause(
            input,
            "phase 2",
            &[
                "slot 2 was unregistered and registered again on a worker thread: does it still \
                 show its picture?",
                "slot 1 was updated from the worker thread: unchanged?",
            ],
        ) {
            return false;
        }

        self.set_header("Phase 3 of 4, step 1: WS_EX_LAYERED on, alpha 255 (A5)");
        let line = self.set_layered(true);
        self.results.push(format!("A5 step 1 (layered on): {line}"));
        if !self.pause(
            input,
            "phase 3.1",
            &["do the pictures still show, flicker, freeze or vanish?"],
        ) {
            return false;
        }
        self.set_header("Phase 3 of 4, step 2: every thumbnail updated again while layered");
        let line = self.update_all();
        self.results
            .push(format!("A5 step 2 (update while layered): {line}"));
        if !self.pause(
            input,
            "phase 3.2",
            &["do the pictures show now? (record any change from step 1)"],
        ) {
            return false;
        }
        self.set_header("Phase 3 of 4, step 3: WS_EX_LAYERED off again");
        let line = self.set_layered(false);
        self.results
            .push(format!("A5 step 3 (layered off): {line}"));
        if !self.pause(
            input,
            "phase 3.3",
            &["did the pictures come back on their own?"],
        ) {
            return false;
        }
        self.set_header("Phase 3 of 4, step 4: every thumbnail unregistered and registered again");
        for index in self.primary_slots() {
            self.place(index);
        }
        let line = self.registration_count();
        self.results
            .push(format!("A5 step 4 (fresh registrations): {line}"));
        if !self.pause(
            input,
            "phase 3.4",
            &["every picture back as in phase 1? (what Stem does after click-through)"],
        ) {
            return false;
        }

        self.set_header("Phase 4 of 4, step 1: a second registration of slot 1's source (A11)");
        let second = self.slots.len().saturating_sub(1);
        self.place(second);
        let line = self
            .slots
            .get(second)
            .and_then(|slot| slot.placement.as_ref())
            .and_then(|placement| placement.register.as_ref())
            .map_or_else(|| "not registered (no source)".to_owned(), Call::describe);
        self.results
            .push(format!("A11 step 1 (second registration): {line}"));
        if !self.pause(
            input,
            "phase 4.1",
            &["do slot 1 and slot 10 both show a live picture (ticking clock)?"],
        ) {
            return false;
        }
        self.set_header("Phase 4 of 4, step 2: the second registration removed");
        let clock = self.clock;
        let line = match self
            .slots
            .get_mut(second)
            .and_then(|slot| slot.placement.as_mut())
            .and_then(|placement| placement.thumb.take())
        {
            Some(thumb) => {
                let call = unregister(clock, thumb);
                let text = call.describe();
                self.calls.push(call);
                text
            }
            None => "nothing to unregister".to_owned(),
        };
        say!("[10] {line}");
        self.results
            .push(format!("A11 step 2 (second removed): {line}"));
        self.pause(
            input,
            "phase 4.2",
            &["does slot 1 still show its live picture?"],
        )
    }
}
