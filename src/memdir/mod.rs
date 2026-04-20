//! Background memory agents + MEMORY.md discovery — crosslink #508.
//!
//! Phased rollout (see `docs/designs/508-memdir.md`):
//!
//! - **Phase 1 (this commit)**: MEMORY.md entrypoint discovery +
//!   loading + truncation. No background agents yet — callers read
//!   the loaded text and inject it into the system prompt themselves.
//! - **Phase 2+**: per-session notes writer, extractor subagent,
//!   autoDream consolidation, prompt suggestion speculation.
//!
//! The module is deliberately additive for now: nothing else in the
//! codebase calls into it. The `prompt.rs` builder can optionally
//! invoke [`load_entrypoint`] to get a truncated MEMORY.md block,
//! but the wiring lands as a follow-up so this commit can ship with
//! only the new module under test.

pub mod entrypoint;

pub use entrypoint::{
    load_entrypoint, EntrypointFile, EntrypointTruncation, MAX_ENTRYPOINT_BYTES,
    MAX_ENTRYPOINT_LINES,
};
