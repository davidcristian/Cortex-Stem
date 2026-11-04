# Research: auto-hide

This page holds the detail behind [ADR-0008](../adr/0008-auto-hide.md),
which decides how the dock hides and comes back: the alternatives it
rejects for detecting the reveal.

## Detecting the reveal

These are the alternatives to the reveal strip that
[ADR-0008](../adr/0008-auto-hide.md#decision) rejects. A low-level mouse
hook (`WH_MOUSE_LL`) is rejected: it runs in the input path of every
process, Windows removes it silently when a callback is slow, and it would
duplicate the tracking the hit-test thread does. A second poll on the
events thread is rejected for the same duplication.
