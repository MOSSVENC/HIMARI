# Documentation index

Last reconciled: 2026-11-21. The repository [README](../README.md) is the entry
point; this page is the table of contents for every note under `docs/`, and it
says which notes are **current** and which are **historical evidence**.

## Current reference

- [architecture.md](architecture.md) — crate split: typed protocol, serialized
  hardware worker, Slint UI; where the Win32/Slint boundary sits.
- [cc40-feature-parity.md](cc40-feature-parity.md) — capability matrix against
  the vendor Control Center, with per-control evidence and what is still missing.
- [protocol-map.md](protocol-map.md) — CC4.0/DCHU protocol map recovered from the
  installed vendor package (WMI/ACPI pages, fan tables, quick toggles).
- [fn-event-protocol.md](fn-event-protocol.md) — Fn notification identifiers,
  wide-identifier decoding, per-event OSD mapping.
- [rendering.md](rendering.md) — Windows rendering path: Slint → Skia → Vulkan,
  plus the layered-window OSD surface and its measured limits.
- [driver-installation.md](driver-installation.md) — signed `AcpiBridge` driver as
  a separate deployment dependency, detection and explicit install.
- [vendor-dependency-matrix.md](vendor-dependency-matrix.md) — vendor packages,
  versions and which component owns which control.
- [binary-replacement.md](binary-replacement.md) — how the user-mode vendor path
  is replaced and kept recoverable.
- [vendor-console-retirement.md](vendor-console-retirement.md) — state of the
  legacy CC3.0 console retirement and its provenance.
- [design.md](design.md) — visual direction and the user-facing rules the UI
  follows.
- [verification.md](verification.md) — verification log; newest runs first.
- [quality-assessment.md](quality-assessment.md) — quality review of the current
  tree; historical findings are labelled as such.
- [layout-governor-observations.md](layout-governor-observations.md) — running
  notes and reproducible defects for the directory-governance plugin.
- [handoff-2026-11-21.md](handoff-2026-11-21.md) — takeover record: where the
  work stopped, what was verified, what is still open.

## Historical records

These are kept verbatim as evidence. They describe the tree at their own date and
are not statements about the current build.

- [daily-report-2026-11-14.md](daily-report-2026-11-14.md)
- [commit-log-2026-11-14.md](commit-log-2026-11-14.md)
- [push-log-2026-11-14.md](push-log-2026-11-14.md)
- [incident-report-2026-11-14.md](incident-report-2026-11-14.md) — Windows input
  loss and the unknown USB device.
- [codex-handoff-2026-11-14.md](codex-handoff-2026-11-14.md) — input/USB handoff
  to the next session.
- [repair-status-2026-11-16.md](repair-status-2026-11-16.md) — repair and
  acceptance record for that round.
- [boot-performance-2026-11-19.md](boot-performance-2026-11-19.md) — boot
  telemetry investigation.
- [input-responsiveness-2026-11-19.md](input-responsiveness-2026-11-19.md) —
  input pairing and FlexiKey latency defects.
- [original-package-map.md](original-package-map.md) — Firebat T6KD package map.

## Research

Reverse-engineering notes and the extracted artifacts they cite. `docs/research/`
is deliberately ignored by git; the notes below are tracked.

- [research/protocol-handoff.md](research/protocol-handoff.md) — protocol
  provenance and the vendor binary the map was recovered from.
- [research/cc40-startup-architecture.md](research/cc40-startup-architecture.md)
  — CC40 process and frontend architecture.
- [research/keyboard-filter.md](research/keyboard-filter.md) — `HKKbdFltr`
  keyboard-filter protocol.

## Images

- [images/overview.png](images/overview.png) — Overview page capture used by the
  README.
