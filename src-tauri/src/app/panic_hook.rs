use crate::crash;

/// Write the crash marker (`crash::record_panic`, so the next start can
/// report the crash) and log every panic (message, location,
/// backtrace) before the default hook prints it. With `panic = "abort"` in
/// release builds these are the only record of a crash, so the log line
/// goes through `log` (file + stdout; the file target flushes per record).
/// The marker comes first: it is one small write, and the next start shows
/// it even if the log line never makes it out.
pub(crate) fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("<non-string panic payload>");
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown location>".to_owned());
        crash::record_panic(message, &location);
        let backtrace = std::backtrace::Backtrace::force_capture();
        log::error!("panic at {location}: {message}\nbacktrace:\n{backtrace}");
        default_hook(info);
    }));
}

/// Debug builds only: `STEM_PANIC_TEST=1` (surrounding blanks ignored)
/// panics once logging is up, so the panic hook can be exercised and the
/// next start's crash report with it.
pub(crate) fn panic_test() {
    if cfg!(debug_assertions) && std::env::var("STEM_PANIC_TEST").is_ok_and(|v| v.trim() == "1") {
        panic!("STEM_PANIC_TEST=1: intentional panic to exercise the panic hook");
    }
}
