//! ROADMAP_NEW B6.2: HookEcho's self-hostable LDM/IDD Level II ingest backend.
//!
//! This crate is the beginning of the `radar-ingest` service the roadmap calls for — a focused
//! backend, independent of the GUI app, that a deployment points at its own permitted LDM/IDD
//! `NEXRAD2` upstream peer. Nothing here talks to a real LDM process yet (that is B6.11 step 5);
//! this first increment (step 2) lays down the two pieces everything else builds on:
//!
//! - [`input`]: the adapter boundary between "wherever raw products come from" and the ingest
//!   core, plus [`input::ReplayInputAdapter`], the fixture-driven implementation tests and local
//!   development use in place of a live LDM feed (B6.2's explicit ask: "provide an adapter
//!   boundary ... so tests can replay recorded Level II bytes without a live LDM process").
//! - [`store`]: the bounded, per-site in-memory ring buffer raw products land in once accepted —
//!   "maintain per-site rolling state and enough recent blocks for reconnect/resume" (B6.2), with
//!   the memory bound enforced on both item count and total bytes so one large-product site cannot
//!   starve the others.
//!
//! Deliberately absent from this increment: Level II message parsing, rechunking into
//! [`wxdata::live_block::LiveLevel2Block`] (B6.3), and any network distribution (B6.4) — those are
//! later steps in ROADMAP_NEW B6.11's implementation order, and folding them in here would make
//! this increment untestable in isolation.

pub mod input;
pub mod store;
