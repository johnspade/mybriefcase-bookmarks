#[expect(
    dead_code,
    reason = "shared test helpers; not all used by every test binary"
)]
mod common;

#[path = "prop_tests/invariants.rs"]
mod invariants;
#[path = "prop_tests/merge.rs"]
mod merge;
#[path = "prop_tests/roundtrip.rs"]
mod roundtrip;
#[path = "prop_tests/strategies.rs"]
mod strategies;
#[path = "prop_tests/tree_ops.rs"]
mod tree_ops;
