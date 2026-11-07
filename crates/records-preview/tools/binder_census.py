"""Static, source-pinned audit of per-node binder writes (no Rust call-graph claim).

Inventory is discovered from live Rust, including multiline/destructured receivers.
Hand-reviewed guards below are checked against pinned whole-file hashes, so a
changed guard cannot silently inherit an old kind classification. `any` means
no syntactic bound on the written key, even when a helper's name suggests one.
Dispatch evidence is separate from the writer's local bound.
"""
import hashlib
import re

MAP_FIELDS = {
    'node_symbol': 'symbol', 'node_local_symbol': 'local_symbol',
    'locals': 'locals', 'next_container': 'next_container', 'node_flow': 'flow',
    'node_end_flow': 'end_flow', 'node_return_flow': 'return_flow',
    'node_flow_when_true': 'flow_when_true', 'node_flow_when_false': 'flow_when_false',
    'possibly_exhaustive': 'possibly_exhaustive',
    'node_fallthrough_flow': 'fallthrough_flow', 'node_flags_mut': 'flags',
}
FILES = ('bind', 'declare', 'containers', 'flow', 'node_util')
WRITE_METHODS = {'insert', 'entry', 'remove', 'get_mut', 'values_mut', 'iter_mut',
                 'set', 'set_has_locals', 'extend', 'clear', 'retain', 'drain'}
READ_METHODS = {'get', 'contains_key', 'clone', 'len', 'heap_bytes', 'values'}
# Each bound concerns the MAP KEY, not some other node mentioned nearby.
AUDIT = {
    'bind:125': ('node_flow', ['Identifier', 'ThisKeyword'], 'bind_worker match kind_of(node), bind.rs:94-95.'),
    'bind:132': ('node_flow', ['QualifiedName'], 'bind_worker match arm, bind.rs:129; is_part_of_type_query is an additional predicate.'),
    'bind:138': ('node_flow', ['MetaProperty', 'SuperKeyword'], 'bind_worker match arm, bind.rs:136.'),
    'bind:145': ('node_flow', ['PropertyAccessExpression', 'ElementAccessExpression'], 'bind_worker match arm, bind.rs:142; is_narrowable_reference further restricts.'),
    'bind:195': ('node_flow', ['BindingElement'], 'bind_worker match arm, bind.rs:193.'),
    'bind:432': ('node_symbol', 'any', 'Key is self.source.root, the file root; enclosing branch tests .json file name, not a node kind.'),
    'bind:1021': ('node_symbol', 'any', 'Key is name from the collected names vector. JSDocOverloadTag guard is on tag, not on the written name.'),
    'bind:1057': ('node_flow', 'any', 'No local kind guard. is_async_function only controls emit flags, not this insertion.'),
    'bind:1086': ('node_flow', ['MethodDeclaration', 'GetAccessor', 'SetAccessor'], 'Dominating is_object_literal_or_class_expression_method_or_accessor guard; node_util.rs:1799-1816 matches these kinds and checks their parent.'),
    'bind:1578': ('locals', 'any', 'get_mut mutates table contents; root is self.source.root at bind.rs:1547.'),
    'bind:2115': ('node_flow', 'flow-capable', 'Dominating can_have_flow_node guard; node_util.rs:3095-3118 is statement range OR the listed expression kinds. Removal clears a fact.'),
    'bind:2143': ('node_flow', 'statements', 'Dominating FirstStatement <= kind <= LastStatement and can_have_flow_node; bind.rs:2137-2143, kind.rs:789/791.'),
    'bind:2981': ('possibly_exhaustive', 'any', 'Earlier SwitchStatement data match has a non-returning fallback, so does not bound this writer. Dispatch evidence recorded separately.'),
    'bind:3057': ('node_fallthrough_flow', 'any', 'Key is clause from a CaseBlock list; no kind guard on clause. Schema intent is CaseClause/DefaultClause, recorded separately.'),
    'bind:3543': ('node_flow_when_true', 'any', 'in_return_position guard is not a kind guard; ConditionalExpression data match has a non-returning fallback.'),
    'bind:3555': ('node_flow_when_false', 'any', 'in_return_position guard is not a kind guard; ConditionalExpression data match has a non-returning fallback.'),
    'declare:430': ('node_flags_mut', 'any', 'Generic set_has_locals helper; key index is derived from node.'),
    'declare:436': ('locals', 'any', 'Generic locals_entry returns a mutable table, inserting on absence.'),
    'declare:446': ('node_flags_mut', 'any', 'Generic set_flags_of helper; debug assertion bounds flag bits, not node kind.'),
    'declare:859': ('node_symbol', 'any', 'Generic add_declaration_to_symbol; no kind guard.'),
    'declare:1370': ('node_symbol', 'any', 'Identity relocation rewrites every stored symbol value through values_mut.'),
    'declare:1373': ('node_local_symbol', 'any', 'Identity relocation rewrites every stored symbol value through values_mut.'),
    'declare:1377': ('locals', 'any', 'Identity relocation rewrites all tables under has_private_serials; not a kind guard.'),
    'declare:1381': ('locals', 'any', 'Identity relocation rewrites all tables without private serials; not a kind guard.'),
    'containers:204': ('next_container', 'any', 'Key last comes from last_container, with no syntactic kind bound.'),
    'containers:213': ('locals', 'any', 'Generic ensure_locals inserts via Entry::Vacant(entry) at containers.rs:214.'),
    'containers:272': ('locals', 'any', 'IS_CONTAINER and HAS_LOCALS flags are caller-supplied; no dominating node-kind bound.'),
    'containers:281': ('locals', 'any', 'IS_BLOCK_SCOPED_CONTAINER and HAS_LOCALS are caller-supplied; remove clears a fact.'),
    'containers:351': ('node_end_flow', 'any', 'IS_FUNCTION_LIKE is caller-supplied; reachability/body checks do not syntactically bound node kind.'),
    'containers:358': ('node_end_flow', ['SourceFile'], 'Dominating kind_of(node) == SourceFile at containers.rs:356.'),
    'containers:375': ('node_return_flow', ['Constructor', 'ClassStaticBlockDeclaration', 'FunctionDeclaration', 'FunctionExpression'], 'Dominating kind equality OR matches! guard at containers.rs:366-373; functions additionally require JS file.'),
    'containers:646': ('node_local_symbol', 'any', 'declare_module_member branches on container/export flags, not the written node kind.'),
}
DISPATCH = {
    'bind:432': (['SourceFile'], 'Key self.source.root denotes the file root; structural evidence, not a dominating kind guard'),
    'bind:1578': (['SourceFile'], 'Key root = self.source.root at bind.rs:1547; structural evidence, not a dominating kind guard'),
    'bind:1057': (['FunctionExpression', 'ArrowFunction'], 'bind.rs:288-289 match in bind_worker'),
    'bind:2981': (['SwitchStatement'], 'bind.rs:2160 match in bind_children'),
    'bind:3057': (['CaseClause', 'DefaultClause'], 'CaseBlock clauses schema intent; not a Rust kind guard on clause'),
    'bind:3543': (['ConditionalExpression'], 'bind.rs:2176 match in bind_children'),
    'bind:3555': (['ConditionalExpression'], 'bind.rs:2176 match in bind_children'),
}


def code_only(text):
    # Preserve line/byte positions. These audited files contain no nested block
    # comments or raw string literals in map receivers; hashes pin that scope.
    return re.sub(r'//[^\n]*|/\*[\s\S]*?\*/|"(?:\\.|[^"\\])*"',
                  lambda m: re.sub(r'[^\n]', ' ', m[0]), text)


def inventory(root):
    sites = []
    for file in FILES:
        path = f'crates/binder/src/{file}.rs'
        code = code_only((root / path).read_text())
        for m in re.finditer(r'\b(' + '|'.join(MAP_FIELDS) + r')\s*\.\s*(\w+)\s*\(', code):
            method = m[2]
            if method not in WRITE_METHODS | READ_METHODS:
                raise ValueError(f'{path}: unaudited map method {m[1]}.{method}')
            if method in WRITE_METHODS:
                fn = list(re.finditer(r'\bfn\s+(\w+)\s*\(', code[:m.start()]))[-1][1]
                sites.append({'source': path, 'line': code[:m.start()].count('\n') + 1,
                              'map': m[1], 'method': method, 'function': fn})
        # Index assignments and standalone mutable borrows would evade a method
        # census. Fail closed if either appears; entry aliases are audited above.
        for name in MAP_FIELDS:
            if re.search(r'\b' + name + r'\s*\[[^\]]+\]\s*=(?!=)|&mut\s+(?:self\s*\.\s*)?' + name + r'\b', code):
                raise ValueError(f'{path}: new non-method writer for {name}')
    return sites


def derive(root, go, config):
    for file in FILES:
        path = f'crates/binder/src/{file}.rs'
        digest = hashlib.sha256((root / path).read_bytes()).hexdigest()
        if config['sourceSha256'].get(path) != digest:
            raise ValueError(f'{path}: Rust binder guard audit drift; re-audit before updating its hash')
    syntax = (root / 'crates/syntax/src/kind.rs').read_text()
    values = {m[1]: int(m[2]) for m in re.finditer(r'^    (\w+) = (\d+),', syntax, re.M)}
    first = re.search(r'pub const FirstStatement: Self = Self::(\w+);', syntax)[1]
    last = re.search(r'pub const LastStatement: Self = Self::(\w+);', syntax)[1]
    statements = [k for k, v in values.items() if values[first] <= v <= values[last]]
    flow_capable = statements + ['Identifier', 'ThisKeyword', 'SuperKeyword', 'QualifiedName', 'MetaProperty',
        'ElementAccessExpression', 'PropertyAccessExpression', 'BindingElement', 'FunctionExpression',
        'ArrowFunction', 'MethodDeclaration', 'GetAccessor', 'SetAccessor']
    matrix = {name: {'field': field, 'writers': []} for name, field in MAP_FIELDS.items()}
    seen = set()
    for site in inventory(root):
        key = site['source'].rsplit('/', 1)[-1][:-3] + ':' + str(site['line'])
        if key not in AUDIT or AUDIT[key][0] != site['map']:
            raise ValueError(f'Rust binder writer lacks a guard audit: {key}')
        seen.add(key)
        _, kinds, why = AUDIT[key]
        if kinds == 'statements':
            kinds = statements
        elif kinds == 'flow-capable':
            kinds = flow_capable
        site.update(kinds=kinds, why=why)
        if key in DISPATCH:
            site['dispatchKinds'], site['dispatchEvidence'] = DISPATCH[key]
        matrix[site.pop('map')]['writers'].append(site)
    if seen != set(AUDIT):
        raise ValueError('Rust binder writer removed; re-audit the matrix')
    all_go = dict(go['kinds'], **go['tokens'])
    for name, entry in matrix.items():
        field = entry['field']
        go_kinds = [k for k, r in all_go.items() if any(f['field'] == field for f in r['fields'])]
        known = set()
        for site in entry['writers']:
            known.update(site['kinds'] if site['kinds'] != 'any' else [])
            known.update(site.get('dispatchKinds', []))
        missing = sorted(known - set(go_kinds)) if field != 'flags' else []
        entry.update(goKinds=go_kinds, rustKnownKinds=sorted(known),
                     hasUnboundedWriter=any(s['kinds'] == 'any' for s in entry['writers']),
                     missingTailKinds=missing)
        entry['status'] = 'port-specific' if missing or not go_kinds and field != 'flags' else 'header-backed' if field == 'flags' else 'Go-backed'
        entry['placement'] = ('Node.flags header word on every kind; no extra tail word.' if field == 'flags' else
            'Inline only on goKinds listed above. Retain the Rust per-file map for every other written kind; '
            'missingTailKinds lists known differences, and any is NOT restricted to goKinds. '
            'No Rust fact is discarded; this preview does not migrate the binder or price retained maps.')
    return {'why': 'Source-pinned static writer/guard census; local kind bounds and dispatch evidence are separate. '
                   'Mutating table contents, removals and identity relocation are included; initialization/publication copies are not per-node writes.',
            'sources': config['sourceSha256'], 'writerCount': len(seen), 'maps': matrix}
