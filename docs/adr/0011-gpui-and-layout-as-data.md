# ADR 0011 — The GUI is built on GPUI, and its layout is data

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

The interface of a foobar-style player has two hard requirements:

- very large virtualised lists and tables with dense, script-formatted text;
- a layout the user assembles: every panel is a widget, placed in a tree of splitters and tabs, rearranged at run time and saved.

Rust has no mature toolkit. GPUI (Zed's framework) and `iced` are both pre-1.0 and break their APIs between releases. GPUI was built for large virtualised text views and is proven by Zed; its licence and `gpui-component`'s are Apache-2.0 (checked on crates.io on 2026-11-14: `gpui` 0.2.2, `gpui-component` 0.9.1).

## Decision

1. **`aeri-gui` is built on GPUI**, with `gpui-component` for stock widgets where it fits. No GUI dependency enters the workspace before the GUI phase.
2. **Layout is data.** The window's content is a tree whose nodes are splitters, tab stacks, and widgets named by registry id (ADR 0002), each with its own settings. The tree is serialised to a file. The default interface is one such file; there is no hard-coded screen.
3. **Editing the layout in the application comes after the layout tree itself.** The first GUI release renders a layout file; a later one edits it.

## Consequences

- GPUI runs its own executor. `aeri-gui` awaits the `Player` handle on it (ADR 0004); no tokio runtime is involved.
- API churn is expected and is contained in one crate behind `Player` (ADR 0006).
- **To verify at the start of the GUI phase, before any screen is built** (none of these was verified when this was written):
  - a 50,000-row track table with script-formatted columns and live filtering;
  - nested, resizable splitters and tab stacks driven from a deserialised tree;
  - accessibility support, and Linux requirements (GPU, Wayland, X11).
- If verification fails, the fallback is decided then and recorded as a new ADR. The cost of switching is `aeri-gui` only.
- Building every screen as registry widgets costs more up front than fixed screens. It is the product's identity and cannot be retrofitted cheaply.
