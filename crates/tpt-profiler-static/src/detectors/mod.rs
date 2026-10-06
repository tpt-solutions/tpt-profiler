//! Built-in detectors (spec §9–13). Each is a small, individually tested
//! `syn` visitor; limits of span-only (no type info) analysis are
//! documented per detector.

pub mod allocation;
pub mod clone;
pub mod datastructure;
pub mod loops;
pub mod vector;
