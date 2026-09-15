//! Independent integration target for the A6-41-SUPER decorator static-`super`
//! witnesses. It does not touch the shared `contracts` target or its 530-case
//! retained-accessor assertion. `h2_7b_w4a_controls` is included only for its
//! exact complete-command comparator; its own tests run here only when
//! selected by name.
#[path = "integration/h2_7b_w4a_controls.rs"]
#[allow(dead_code)]
mod h2_7b_w4a_controls;
#[path = "integration/h2_8a_decorator_super.rs"]
mod h2_8a_decorator_super;
