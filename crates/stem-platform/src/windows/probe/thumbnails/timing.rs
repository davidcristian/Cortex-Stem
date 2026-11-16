//! `QueryPerformanceCounter` timing, the record of one timed call, and the
//! timing lines of the summary.

use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::core::HRESULT;

use super::{SLOW_MICROS, emit};

/// `QueryPerformanceCounter` timing.
#[derive(Clone, Copy, Debug)]
pub(super) struct Clock {
    frequency: i64,
}

impl Clock {
    pub(super) fn new() -> Self {
        let mut frequency = 0i64;
        // SAFETY: `frequency` is a live out-pointer for the call.
        let result = unsafe { QueryPerformanceFrequency(&mut frequency) };
        if let Err(e) = result {
            say!("QueryPerformanceFrequency failed ({e}); every call time reads 0");
            frequency = 0;
        }
        Self { frequency }
    }

    fn ticks() -> i64 {
        let mut now = 0i64;
        // SAFETY: `now` is a live out-pointer for the call; it cannot fail
        // on Windows XP and later, and a failure leaves 0.
        let _ = unsafe { QueryPerformanceCounter(&mut now) };
        now
    }

    /// Runs `call` and returns its value and its duration in microseconds.
    pub(super) fn time<T>(self, call: impl FnOnce() -> T) -> (T, f64) {
        let start = Self::ticks();
        let value = call();
        let end = Self::ticks();
        let micros = if self.frequency > 0 {
            (end - start) as f64 * 1_000_000.0 / self.frequency as f64
        } else {
            0.0
        };
        (value, micros)
    }
}

/// One timed API call and its `HRESULT`.
#[derive(Clone, Debug)]
pub(super) struct Call {
    pub(super) api: &'static str,
    pub(super) hr: HRESULT,
    pub(super) micros: f64,
}

impl Call {
    pub(super) fn of<T>(api: &'static str, result: &windows::core::Result<T>, micros: f64) -> Self {
        let hr = match result {
            Ok(_) => HRESULT(0),
            Err(e) => e.code(),
        };
        Self { api, hr, micros }
    }

    pub(super) fn ok(&self) -> bool {
        self.hr.is_ok()
    }

    pub(super) fn short(&self) -> String {
        if self.ok() {
            "ok".to_owned()
        } else {
            hex(self.hr)
        }
    }

    pub(super) fn describe(&self) -> String {
        if self.ok() {
            format!("{} ok in {:.0} us", self.api, self.micros)
        } else {
            format!(
                "{} FAILED {} ({}) in {:.0} us",
                self.api,
                hex(self.hr),
                self.hr.message().trim(),
                self.micros
            )
        }
    }
}

fn hex(hr: HRESULT) -> String {
    format!("0x{:08X}", hr.0 as u32)
}

/// Count, slowest and mean per API, and the calls over one frame (A6).
pub(super) fn timing_lines(calls: &[Call]) -> Vec<String> {
    let mut apis: Vec<&'static str> = Vec::new();
    for call in calls {
        if !apis.contains(&call.api) {
            apis.push(call.api);
        }
    }
    let mut lines = Vec::new();
    for api in apis {
        let of: Vec<&Call> = calls.iter().filter(|call| call.api == api).collect();
        let slowest = of.iter().map(|call| call.micros).fold(0.0f64, f64::max);
        let mean = of.iter().map(|call| call.micros).sum::<f64>() / of.len().max(1) as f64;
        let failed = of.iter().filter(|call| !call.ok()).count();
        lines.push(format!(
            "A6 {api}: {} calls, {failed} failed, slowest {slowest:.0} us, mean {mean:.0} us",
            of.len()
        ));
    }
    let slow = calls
        .iter()
        .filter(|call| call.micros > SLOW_MICROS)
        .count();
    lines.push(if slow == 0 {
        format!(
            "A6: none of {} DWM calls took longer than 16 ms",
            calls.len()
        )
    } else {
        format!(
            "A6: {slow} of {} DWM calls took longer than 16 ms",
            calls.len()
        )
    });
    lines
}
