//! Experimental bounded index of immutable straight assignment blocks.
//! It contains binding identities only, never reference-specific flow answers.
use tsc_binder::{flow::FlowId, SymbolId};
#[cfg(test)]
use tsc_syntax::NodeId;
use tsc_syntax::SyntaxKind;
use tsc_types::FlowFlags;

use crate::flow::FlowQuery;
use crate::state::CheckerState;

const BLOCK_NODES: usize = 16;
const INDEX_SLOTS: usize = 512;

#[derive(Clone, Copy)]
struct Block {
    file: usize,
    entry: FlowId,
    next: FlowId,
    roots: [u64; BLOCK_NODES],
    flows: [FlowId; BLOCK_NODES],
    len: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct Jump {
    pub(crate) next: FlowId,
    pub(crate) steps: u32,
}

#[derive(Default)]
pub(crate) struct FlowIndex {
    slots: Vec<Option<Block>>,
    #[cfg(feature = "perf-counters")]
    counts: [u64; 12],
    #[cfg(feature = "perf-counters")]
    lengths: [u64; BLOCK_NODES + 1],
    #[cfg(feature = "perf-counters")]
    reported: std::cell::Cell<bool>,
}

impl FlowIndex {
    fn slot(file: usize, flow: FlowId) -> usize {
        (file.wrapping_mul(0x9e3779b1) ^ (flow.index() as usize).wrapping_mul(0x85ebca6b))
            & (INDEX_SLOTS - 1)
    }

    #[inline]
    fn bump(&mut self, counter: usize, n: u64) {
        #[cfg(feature = "perf-counters")]
        {
            self.counts[counter] += n;
        }
        #[cfg(not(feature = "perf-counters"))]
        let _ = (counter, n);
    }

    #[cfg(feature = "perf-counters")]
    pub(crate) fn step(&mut self, flags: FlowFlags) {
        self.counts[0] += 1;
        self.counts[1] += u64::from(flags.intersects(FlowFlags::ASSIGNMENT));
        self.counts[2] += u64::from(flags.intersects(FlowFlags::SHARED));
    }

    #[cfg(feature = "perf-counters")]
    pub(crate) fn report(&self) {
        let Some(directory) = std::env::var_os("TSRS_FLOW_INDEX_REPORT") else {
            return;
        };
        if self.reported.replace(true) {
            return;
        }
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!(
            "flow-index-{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(
            path,
            format!(
                "{{\"counts\":{:?},\"lengths\":{:?},\"index_bytes\":{}}}\n",
                self.counts,
                self.lengths,
                self.slots.capacity() * std::mem::size_of::<Option<Block>>()
            ),
        )
        .unwrap();
    }
}

fn identifier_key(symbol: SymbolId) -> u64 {
    u64::from(symbol.index()) * 2
}
fn declaration_key(symbol: SymbolId) -> u64 {
    identifier_key(symbol) + 1
}

impl CheckerState<'_> {
    /// No resolution is performed to prepare a query or a block. In speculative
    /// work the normal walker remains responsible for all once-state effects.
    pub(crate) fn flow_index_jump(&mut self, query: &FlowQuery, entry: FlowId) -> Option<Jump> {
        self.flow_index.bump(3, 1);
        if self.speculation_depth != 0
            || self.inline_level != 0
            || query.reduce_depth != 0
            || !self.reduce_label_overrides.is_empty()
            || !self.flow_loop_stack.is_empty()
            || !self.active_type_mappers.is_empty()
            || query.synthetic_props.is_some()
            || query.memo_faithful
            || self.kind_of(query.reference) != SyntaxKind::Identifier
            || self.is_this_in_type_query(query.reference)
        {
            return None;
        }
        let symbol = self
            .links
            .node(query.reference)
            .resolved_symbol
            .resolved()?;
        if symbol == self.unknown_symbol {
            return None;
        }
        self.flow_index.bump(4, 1);
        let identifier = identifier_key(symbol);
        let declaration =
            declaration_key(self.get_export_symbol_of_value_symbol_if_exported(symbol));
        let file = query.file_index();
        let slot = FlowIndex::slot(file, entry);
        let cached = self
            .flow_index
            .slots
            .get(slot)
            .copied()
            .flatten()
            .filter(|block| block.file == file && block.entry == entry);
        let block = if let Some(block) = cached {
            self.flow_index.bump(5, 1);
            block
        } else {
            self.flow_index.bump(6, 1);
            let block = self.build_flow_block(query, entry)?;
            if self.flow_index.slots.is_empty() {
                self.flow_index.slots.resize(INDEX_SLOTS, None);
            }
            self.flow_index.slots[slot] = Some(block);
            block
        };
        // Sorted (root, distance) entries locate the nearest possible write.
        let distance = |root: u64| {
            let i = block.roots[..block.len].partition_point(|&value| value < root * 16);
            if i < block.len && block.roots[i] / 16 == root {
                (block.roots[i] % 16) as usize
            } else {
                block.len
            }
        };
        let steps = distance(identifier).min(distance(declaration));
        if steps < 2 {
            return None;
        }
        self.flow_index.bump(8, 1);
        self.flow_index.bump(9, steps as u64);
        let next = if steps == block.len {
            block.next
        } else {
            block.flows[steps]
        };
        Some(Jump {
            next,
            steps: steps as u32,
        })
    }

    fn build_flow_block(&mut self, query: &FlowQuery, entry: FlowId) -> Option<Block> {
        let mut block = Block {
            file: query.file_index(),
            entry,
            next: entry,
            roots: [0; BLOCK_NODES],
            flows: [entry; BLOCK_NODES],
            len: 0,
        };
        while block.len < BLOCK_NODES {
            let flow = query.flow(block.next);
            // In particular CALL, SHARED, condition, join, loop, reduce-label,
            // array mutation and outer-container START are absolute barriers.
            if !flow.flags.intersects(FlowFlags::ASSIGNMENT)
                || !(flow.flags & !(FlowFlags::ASSIGNMENT | FlowFlags::REFERENCED)).is_empty()
            {
                self.flow_index.bump(10, 1);
                break;
            }
            self.flow_index.bump(7, 1);
            let node = flow.payload_node()?;
            let root = match self.kind_of(node) {
                SyntaxKind::Identifier => {
                    let Some(symbol) = self.links.node(node).resolved_symbol.resolved() else {
                        // A cold link may become eligible later. Do not cache it.
                        self.flow_index.bump(11, 1);
                        return None;
                    };
                    if symbol == self.unknown_symbol {
                        return None;
                    }
                    identifier_key(symbol)
                }
                SyntaxKind::VariableDeclaration => {
                    // for-in declarations can narrow the enumerable expression.
                    if self
                        .parent_of(node)
                        .and_then(|p| self.parent_of(p))
                        .is_some_and(|p| self.kind_of(p) == SyntaxKind::ForInStatement)
                    {
                        self.flow_index.bump(10, 1);
                        break;
                    }
                    declaration_key(self.get_symbol_of_declaration_opt(node)?)
                }
                _ => {
                    self.flow_index.bump(10, 1);
                    break;
                }
            };
            block.roots[block.len] = root * 16 + block.len as u64;
            block.flows[block.len] = block.next;
            block.len += 1;
            block.next = flow.antecedent()?;
        }
        block.roots[..block.len].sort_unstable();
        #[cfg(feature = "perf-counters")]
        {
            self.flow_index.lengths[block.len] += 1;
        }
        Some(block)
    }

    #[cfg(feature = "flow-memo-shadow")]
    pub(crate) fn verify_flow_index_jump(
        &mut self,
        query: &mut FlowQuery,
        entry: FlowId,
        jump: Jump,
    ) -> crate::state::CheckResult<()> {
        let before = self.flow_index_state_snapshot(query);
        let mut flow = entry;
        for _ in 0..jump.steps {
            assert!(
                self.get_type_at_flow_assignment(query, flow)?.is_none(),
                "indexed assignment changed the answer"
            );
            flow = query.antecedent(flow);
        }
        assert_eq!(flow, jump.next);
        assert_eq!(
            before,
            self.flow_index_state_snapshot(query),
            "indexed assignment changed checker state"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_support::with_program_state;
    use tsc_types::CompilerOptions;

    fn last_reference(state: &CheckerState<'_>, name: &str) -> NodeId {
        state
            .binder
            .source(0)
            .arena
            .node_ids()
            .filter(|&node| {
                state.kind_of(node) == SyntaxKind::Identifier
                    && state.escaped_text_of(Some(node)) == Some(name)
            })
            .max_by_key(|&node| state.pos_of(node))
            .unwrap()
    }

    #[test]
    fn straight_index_stops_before_the_nearest_relevant_write() {
        with_program_state(
            &[(
                "a.ts",
                "let focus: string | number = ''; let a = 1; let b = 2; focus;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                state.check_source_file(0);
                let reference = last_reference(state, "focus");
                let query = FlowQuery::for_memo_test(state, reference);
                let entry = state.flow_node_of(reference).unwrap();
                let jump = state
                    .flow_index_jump(&query, entry)
                    .expect("two unrelated declarations");
                assert_eq!(jump.steps, 2);
                let mut cursor = entry;
                for _ in 0..jump.steps {
                    let mut probe = FlowQuery::for_memo_test(state, reference);
                    assert!(state
                        .get_type_at_flow_assignment(&mut probe, cursor)
                        .unwrap()
                        .is_none());
                    cursor = query.antecedent(cursor);
                }
                assert_eq!(cursor, jump.next);
                let mut probe = FlowQuery::for_memo_test(state, reference);
                assert!(state
                    .get_type_at_flow_assignment(&mut probe, cursor)
                    .unwrap()
                    .is_some());
            },
        );
    }

    #[test]
    fn straight_index_preserves_call_and_shared_barriers() {
        with_program_state(&[("a.ts", "declare function stop(): void; let focus = ''; stop(); let a = 1; let b = 2; focus;\n")], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            let reference = last_reference(state, "focus");
            let query = FlowQuery::for_memo_test(state, reference);
            let entry = state.flow_node_of(reference).unwrap();
            let jump = state.flow_index_jump(&query, entry).expect("two declarations before the call barrier");
            {
                let mut cursor = entry;
                for _ in 0..jump.steps {
                    let flags = query.flow(cursor).flags;
                    assert!(flags.intersects(FlowFlags::ASSIGNMENT));
                    assert!(!flags.intersects(FlowFlags::SHARED | FlowFlags::CALL));
                    cursor = query.antecedent(cursor);
                }
                assert_eq!(cursor, jump.next);
                assert!(query.flow(cursor).flags.intersects(FlowFlags::CALL | FlowFlags::SHARED));
            }
            state.speculation_depth += 1;
            assert!(state.flow_index_jump(&query, entry).is_none());
            state.speculation_depth -= 1;
        });
    }
}
