//! Which windows belong in the dock.
//!
//! `windows::enumerate` collects plain facts about a window (styles, owner,
//! class, title, cloak state, process) into a struct defined here and asks
//! `is_taskbar_candidate(&facts)`. Keep the rule table and its tests here.
