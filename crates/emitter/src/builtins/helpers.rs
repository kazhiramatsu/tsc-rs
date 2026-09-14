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

const PROP_KEY_HELPER_TEXT: &str = r#"var __propKey = (this && this.__propKey) || function (x) {
    return typeof x === "symbol" ? x : "".concat(x);
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

const READ_HELPER_TEXT: &str = r#"var __read = (this && this.__read) || function (o, n) {
    var m = typeof Symbol === "function" && o[Symbol.iterator];
    if (!m) return o;
    var i = m.call(o), r, ar = [], e;
    try {
        while ((n === void 0 || n-- > 0) && !(r = i.next()).done) ar.push(r.value);
    }
    catch (error) { e = { error: error }; }
    finally {
        try {
            if (r && !r.done && (m = i["return"])) m.call(i);
        }
        finally { if (e) throw e.error; }
    }
    return ar;
};"#;

const ASSIGN_HELPER_TEXT: &str = r#"var __assign = (this && this.__assign) || function () {
    __assign = Object.assign || function(t) {
        for (var s, i = 1, n = arguments.length; i < n; i++) {
            s = arguments[i];
            for (var p in s) if (Object.prototype.hasOwnProperty.call(s, p))
                t[p] = s[p];
        }
        return t;
    };
    return __assign.apply(this, arguments);
};"#;

const EXTENDS_HELPER_TEXT: &str = r#"var __extends = (this && this.__extends) || (function () {
    var extendStatics = function (d, b) {
        extendStatics = Object.setPrototypeOf ||
            ({ __proto__: [] } instanceof Array && function (d, b) { d.__proto__ = b; }) ||
            function (d, b) { for (var p in b) if (Object.prototype.hasOwnProperty.call(b, p)) d[p] = b[p]; };
        return extendStatics(d, b);
    };

    return function (d, b) {
        if (typeof b !== "function" && b !== null)
            throw new TypeError("Class extends value " + String(b) + " is not a constructor or null");
        extendStatics(d, b);
        function __() { this.constructor = d; }
        d.prototype = b === null ? Object.create(b) : (__.prototype = b.prototype, new __());
    };
})();"#;

const MAKE_TEMPLATE_OBJECT_HELPER_TEXT: &str = r#"var __makeTemplateObject = (this && this.__makeTemplateObject) || function (cooked, raw) {
    if (Object.defineProperty) { Object.defineProperty(cooked, "raw", { value: raw }); } else { cooked.raw = raw; }
    return cooked;
};"#;

const SPREAD_ARRAY_HELPER_TEXT: &str = r#"var __spreadArray = (this && this.__spreadArray) || function (to, from, pack) {
    if (pack || arguments.length === 2) for (var i = 0, l = from.length, ar; i < l; i++) {
        if (ar || !(i in from)) {
            if (!ar) ar = Array.prototype.slice.call(from, 0, i);
            ar[i] = from[i];
        }
    }
    return to.concat(ar || Array.prototype.slice.call(from));
};"#;

const VALUES_HELPER_TEXT: &str = r#"var __values = (this && this.__values) || function(o) {
    var s = typeof Symbol === "function" && Symbol.iterator, m = s && o[s], i = 0;
    if (m) return m.call(o);
    if (o && typeof o.length === "number") return {
        next: function () {
            if (o && i >= o.length) o = void 0;
            return { value: o && o[i++], done: !o };
        }
    };
    throw new TypeError(s ? "Object is not iterable." : "Symbol.iterator is not defined.");
};"#;

const GENERATOR_HELPER_TEXT: &str = r#"var __generator = (this && this.__generator) || function (thisArg, body) {
    var _ = { label: 0, sent: function() { if (t[0] & 1) throw t[1]; return t[1]; }, trys: [], ops: [] }, f, y, t, g = Object.create((typeof Iterator === "function" ? Iterator : Object).prototype);
    return g.next = verb(0), g["throw"] = verb(1), g["return"] = verb(2), typeof Symbol === "function" && (g[Symbol.iterator] = function() { return this; }), g;
    function verb(n) { return function (v) { return step([n, v]); }; }
    function step(op) {
        if (f) throw new TypeError("Generator is already executing.");
        while (g && (g = 0, op[0] && (_ = 0)), _) try {
            if (f = 1, y && (t = op[0] & 2 ? y["return"] : op[0] ? y["throw"] || ((t = y["return"]) && t.call(y), 0) : y.next) && !(t = t.call(y, op[1])).done) return t;
            if (y = 0, t) op = [op[0] & 2, t.value];
            switch (op[0]) {
                case 0: case 1: t = op; break;
                case 4: _.label++; return { value: op[1], done: false };
                case 5: _.label++; y = op[1]; op = [0]; continue;
                case 7: op = _.ops.pop(); _.trys.pop(); continue;
                default:
                    if (!(t = _.trys, t = t.length > 0 && t[t.length - 1]) && (op[0] === 6 || op[0] === 2)) { _ = 0; continue; }
                    if (op[0] === 3 && (!t || (op[1] > t[0] && op[1] < t[3]))) { _.label = op[1]; break; }
                    if (op[0] === 6 && _.label < t[1]) { _.label = t[1]; t = op; break; }
                    if (t && _.label < t[2]) { _.label = t[2]; _.ops.push(op); break; }
                    if (t[2]) _.ops.pop();
                    _.trys.pop(); continue;
            }
            op = body.call(thisArg, _);
        } catch (e) { op = [6, e]; y = 0; } finally { f = t = 0; }
        if (op[0] & 5) throw op[1]; return { value: op[0] ? op[1] : void 0, done: true };
    }
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

const AWAITER_HELPER_TEXT: &str = r#"var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {
    function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }
    return new (P || (P = Promise))(function (resolve, reject) {
        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }
        function rejected(value) { try { step(generator["throw"](value)); } catch (e) { reject(e); } }
        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }
        step((generator = generator.apply(thisArg, _arguments || [])).next());
    });
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

pub(super) fn prop_key() -> EmitHelper {
    EmitHelper::with_text(
        "typescript:propKey",
        false,
        PROP_KEY_HELPER_TEXT,
        None,
        Vec::new(),
    )
}

pub(super) fn object_rest() -> EmitHelper {
    EmitHelper::with_text("typescript:rest", false, REST_HELPER_TEXT, None, Vec::new())
}

pub(super) fn read() -> EmitHelper {
    EmitHelper::with_text("typescript:read", false, READ_HELPER_TEXT, None, Vec::new())
}

/// Requested by the CA-2b object-spread forks (H2.5h corpus adoption):
/// the es2018 lowering and the JSX spread-attribute builder call the
/// helper below ES2015 where upstream's `createAssignHelper` forks away
/// from `Object.assign`.
/// tsc-port: assignHelper @6.0.3
/// tsc-hash: a195c84f6fbb4280f8164fde5d33f0d246cfec5b40286c16416b042d9de991f1
/// tsc-span: _tsc.js:26122-26139
pub(super) fn assign() -> EmitHelper {
    EmitHelper::with_text(
        "typescript:assign",
        false,
        ASSIGN_HELPER_TEXT,
        Some(1),
        Vec::new(),
    )
}

/// Requested by the B-4 ES2015 class-lowering lanes (H2.5h-b); until those owners land,
/// the factory is reachable only from the byte-parity unit suite below.
/// tsc-port: extendsHelper @6.0.3
/// tsc-hash: 6b5178969d2205e2b7bf428def518fce4fb427d1c56dd9d67a7f98e497e6f8d4
/// tsc-span: _tsc.js:26224-26246
#[allow(dead_code)]
pub(super) fn extends() -> EmitHelper {
    EmitHelper::with_text(
        "typescript:extends",
        false,
        EXTENDS_HELPER_TEXT,
        Some(0),
        Vec::new(),
    )
}

/// Requested by the B-5 tagged-template shared module (H2.5h-b) — the
/// second priority-0 helper next to `typescript:extends`.
/// tsc-port: templateObjectHelper @6.0.3
/// tsc-hash: 95a23beee7acf99b5f61e7ee0971c9783339404894a17b279498f19421a9609f
/// tsc-span: _tsc.js:26247-26257
pub(super) fn make_template_object() -> EmitHelper {
    EmitHelper::with_text(
        "typescript:makeTemplateObject",
        false,
        MAKE_TEMPLATE_OBJECT_HELPER_TEXT,
        Some(0),
        Vec::new(),
    )
}

/// Requested by the B-4 ES2015 call/array spread lanes (H2.5h-b); until those owners land,
/// the factory is reachable only from the byte-parity unit suite below.
/// tsc-port: spreadArrayHelper @6.0.3
/// tsc-hash: 41436a6b055f8314a4496de97df7d447ebfce78c2ef4b68939ff7ef97e7c0896
/// tsc-span: _tsc.js:26280-26294
#[allow(dead_code)]
pub(super) fn spread_array() -> EmitHelper {
    EmitHelper::with_text(
        "typescript:spreadArray",
        false,
        SPREAD_ARRAY_HELPER_TEXT,
        None,
        Vec::new(),
    )
}

/// Requested by the B-4 ES2015 iteration lanes (H2.5h-b); until those owners land,
/// the factory is reachable only from the byte-parity unit suite below.
/// tsc-port: valuesHelper @6.0.3
/// tsc-hash: 7f2f157873cc2dfc3c0a2548ade92b93d59e75d6f7b69ffd441c67b5f05a7ad9
/// tsc-span: _tsc.js:26314-26330
#[allow(dead_code)]
pub(super) fn values() -> EmitHelper {
    EmitHelper::with_text(
        "typescript:values",
        false,
        VALUES_HELPER_TEXT,
        None,
        Vec::new(),
    )
}

/// Requested by the B-3 Generators state machine (H2.5h-b); until those owners land,
/// the factory is reachable only from the byte-parity unit suite below.
/// tsc-port: generatorHelper @6.0.3
/// tsc-hash: 8e304cf0731f40924fc0b2c9e6a4b7f9773ef2d8d317e1d59c172d476cf820d3
/// tsc-span: _tsc.js:26331-26364
#[allow(dead_code)]
pub(super) fn generator() -> EmitHelper {
    EmitHelper::with_text(
        "typescript:generator",
        false,
        GENERATOR_HELPER_TEXT,
        Some(6),
        Vec::new(),
    )
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

pub(super) fn awaiter() -> EmitHelper {
    EmitHelper::with_text(
        "typescript:awaiter",
        false,
        AWAITER_HELPER_TEXT,
        Some(5),
        Vec::new(),
    )
}

#[cfg(test)]
#[path = "../../tests/unit/helpers/tests.rs"]
mod helpers_tests;
