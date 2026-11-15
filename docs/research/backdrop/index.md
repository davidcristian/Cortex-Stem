# Research: Acrylic or Mica behind the dock panel

This note compares the ways to show a Windows system material (Acrylic or
Mica) behind the dock's panel. It recommends option 6, a second native
window under the panel, and describes the prototype of that option that
Stem contains.

## Contents

- [context.md](context.md): the goal, the shape of the dock window, and
  what the code does that matters for a backdrop.
- [css-look.md](css-look.md): what the built-in CSS look draws today.
- [options.md](options.md): options 1 to 5, the ideas not taken further,
  and a summary table of every option.
- [backdrop-window.md](backdrop-window.md): option 6, a second backdrop
  window under the dock window, and its assessment.
- [recommendation.md](recommendation.md): the recommended option and
  material, and what holds either way.
- [consequences.md](consequences.md): what changes if option 6 ships.
- [assumptions.md](assumptions.md): assumptions A1 to A10, which
  Microsoft's documentation does not confirm.
- [manual-test-plan.md](manual-test-plan.md): the manual test on Windows 11
  build 22621 or later that checks the assumptions and the prototype.
- [remaining-work.md](remaining-work.md): what the prototype does not
  contain yet.
- [sources.md](sources.md): the documentation, code and reports this note
  relies on.
