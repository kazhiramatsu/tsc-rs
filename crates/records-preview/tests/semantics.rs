use tsc_records_preview::{
    arena_stub::{Arena, ArenaCell, List, Ref, SharedArena, Span},
    nodes::*,
    records::*,
    support::*,
    tables::FlatSymbolTable,
};

#[test]
fn optional_spans_preserve_absent_and_present_empty() {
    let absent = OptionalSpan::<u32>::encode(None);
    let empty = OptionalSpan::encode(Some(Span::<u32>::default()));
    assert_eq!(absent.len_plus_one, 0);
    assert_eq!(empty.len_plus_one, 1);
    assert!(absent.decode().is_none());
    assert_eq!(empty.decode().unwrap().len, 0);
    assert_eq!(std::mem::size_of_val(&absent), 8);
}
#[test]
fn packed_slots_keep_resolving_and_negative_results_distinct() {
    let cell = ArenaCell::new(Slot::<Type>::default());
    assert!(matches!(cell.get().state(), SlotState::Vacant));
    cell.set(Slot::encode(SlotState::Resolving));
    assert!(matches!(cell.get().state(), SlotState::Resolving));
    cell.set(Slot::encode(SlotState::Resolved(Id::default())));
    assert!(matches!(cell.get().state(),SlotState::Resolved(id) if id.is_none()));
    cell.set(Slot::encode(SlotState::Resolved(Id::from_raw(31))));
    assert!(matches!(cell.get().state(),SlotState::Resolved(id) if id.raw()==31));
}
#[test]
#[should_panic(expected = "reserved slot handle")]
fn slot_rejects_collision_with_reserved_state() {
    let _ = Slot::<Type>::encode(SlotState::Resolved(Id::from_raw(u32::MAX)));
}
#[test]
fn type_ids_are_dense_creation_numbers_separate_from_arena_handles() {
    let checker = CheckerArena::new(2);
    let arena = &checker.arena;
    let _ = arena.alloc_bytes(&[4; 200]);
    let a = checker.alloc_type_object(TypeObjectTail::default());
    let _ = arena.alloc_list(&[9u32, 8, 7]);
    let b =
        checker.alloc_type_reference(tsc_records_preview::records::TypeReferenceTail::default());
    assert_eq!(arena.get(a).header.id, 1);
    assert_eq!(arena.get(b).header.id, 2);
    assert_ne!(a.raw(), 1);
    let record = arena.get(a);
    assert!(std::ptr::eq(record.tail(), &record.tail));
    record
        .tail()
        .resolved_members
        .set(Slot::encode(SlotState::Resolving));
    assert!(matches!(
        record.tail().resolved_members.get().state(),
        SlotState::Resolving
    ));
}
#[test]
fn instantiated_symbol_borrows_name_and_declarations_from_origin() {
    let arena = Arena::new(3);
    let more = arena.alloc_slice(&[Id::<Node>::from_raw(22), Id::from_raw(23)]);
    let origin = arena.alloc(Symbol {
        name: 55,
        decl0: Id::from_raw(21),
        more: OptionalSpan::encode(Some(more)),
        ..Symbol::default()
    });
    let transient = TransientSymbol {
        flags: TransientSymbol::INSTANTIATED,
        origin: Id::from_ref(origin),
        ..TransientSymbol::default()
    };
    assert_eq!(transient.name(&arena), 55);
    let declarations = transient.declarations(&arena);
    assert_eq!(
        declarations.iter().map(Id::raw).collect::<Vec<_>>(),
        [21, 22, 23]
    );
    assert_eq!(transient.origin_record(&arena).decl0.raw(), 21);
}
#[test]
fn candidate_pairs_borrow_inline_and_retain_overflow() {
    let arena = Arena::new(1);
    for len in 0..=6 {
        let values: Vec<Id<Type>> = (1..=len).map(Id::from_raw).collect();
        let pair = CandidatePair::new(&arena, &values);
        {
            let view = pair.values(&arena);
            assert_eq!(&*view, &values);
            if len > 0 && len <= 2 {
                assert!(std::ptr::eq(&view[0], &pair.inline[0]));
            }
        }
        let cloned = pair.deep_clone(&arena);
        assert_eq!(&*cloned.values(&arena), &values);
        if len > 2 {
            assert_ne!(pair.overflow.start, cloned.overflow.start);
        }
    }
}
#[test]
fn mapper_all_six_shapes_and_all_deferred_forms() {
    let samples = [
        MapperData::Simple {
            source: Id::from_raw(7),
            target: Id::from_raw(9),
        },
        MapperData::Array {
            sources: Span::from_raw(11, 3),
            targets: Some(Span::from_raw(21, 3)),
        },
        MapperData::Array {
            sources: Span::from_raw(11, 3),
            targets: None,
        },
        MapperData::Array {
            sources: Span::default(),
            targets: Some(Span::default()),
        },
        MapperData::DeferredFixing(Id::from_raw(42)),
        MapperData::DeferredNonFixing(Id::from_raw(43)),
        MapperData::DeferredEffective {
            node: Id::from_raw(5),
            sources: Span::from_raw(23, 4),
        },
        MapperData::Function(4),
        MapperData::Composite(Id::from_raw(8), Id::from_raw(9)),
        MapperData::Merged(Id::from_raw(9), Id::from_raw(8)),
    ];
    let expected = [
        (0, 7, 9, 0),
        (129, 11, 21, 3),
        (1, 11, 0, 3),
        (129, 0, 0, 0),
        (2, 42, 0, 0),
        (10, 43, 0, 0),
        (18, 5, 23, 4),
        (3, 4, 0, 0),
        (4, 8, 9, 0),
        (5, 9, 8, 0),
    ];
    for (data, words) in samples.into_iter().zip(expected) {
        let record = Mapper::encode(data);
        assert_eq!((record.kind, record.a, record.b, record.c), words);
        assert_eq!(Mapper::encode(record.decode().unwrap()), record);
    }
    assert!(Mapper {
        kind: 255,
        ..Mapper::default()
    }
    .decode()
    .is_none());
}
#[test]
fn tables_switch_at_seventeen_entries_and_preserve_insertion_order() {
    for len in [0, 1, 16, 17, 80] {
        let entries: Vec<_> = (0..len)
            .map(|i| TableEntry {
                name: i * 128 + 13,
                symbol: Id::from_raw(200 + i),
            })
            .collect();
        let table = FlatSymbolTable::new(&entries);
        assert_eq!(table.len(), len as usize);
        assert_eq!(table.flags() != 0, len > 16);
        assert_eq!(table.entries().collect::<Vec<_>>(), entries);
        for entry in &entries {
            assert_eq!(table.get(entry.name), Some(entry.symbol));
        }
        assert_eq!(table.get(99999), None);
        assert!(table.words().len() >= 2 + len as usize * 2);
    }
}
#[test]
fn flow_label_span_keeps_the_sixteen_byte_record() {
    let arena = Arena::new(5);
    let span = arena.alloc_slice(&[Id::<Flow>::from_raw(1), Id::from_raw(2)]);
    let flow = Flow::label(4, span);
    assert_eq!(std::mem::size_of_val(&flow), 16);
    assert_eq!(
        &*flow.label_antecedents().as_slice(&arena),
        &[Id::from_raw(1), Id::from_raw(2)]
    );
}
#[test]
fn optional_text_and_jsdoc_zero_text_are_lossless() {
    let mut data = NodeData::missing(SyntaxKind::TemplateHead)
        .as_template_head()
        .unwrap();
    data.raw_text = Some(TextRef(0));
    let record = TemplateHeadRecord::new(0, 0, 0, &data);
    assert_eq!(record.data(), data);
    data.raw_text = None;
    assert_eq!(TemplateHeadRecord::new(0, 0, 0, &data).data(), data);
    for comment in [
        None,
        Some(JSDocComment::Text {
            text: TextRef(0),
            pos: 0,
            end: 0,
            text_end: 0,
        }),
        Some(JSDocComment::Text {
            text: TextRef(u32::MAX),
            pos: 3,
            end: 9,
            text_end: 7,
        }),
    ] {
        let data = JSDocData {
            tags: None,
            comment,
        };
        assert_eq!(JSDocRecord::new(0, 0, 0, &data).data(), data);
    }
}
#[test]
fn binder_flow_and_observable_field_order() {
    let mut identifier = IdentifierRecord::new(
        0,
        1,
        0,
        &IdentifierData {
            escaped_text: TextRef(8),
        },
    );
    let flow = Ref::from_raw(7).unwrap();
    identifier.set_flow(Some(flow));
    assert_eq!(identifier.flow(), Some(flow));
    let mut names = Vec::new();
    identifier.observable_fields(|name, _| names.push(name));
    assert_eq!(names, ["escapedText"]);
    let call = NodeData::missing(SyntaxKind::CallExpression)
        .as_call_expression()
        .unwrap();
    names.clear();
    CallExpressionRecord::new(0, 0, 0, &call).observable_fields(|name, _| names.push(name));
    assert_eq!(
        names,
        [
            "expression",
            "questionDotToken",
            "typeArguments",
            "arguments"
        ]
    );
    let binary = NodeData::missing(SyntaxKind::BinaryExpression)
        .as_binary_expression()
        .unwrap();
    names.clear();
    BinaryExpressionRecord::new(0, 0, 0, &binary).observable_fields(|name, _| names.push(name));
    assert_eq!(names, ["left", "operatorToken", "right"]);
}
#[test]
fn shared_stub_mutation_freezing_lists_and_tagged_text() {
    let mut arena = SharedArena::new(4);
    let r = arena.alloc(Node::default());
    arena.get_mut(r).flags = 37;
    let list = arena.alloc_list(&[19u32, 23]);
    let text = arena.alloc_bytes(b"text");
    assert_eq!(TextRef::bytes(text).bytes_ref(), Some(text));
    assert_eq!(TextRef::source(42).source_index(), Some(42));
    assert_eq!(&*arena.list_items(list), &[19, 23]);
    assert_eq!(arena.list_header(list).len, 2);
    assert_eq!(&*arena.bytes(text), b"text");
    assert_eq!(arena.freeze().get(r).flags, 37);
    assert_eq!(std::mem::size_of::<Option<Ref<List<u32>>>>(), 4);
}

#[test]
fn inference_metadata_preserves_circularity_and_optional_zero() {
    let info = InferenceInfo::default();
    assert_eq!(info.priority(), None);
    info.set_priority(Some(-1));
    assert_eq!(info.priority(), Some(-1));
    info.set_priority(Some(0));
    assert_eq!(info.priority(), Some(0));
    info.set_priority(None);
    assert_eq!(info.priority(), None);
    info.set_implied_arity(Some(0));
    assert_eq!(info.implied_arity(), Some(0));
    info.set_implied_arity(Some(u32::MAX as usize));
    assert_eq!(info.implied_arity(), Some(u32::MAX));
    info.set_implied_arity(None);
    assert_eq!(info.implied_arity(), None);
}

#[test]
fn direct_walk_stops_on_first_match_or_error_and_rewrites_slots() {
    struct Lookup;
    impl PreviewLookup for Lookup {
        fn kind(&self, _: NodeId) -> SyntaxKind {
            SyntaxKind::Identifier
        }
        fn list(&self, _: ListId) -> &[NodeId] {
            &[]
        }
    }
    let data = BinaryExpressionData {
        left: Ref::from_raw(1),
        operator_token: Ref::from_raw(2),
        right: Ref::from_raw(3),
    };
    let mut record = BinaryExpressionRecord::new(0, 0, 0, &data);
    let mut visited = Vec::new();
    assert_eq!(
        record
            .for_each_child(&Lookup, |id| {
                visited.push(id.raw());
                true
            })
            .unwrap()
            .raw(),
        1
    );
    assert_eq!(visited, [1]);
    let mut calls = 0;
    assert_eq!(
        record.try_visit(&Lookup, |_| {
            calls += 1;
            Err("stop")
        }),
        Err("stop")
    );
    assert_eq!(calls, 1);
    record.apply_child_slots(
        |_| SyntaxKind::Identifier,
        |slot| match slot {
            ChildSlot::Node(name, Some(id)) => ChildSlot::Node(name, Ref::from_raw(id.raw() + 100)),
            other => other,
        },
    );
    assert_eq!(record.data().left.unwrap().raw(), 101);
    assert_eq!(record.data().right.unwrap().raw(), 103);
    assert_eq!(NodeView::BinaryExpression(&record).flow(), None);
}
