//! The startup work `run` calls: where the app keeps its files
//! (`paths`), the service over the settings store it can use
//! (`service`), logging, the panic hook, and the forwarding of the
//! service's events to the pages (`forward`).

pub(crate) mod forward;
pub(crate) mod logging;
pub(crate) mod panic_hook;
pub(crate) mod paths;
pub(crate) mod service;

#[cfg(test)]
mod tests;
