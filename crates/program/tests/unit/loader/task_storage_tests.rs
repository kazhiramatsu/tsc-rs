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
