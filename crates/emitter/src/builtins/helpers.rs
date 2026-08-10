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

const REST_HELPER_TEXT: &str = r#"var __rest = (this && this.__rest) || function (s, e) {
    var t = {};
    for (var p in s) if (Object.prototype.hasOwnProperty.call(s, p) && e.indexOf(p) < 0)
        t[p] = s[p];
    if (s != null && typeof Object.getOwnPropertySymbols === "function")
        for (var i = 0, p = Object.getOwnPropertySymbols(s); i < p.length; i++) {
            if (e.indexOf(p[i]) < 0 && Object.prototype.propertyIsEnumerable.call(s, p[i]))
                t[p[i]] = s[p[i]];
        }
    return t;
};"#;

const AWAIT_HELPER_TEXT: &str = r#"var __await = (this && this.__await) || function (v) { return this instanceof __await ? (this.v = v, this) : new __await(v); }"#;

const ASYNC_GENERATOR_HELPER_TEXT: &str = r#"var __asyncGenerator = (this && this.__asyncGenerator) || function (thisArg, _arguments, generator) {
    if (!Symbol.asyncIterator) throw new TypeError("Symbol.asyncIterator is not defined.");
    var g = generator.apply(thisArg, _arguments || []), i, q = [];
    return i = Object.create((typeof AsyncIterator === "function" ? AsyncIterator : Object).prototype), verb("next"), verb("throw"), verb("return", awaitReturn), i[Symbol.asyncIterator] = function () { return this; }, i;
    function awaitReturn(f) { return function (v) { return Promise.resolve(v).then(f, reject); }; }
    function verb(n, f) { if (g[n]) { i[n] = function (v) { return new Promise(function (a, b) { q.push([n, v, a, b]) > 1 || resume(n, v); }); }; if (f) i[n] = f(i[n]); } }
    function resume(n, v) { try { step(g[n](v)); } catch (e) { settle(q[0][3], e); } }
    function step(r) { r.value instanceof __await ? Promise.resolve(r.value.v).then(fulfill, reject) : settle(q[0][2], r); }
    function fulfill(value) { resume("next", value); }
    function reject(value) { resume("throw", value); }
    function settle(f, v) { if (f(v), q.shift(), q.length) resume(q[0][0], q[0][1]); }
};"#;

const ASYNC_DELEGATOR_HELPER_TEXT: &str = r#"var __asyncDelegator = (this && this.__asyncDelegator) || function (o) {
    var i, p;
    return i = {}, verb("next"), verb("throw", function (e) { throw e; }), verb("return"), i[Symbol.iterator] = function () { return this; }, i;
    function verb(n, f) { i[n] = o[n] ? function (v) { return (p = !p) ? { value: __await(o[n](v)), done: false } : f ? f(v) : v; } : f; }
};"#;

const ASYNC_VALUES_HELPER_TEXT: &str = r#"var __asyncValues = (this && this.__asyncValues) || function (o) {
    if (!Symbol.asyncIterator) throw new TypeError("Symbol.asyncIterator is not defined.");
    var m = o[Symbol.asyncIterator], i;
    return m ? m.call(o) : (o = typeof __values === "function" ? __values(o) : o[Symbol.iterator](), i = {}, verb("next"), verb("throw"), verb("return"), i[Symbol.asyncIterator] = function () { return this; }, i);
    function verb(n) { i[n] = o[n] && function (v) { return new Promise(function (resolve, reject) { v = o[n](v), settle(resolve, reject, v.done, v.value); }); }; }
    function settle(resolve, reject, d, v) { Promise.resolve(v).then(function(v) { resolve({ value: v, done: d }); }, reject); }
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

pub(super) fn object_rest() -> EmitHelper {
    EmitHelper::with_text("typescript:rest", false, REST_HELPER_TEXT, None, Vec::new())
}

pub(super) fn async_await() -> EmitHelper {
    EmitHelper::with_text(
        "typescript:await",
        false,
        AWAIT_HELPER_TEXT,
        None,
        Vec::new(),
    )
}

pub(super) fn async_generator() -> EmitHelper {
    EmitHelper::with_text(
        "typescript:asyncGenerator",
        false,
        ASYNC_GENERATOR_HELPER_TEXT,
        None,
        vec![async_await()],
    )
}

pub(super) fn async_delegator() -> EmitHelper {
    EmitHelper::with_text(
        "typescript:asyncDelegator",
        false,
        ASYNC_DELEGATOR_HELPER_TEXT,
        None,
        vec![async_await()],
    )
}

pub(super) fn async_values() -> EmitHelper {
    EmitHelper::with_text(
        "typescript:asyncValues",
        false,
        ASYNC_VALUES_HELPER_TEXT,
        None,
        Vec::new(),
    )
}
