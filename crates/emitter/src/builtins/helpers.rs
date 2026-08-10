//! Shared definitions for emit helpers requested by more than one transform.
//!
//! Helper identity, ordering, and text belong to the transformation pipeline,
//! not to whichever pass happens to request them first.  Keeping shared
//! helpers here lets independently composed passes request the same typed
//! value without duplicating its observable contract.

use crate::EmitHelper;

const SET_FUNCTION_NAME_HELPER_TEXT: &str = r#"var __setFunctionName = (this && this.__setFunctionName) || function (f, name, prefix) {
    if (typeof name === "symbol") name = name.description ? "[".concat(name.description, "]") : "";
    return Object.defineProperty(f, "name", { configurable: true, value: prefix ? "".concat(prefix, " ", name) : name });
};"#;

pub(super) fn set_function_name() -> EmitHelper {
    EmitHelper::with_text(
        "typescript:setFunctionName",
        false,
        SET_FUNCTION_NAME_HELPER_TEXT,
        None,
        Vec::new(),
    )
}
