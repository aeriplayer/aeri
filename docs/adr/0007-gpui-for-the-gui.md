# ADR 0007 — The GUI is built on GPUI

- **Status:** Accepted
- **Date:** 2026-11-14
- **Supersedes:** the `iced` entry in the original tech stack

## Context

The GUI of a music player is dominated by very large virtualised lists and tables
(tens of thousands of tracks), dense text, and a macOS-first expectation of polish.
`iced` was the original choice. Both `iced` and GPUI are pre-1.0 and break their APIs
between releases, so neither clears the "defensible dependencies" pillar without
written rationale. This is that rationale.

## Decision

`aeri-gui` is built on [GPUI](https://gpui.rs) (Zed's UI framework), with
`gpui-component` for stock widgets where it fits.

- GPUI was built for exactly this workload and is proven at scale by Zed.
- Text rendering and input feel on macOS are stronger than iced's.
- Licence is Apache-2.0 for both crates, compatible with `MIT OR Apache-2.0`.
  (Checked on crates.io on the date above: `gpui` 0.2.2, `gpui-component` 0.9.1.)

No GUI dependency is carried in the workspace until Phase 6. The version is chosen and
pinned then; `iced` is removed now.

## Consequences

- GPUI runs its own executor, not tokio. `aeri-gui` bridges to the `Player` trait over
  channels; no tokio handle crosses into view code.
- API churn is expected. The GUI is isolated behind `Player`, so churn is contained in
  one crate.
- **To verify at the start of Phase 6, before building screens:**
  - Accessibility support. The v1.0 roadmap has an accessibility audit; GPUI's support
    was not verified when this was written.
  - Linux requirements (GPU/Vulkan, Wayland and X11) on the supported targets.
  - A 50,000-row track list with live search, as the first thing built.
- If that verification fails, the fallback is `iced`, recorded as a new ADR. The cost
  of switching is the GUI crate only.
