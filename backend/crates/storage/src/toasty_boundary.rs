//! Toasty integration boundary for control-plane models.
//!
//! The first implementation keeps telemetry outside Toasty and uses raw SQL
//! for Timescale hypertables. Control-plane repositories can replace the
//! in-memory store behind this boundary without changing API handlers.

pub type ToastyDb = toasty::Db;
