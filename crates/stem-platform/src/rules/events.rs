//! WinEvent / shell-hook translation table.
//!
//! Map `(event id, object id, child id)` to an intent such as `Added`,
//! `Removed`, `Updated`, `Foreground`, `Attention` or `Ignore`, and hold the
//! debounce policy for name-change storms. Pure and tested here.
