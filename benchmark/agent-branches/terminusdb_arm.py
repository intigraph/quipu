"""The TerminusDB arm: the same scenario through TerminusDB's documented
branch-and-merge workflow (https://terminusdb.com/docs/merge-howto/).

- The shared memory graph is a database whose schema mirrors the bench shapes:
  label/status/owner/version are `Optional` (at most one), tag is a `Set` with
  `@max_cardinality 4`, note and relatedTo are unbounded `Set`s.
- Each agent gets a branch from main at the base commit and writes documents
  on it (one commit per write). The live writer commits to main meanwhile.
- Each branch is merged with `POST /api/apply` and `before_commit` = the base
  commit — the documented common-ancestor form, so main's later changes are
  not reverted. A 409 conflict (with witnesses) or a 400 schema failure (a
  cardinality overflow the merged result would commit) is a question for the
  person, who takes the branch's value — the same ruling every arm gets — on
  main, and the apply is retried.

What cannot be expressed natively is scored, not skipped: TerminusDB has commit
time only, so a back-dated correction is an ordinary commit (its valid time
goes into the commit message), and the valid-time probes are answered from
main's HEAD — the most charitable native reading. Commit-time travel to the
probe date would answer from before the database existed.
"""
import base64
import json
import urllib.error
import urllib.parse
import urllib.request

import scoring

SCHEMA_PREDICATES = ('label', 'status', 'owner', 'version', 'tag', 'note', 'relatedTo')


class Tdb:
    def __init__(self, url, user, password, db):
        self.url = url.rstrip('/')
        self.auth = 'Basic ' + base64.b64encode(f'{user}:{password}'.encode()).decode()
        self.db = db
        self.requests = 0

    def call(self, method, path, body=None, query=''):
        self.requests += 1
        req = urllib.request.Request(
            self.url + path + query, method=method,
            data=None if body is None else json.dumps(body).encode(),
            headers={'Authorization': self.auth, 'Content-Type': 'application/json'})
        try:
            with urllib.request.urlopen(req, timeout=120) as r:
                raw = r.read()
                status = r.status
        except urllib.error.HTTPError as e:
            raw = e.read()
            status = e.code
        text = raw.decode() if raw else ''
        try:
            return status, json.loads(text) if text else None
        except json.JSONDecodeError:
            # GET /api/document without as_list streams one JSON per line.
            return status, [json.loads(line) for line in text.splitlines() if line.strip()]

    def doc_path(self, ref):
        return f'/api/document/admin/{self.db}/local/{ref}'

    def head(self, branch='main'):
        status, log = self.call('GET', f'/api/log/admin/{self.db}/local/branch/{branch}', query='?count=1')
        assert status == 200, log
        return log[0]['identifier']

    def docs(self, ref='branch/main'):
        status, docs = self.call('GET', self.doc_path(ref), query='?type=Entity&as_list=true')
        assert status == 200, docs
        return {d['slug']: d for d in docs}

    def put(self, branch, doc, message):
        status, body = self.call('PUT', self.doc_path(f'branch/{branch}'), doc,
                                 f'?author=agentbranches&message={urllib.parse.quote(message)}')
        if status != 200:
            raise RuntimeError(f'write refused on {branch}: {status} {body}')


def as_doc(slug, fields):
    doc = {'@id': f'Entity/{slug}', '@type': 'Entity', 'slug': slug}
    for p in SCHEMA_PREDICATES:
        vs = fields.get(p, [])
        if p in ('tag', 'note', 'relatedTo'):
            doc[p] = sorted(vs)
        elif vs:
            doc[p] = vs[0]
    return doc


def doc_slots(docs):
    out = {}
    for slug, d in docs.items():
        for p in SCHEMA_PREDICATES:
            v = d.get(p)
            if v is None or v == []:
                continue
            out[(slug, p)] = frozenset(v if isinstance(v, list) else [v])
    return out


def apply_op(doc, op):
    p, v = op['predicate'], op['value']
    if op['kind'] in ('set', 'correct'):
        doc[p] = v
    elif op['kind'] == 'clear':
        doc.pop(p, None)
    else:
        doc[p] = sorted(set(doc.get(p, [])) | {v})


def schema(bounded):
    def setof(p):
        s = {'@type': 'Set', '@class': 'xsd:string'}
        if bounded.get(p):
            s['@max_cardinality'] = bounded[p]
        return s
    cls = {'@type': 'Class', '@id': 'Entity',
           '@key': {'@type': 'Lexical', '@fields': ['slug']},
           'slug': 'xsd:string'}
    for p in ('label', 'status', 'owner', 'version'):
        cls[p] = {'@type': 'Optional', '@class': 'xsd:string'}
    for p in ('tag', 'note', 'relatedTo'):
        cls[p] = setof(p)
    return [{'@type': '@context', '@base': 'terminusdb:///data/',
             '@schema': 'terminusdb:///schema#'}, cls]


def witnessed_slots(status, body):
    """The (entity, predicate) slots a failed apply puts to a person."""
    slots = set()
    if status == 409:
        for w in body.get('api:witnesses', []):
            slug = str(w.get('@id', '')).rsplit('/', 1)[-1]
            fields = [k for k in w if not k.startswith('@')]
            for f in fields or ['*']:
                slots.add((slug, f))
    elif status == 400:
        for w in body.get('system:witnesses', []) or body.get('api:witnesses', []):
            doc = w.get('document', {})
            slug = str(doc.get('@id', '')).rsplit('/', 1)[-1]
            field = str(w.get('field', '*')).rsplit('#', 1)[-1]
            slots.add((slug, field))
    return slots


def manual_merge(tdb, a, branch, base_docs, ruled):
    """Land a branch by hand, the way a careful person would: a three-way merge
    per field the branch changed since base. A Set keeps main's members and
    applies the branch's own additions and removals; a single value takes the
    branch's value, and is a question when main changed it too; a slot already
    ruled on takes the branch's value."""
    mine = tdb.docs(f'branch/{branch}')
    main = tdb.docs()
    for slug, doc in sorted(mine.items()):
        b = base_docs.get(slug, {})
        target = dict(main.get(slug, {'@id': f'Entity/{slug}', '@type': 'Entity', 'slug': slug}))
        changed = False
        for p in SCHEMA_PREDICATES:
            bv, tv = b.get(p), doc.get(p)
            if bv == tv or (isinstance(bv, list) and sorted(bv) == sorted(tv or [])):
                continue
            changed = True
            if isinstance(tv, list) and (slug, p) not in ruled:
                ov = set(target.get(p, []))
                target[p] = sorted((ov - (set(bv or []) - set(tv))) | (set(tv) - set(bv or [])))
            else:
                # Both sides changed a single value since base: a question the
                # apply never got far enough to report. The person asks it
                # (counted) and rules the same way.
                ov = target.get(p)
                if ov != bv and ov != tv:
                    ruled.add((slug, p))
                if tv is None or tv == []:
                    target.pop(p, None)
                else:
                    target[p] = tv
        if not changed:
            continue
        for _ in range(len(SCHEMA_PREDICATES) + 1):
            status, body = tdb.call('PUT', tdb.doc_path('branch/main'), target,
                                    f'?author=agentbranches&message=person+lands+{branch}+by+hand')
            if status == 200:
                break
            # A further overflow the apply never got far enough to report:
            # one more question, ruled the same way.
            slots = witnessed_slots(400, body.get('api:error', body))
            if not slots or not slots - ruled:
                raise RuntimeError(f'manual merge of {branch} refused: {status} {body}')
            for s, p in slots - ruled:
                ruled.add((s, p))
                if doc.get(p) in (None, []):
                    target.pop(p, None)
                else:
                    target[p] = doc[p]


def run(scenario, url, user, password, db, log=print):
    bound = scoring.bounds(scenario)
    tdb = Tdb(url, user, password, db)
    tdb.call('DELETE', f'/api/db/admin/{db}')
    status, body = tdb.call('POST', f'/api/db/admin/{db}',
                            {'label': db, 'comment': 'agent-branches benchmark', 'schema': True})
    assert status == 200, body
    status, body = tdb.call('POST', f'/api/document/admin/{db}', schema(scenario['bounded']),
                            '?graph_type=schema&author=agentbranches&message=schema&full_replace=true')
    assert status == 200, body
    base_docs = [as_doc(slug, fields) for slug, fields in sorted(scenario['base'].items())]
    status, body = tdb.call('POST', f'/api/document/admin/{db}', base_docs,
                            '?author=agentbranches&message=base')
    assert status == 200, body
    base_commit = tdb.head()
    base = doc_slots(tdb.docs())

    n = len(scenario['agents'])
    for a in range(n):
        status, body = tdb.call('POST', f'/api/branch/admin/{db}/local/branch/agent-{a}',
                                {'origin': f'admin/{db}/local/branch/main'})
        assert status == 200, body

    def write(branch, op):
        status, doc = tdb.call('GET', tdb.doc_path(f'branch/{branch}'),
                               query=f'?id=Entity/{op["entity"]}')
        assert status == 200, doc
        apply_op(doc, op)
        msg = f'{op["writer"]} {op["kind"]} {op["entity"]}.{op["predicate"]}'
        if op.get('valid_from'):
            msg += f' (valid from {op["valid_from"]})'
        tdb.put(branch, doc, msg)

    for op in scenario['root']:
        write('main', op)
    for a, ops in enumerate(scenario['agents']):
        for op in ops:
            write(f'agent-{a}', op)

    promotions = []
    landed = {}
    errors = []
    manual = 0
    base_docs_by_slug = {d['slug']: d for d in base_docs}
    states = [doc_slots(tdb.docs())]
    for a in range(n):
        branch = f'agent-{a}'
        ours, theirs = doc_slots(tdb.docs()), doc_slots(tdb.docs(f'branch/{branch}'))
        need = scoring.needed(base, ours, theirs, bound)
        landed[a] = tdb.head()
        asked, rounds = set(), 0
        while True:
            rounds += 1
            status, body = tdb.call('POST', f'/api/apply/admin/{db}/local/branch/main', {
                'before_commit': base_commit, 'after_commit': branch,
                'commit_info': {'author': 'agentbranches', 'message': f'merge {branch}'}})
            if status == 200:
                break
            slots = witnessed_slots(status, body)
            if slots and slots <= asked:
                # The person already ruled on every witnessed slot (Option A),
                # and the retry still fails: a Set patch re-inserts the values
                # it adds, so a ruled cardinality overflow never re-applies.
                # The person lands the branch by hand instead.
                manual_merge(tdb, a, branch, base_docs_by_slug, asked)
                manual += 1
                break
            if not slots or rounds > 6:
                errors.append({'agent': a, 'status': status, 'body': body})
                break
            asked |= slots
            mine = tdb.docs(f'branch/{branch}')
            main = tdb.docs()
            for slug in sorted({s for s, _ in slots}):
                doc = main[slug]
                for s, p in slots:
                    if s != slug:
                        continue
                    if p == '*':
                        doc = dict(mine[slug])
                    elif p in mine[slug]:
                        doc[p] = mine[slug][p]
                    else:
                        doc.pop(p, None)
                tdb.put('main', doc, f'person rules for {branch} on {slug}')
        promotions.append({'asked': asked, 'needed': need, 'rounds': rounds})
        states.append(doc_slots(tdb.docs()))

    final = states[-1]
    questions = sum(len(p['asked']) for p in promotions)
    needed_n = sum(len(p['needed']) for p in promotions)
    caught = sum(len(p['asked'] & p['needed']) for p in promotions)
    writes = len(scenario['root']) + sum(len(ops) for ops in scenario['agents'])
    metrics = {
        'arm': 'terminusdb-apply', 'runs_on': 'terminusdb',
        'questions': questions, 'needed': needed_n, 'caught': caught,
        'missed': needed_n - caught, 'unnecessary': questions - caught,
        'writes': writes,
        'silent_losses': scoring.silent_losses(scenario, states, [p['asked'] for p in promotions]),
        'violations': scoring.violations(final, bound),
        'rounds': sum(p['rounds'] for p in promotions),
        'errors': errors,
        'manual_merges': manual,
        'http_requests': tdb.requests,
    }

    probes = []
    head = {s: d for s, d in tdb.docs().items()}
    for c in scenario['corrections']:
        old, new = [c['old']], [c['new']]

        def value(doc):
            v = doc.get(c['predicate'])
            return [] if v is None else sorted(v if isinstance(v, list) else [v])
        status, then = tdb.call('GET', tdb.doc_path(f'commit/{landed[c["agent"]]}'),
                                query=f'?id=Entity/{c["entity"]}')
        assert status == 200, then
        now = value(head[c['entity']])
        for name, expected, got in (
                ('valid-before', old, now),
                ('valid-after', new, now),
                ('current', new, now),
                ('belief-before-landing', old, value(then))):
            probes.append({'arm': 'terminusdb-apply', 'agent': c['agent'], 'probe': name,
                           'expected': expected, 'got': got, 'ok': got == expected})
    tdb.call('DELETE', f'/api/db/admin/{db}')
    return metrics, probes
