//! `tonic completions <shell>` integration tests (#21).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::string_slice
)]

mod common;
use common::{stdout, Scratch};

#[test]
fn completions_emits_a_script() {
    let s = Scratch::new();
    let out = s.tonic(&["completions", "bash"]);
    assert!(out.status.success(), "completions should succeed");
    // the generated bash script defines the completion function for the binary
    assert!(stdout(&out).contains("_tonic"), "expected a completion script:\n{}", stdout(&out));
}

#[test]
fn completions_rejects_an_unknown_shell() {
    let s = Scratch::new();
    let out = s.tonic(&["completions", "nonsense"]);
    assert!(!out.status.success(), "an unknown shell should error");
}
