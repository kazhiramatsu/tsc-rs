// Keep runner contracts in one bounded integration-test target while each
// source file remains independently reviewable.
#[path = "contracts/bounded_effect.rs"]
mod bounded_effect;
#[path = "contracts/error_boundary.rs"]
mod error_boundary;
#[path = "contracts/snapshot_resource.rs"]
mod snapshot_resource;
