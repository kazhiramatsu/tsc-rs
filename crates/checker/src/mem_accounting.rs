//! One-worker/one-checker inventory boundaries, independent of legacy reports.
use crate::program::{BoundDocument, ProgramSnapshot};
use crate::state::CheckerState;
use std::sync::Arc;
use tsc_types::accounting::Account;

fn source(a: &mut Account, source: &tsc_syntax::SourceFile) {
    let bytes = std::mem::size_of_val(source) + 2 * std::mem::size_of::<usize>();
    a.backing(
        "program.source_files",
        (source as *const _ as usize) - 2 * std::mem::size_of::<usize>(),
        1,
        std::mem::size_of_val(source),
        bytes,
        bytes,
        0,
        None,
        None,
    );
    let snapshot = source.snapshot();
    let bytes = std::mem::size_of_val(&**snapshot) + 2 * std::mem::size_of::<usize>();
    a.backing(
        "program.text_snapshots",
        (Arc::as_ptr(snapshot) as usize) - 2 * std::mem::size_of::<usize>(),
        1,
        std::mem::size_of_val(&**snapshot),
        bytes,
        bytes,
        0,
        None,
        None,
    );
    let text = source.text();
    let bytes = text.len() + 2 * std::mem::size_of::<usize>();
    a.backing(
        "program.source_texts",
        (text.as_ptr() as usize) - 2 * std::mem::size_of::<usize>(),
        text.len(),
        1,
        bytes,
        bytes,
        0,
        None,
        None,
    );
    source.arena.account_memory(a);
    diagnostics(a, "syntax.parse_diagnostics", &source.parse_diagnostics);
    diagnostics(a, "syntax.js_doc_diagnostics", &source.js_doc_diagnostics);
}
fn documents(a: &mut Account, documents: &[Arc<BoundDocument>]) {
    for document in documents {
        source(a, document.source());
        document.data.account_memory(a);
        diagnostics(a, "binder.diagnostics", &document.data.bind_diagnostics);
    }
}
fn common(a: &mut Account) {
    tsc_types::names::account_memory(a);
    a.unknown("program.incremental_driver_provider_and_input_copies");
    a.unknown("program.source_positions_and_nested_metadata");
    a.metric(
        "store.mapping_reserved_virtual_bytes",
        Some(tsc_types::accounting::store_reservation_bytes()),
    );
    a.unknown("store.retired_versions_and_allocator_metadata");
}
pub(crate) fn bound(snapshot: &ProgramSnapshot) {
    if let Some(mut a) = Account::open("after_bind") {
        snapshot.account_memory(&mut a);
        documents(&mut a, snapshot.documents());
        common(&mut a);
    }
}
pub(crate) fn completed(snapshot: &ProgramSnapshot, state: &CheckerState<'_>) {
    if let Some(mut a) = Account::open("after_check") {
        snapshot.account_memory(&mut a);
        documents(&mut a, snapshot.documents());
        state.account_memory(&mut a);
        common(&mut a);
    }
}

fn js(a: &mut Account, value: &tsc_types::JsString) {
    if value.heap_bytes() != 0 {
        let text = value.as_js().as_bytes();
        a.backing(
            "diagnostics.texts",
            text.as_ptr() as usize,
            text.len(),
            1,
            text.len(),
            value.heap_bytes(),
            0,
            None,
            None,
        );
    }
}
fn message(a: &mut Account, root: &tsc_diagnostics::MessageChain) {
    // Bounded traversal: a pathological chain is explicitly incomplete.
    let mut pending = [None; 32];
    pending[0] = Some(root);
    let mut len = 1;
    while len != 0 {
        len -= 1;
        let node = pending[len].take().expect("pending diagnostic message");
        js(a, &node.text);
        a.vector("diagnostics.message_args", &node.args);
        for arg in &node.args {
            js(a, arg);
        }
        a.vector("diagnostics.message_children", &node.next);
        for child in &node.next {
            if len == pending.len() {
                a.unknown("diagnostics.message_chain_overflow");
                break;
            }
            pending[len] = Some(child);
            len += 1;
        }
    }
}
pub(crate) fn diagnostics(a: &mut Account, owner: &str, rows: &Vec<tsc_diagnostics::Diagnostic>) {
    a.vector(owner, rows);
    for row in rows {
        if let Some(name) = &row.file_name {
            js(a, name);
        }
        message(a, &row.message);
        a.vector("diagnostics.related", &row.related);
        for related in &row.related {
            if let Some(name) = &related.file_name {
                js(a, name);
            }
            message(a, &related.message);
        }
        if let Some(s) = &row.source {
            a.backing(
                "diagnostics.sources",
                s.as_ptr() as usize,
                s.len(),
                1,
                s.len(),
                s.capacity(),
                0,
                None,
                None,
            );
        }
    }
}
