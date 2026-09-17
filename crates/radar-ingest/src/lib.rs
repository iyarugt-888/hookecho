//! ROADMAP_NEW B6.2: HookEcho's self-hostable LDM/IDD Level II ingest backend.
//!
//! This crate is the `radar-ingest` service the roadmap calls for — a focused backend,
//! independent of the GUI app, that a deployment points at its own permitted LDM/IDD `NEXRAD2`
//! upstream peer. Nothing here talks to a real LDM process yet (that is B6.11 step 5); the pieces
//! built so far cover B6.11 steps 1-4:
//!
//! - [`input`]: the adapter boundary between "wherever raw products come from" and the ingest
//!   core, plus [`input::ReplayInputAdapter`], the fixture-driven implementation tests and local
//!   development use in place of a live LDM feed (B6.2's explicit ask: "provide an adapter
//!   boundary ... so tests can replay recorded Level II bytes without a live LDM process").
//! - [`store`]: the bounded, per-site in-memory holding area raw products land in once accepted —
//!   "maintain per-site rolling state" (B6.2), with the memory bound enforced on both item count
//!   and total bytes so one large-product site cannot starve the others.
//! - [`rechunk`]: parses raw products into canonical, lossless
//!   [`wxdata::live_block::LiveLevel2Block`]s (B6.3) — the identity/provenance model
//!   [`wxdata::live_block`] defines, applied to a live byte stream for the first time.
//! - [`block_store`]: bounded per-site retention of emitted blocks, indexed by sequence — what a
//!   reconnecting client resumes from (B6.4).
//! - [`pipeline`]: wires the rechunker to block retention and live-subscriber fan-out.
//! - [`wire`]: the JSON wire types for [`server`]'s HTTP/WebSocket API.
//! - [`server`]: the WebSocket live stream and HTTP resume/backfill API (B6.4).
//!
//! Deliberately absent so far: a live LDM connection (B6.11 step 5) and the client-side
//! `HookEchoRelayLevel2Provider` (step 6) — those are later steps in ROADMAP_NEW B6.11's
//! implementation order.

pub mod block_store;
pub mod input;
pub mod pipeline;
pub mod rechunk;
pub mod server;
pub mod store;
pub mod wire;
