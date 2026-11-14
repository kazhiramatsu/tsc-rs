use super::*;

#[test]
fn a_known_spelling_is_not_loaded_until_its_task_executes() {
    let first = ProgramPath::from_js_parts("/project/a.ts".into(), "/project/a.ts".into()).unwrap();
    let equal = ProgramPath::from_js_parts("/project/a.ts".into(), "/project/a.ts".into()).unwrap();
    let mut tasks = ParseTasks::default();
    let queue = |tasks: &mut ParseTasks, path| {
        tasks.new_task(
            path,
            TaskOrigin::Import,
            DiscoveryReason::dependency(SourceInclusionReason::Synthetic),
            SourceClass::Ordinary,
            false,
            false,
            0,
        )
    };
    let owner = queue(&mut tasks, Cow::Owned(first.clone()));
    tasks.tasks[owner].payload_mut().loaded = true;
    tasks.tasks[owner].payload_mut().source = Some(0);
    // Equal bytes in independent allocations and reused shared storage must
    // select the same spelling, while retaining the incoming task's state.
    for path in [&equal, &equal, &first] {
        let incoming = queue(&mut tasks, Cow::Borrowed(path));
        assert_eq!(tasks.tasks[incoming].data, tasks.tasks[owner].data);
        assert_eq!(tasks.path(incoming), path);
        assert!(!tasks.tasks[incoming].loaded());
        assert_eq!(tasks.tasks[incoming].source(), None);
        assert_eq!(tasks.tasks[incoming].loaded_task, None);
    }
}

#[test]
fn joined_resolution_shards_replace_old_rows_without_losing_other_entries() {
    let incoming = Sharded::new();
    for index in 0..1024 {
        incoming.insert(index, format!("new-{index}"));
    }
    let mut target = FxHashMap::default();
    target.insert(0, "old".to_owned());
    target.insert(2048, "retained".to_owned());
    incoming.append_to(&mut target);
    assert_eq!(target.len(), 1025);
    assert_eq!(target[&2048], "retained");
    for index in 0..1024 {
        assert_eq!(target[&index], format!("new-{index}"));
    }
}
