// Keep the broad CI-core contract suite behind one integration-test target.
// The workspace audit intentionally limits direct targets so Cargo can build
// and schedule the suite as one bounded unit; each module remains a focused
// source file for local ownership and review.
#[path = "contracts/canonical.rs"]
mod canonical;
#[path = "contracts/descriptors.rs"]
mod descriptors;
#[path = "contracts/explain.rs"]
mod explain;
#[path = "contracts/graph.rs"]
mod graph;
#[path = "contracts/graph_schema.rs"]
mod graph_schema;
#[path = "contracts/graph_validation.rs"]
mod graph_validation;
#[path = "contracts/hashes.rs"]
mod hashes;
#[path = "contracts/identifiers.rs"]
mod identifiers;
#[path = "contracts/identity.rs"]
mod identity;
#[path = "contracts/impact.rs"]
mod impact;
#[path = "contracts/inventory.rs"]
mod inventory;
#[path = "contracts/registry_membership.rs"]
mod registry_membership;
