# Research: the DWM thumbnail probe

The thumbnail probe is a diagnostic program in `stem-platform`, not part of
the app. One run on Windows settles the open assumptions of ADR-0009 before
window previews are built on DWM thumbnails. Its pure parts have unit tests
(see [Source files](source-files.md)).

## Why a probe

[ADR-0009](../../adr/0009-window-thumbnails.md) recommends drawing DWM
thumbnails into the dock window. Microsoft's documentation does not confirm
its assumptions A1 to A11, and several of them decide the design: whether a
minimised or cloaked window gives a picture, whether a layered destination
keeps its thumbnails, whether a second registration of one source breaks
the first, and whether the calls are fast enough for the UI thread. One run
of the probe on a Windows desktop settles them.

## Contents

- [running.md](running.md): what to open before a run, the command, and
  how to step through and stop the probe.
- [what-it-does.md](what-it-does.md): the windows the probe opens and
  reads, the ten slots of its grid, and its phases.
- [output.md](output.md): the block printed for each registration and the
  summary at the end.
- [reading-the-result.md](reading-the-result.md): what in the output
  settles each assumption, and the limits of one run.
- [source-files.md](source-files.md): the probe's source files, its
  feature, and the checks that lint and test it.
