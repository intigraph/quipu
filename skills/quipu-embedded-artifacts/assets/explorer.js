// A small read-only presentation over the real released explorer engine.
const node = (tag, text) => {
  const element = document.createElement(tag);
  if (text !== undefined) element.textContent = text;
  return element;
};
const iri = value => {
  if (!/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(value) || /[\x00-\x20<>"{}|^`\\]/.test(value)) {
    throw new Error('Invalid node IRI');
  }
  return `<${value}>`;
};

export async function attachExplorer(root) {
  root.classList.add('quipu-explorer');
  const heading = node('h3', 'Explore the brief');
  const status = node('p', 'Verifying the bundled graph…');
  status.setAttribute('role', 'status');
  const selection = node('h4', 'Choose a priority or an ask above');
  selection.tabIndex = -1;
  const facts = node('dl'); facts.className = 'quipu-facts';
  const details = node('details'); details.append(node('summary', 'Query this copy'));
  const queryLabel = node('label', 'Read-only SPARQL');
  const queryInput = node('textarea'); queryInput.id = 'quipu-query';
  queryLabel.htmlFor = queryInput.id;
  queryInput.value = 'SELECT ?item ?name ?rank WHERE { ?item <https://schema.org/position> ?rank ; <https://schema.org/name> ?name } ORDER BY ?rank';
  const run = node('button', 'Run query'); run.disabled = true;
  const desk = node('button', 'All Desk cards'); desk.disabled = true;
  const output = node('pre');
  const provenance = node('details'); provenance.append(node('summary', 'Snapshot and verification'));
  const receipt = node('pre'); provenance.append(receipt);
  details.append(queryLabel, queryInput, run, output);
  root.replaceChildren(heading, desk, status, selection, facts, details, provenance);
  const worker = new Worker('./worker.js', { type: 'module' });
  let sequence = 0; const pending = new Map();
  worker.onmessage = ({ data }) => {
    const call = pending.get(data.id);
    if (!call) return;
    pending.delete(data.id); clearTimeout(call.timer);
    data.ok ? call.resolve(data.result) : call.reject(new Error(data.error));
  };
  worker.onerror = event => {
    for (const call of pending.values()) {clearTimeout(call.timer);call.reject(new Error(event.message));}
    pending.clear();
  };
  const ask = message => new Promise((resolve, reject) => {
    const id = ++sequence;
    const timer = setTimeout(() => {pending.delete(id);reject(new Error('Explorer response deadline'));}, 30000);
    pending.set(id, {resolve, reject, timer});
    worker.postMessage({id, ...message});
  });
  let snapshot; let verified = false; let selectionNumber = 0;
  const showError = error => {status.textContent = error.message;status.classList.add('quipu-error');};
  const open = async (entity, title = entity) => {
    const current = ++selectionNumber;
    facts.replaceChildren(); selection.textContent = title; delete selection.dataset.entity;
    if (!verified) {status.textContent = 'The bundled graph is not ready yet.';return;}
    try {
      const query = `SELECT ?predicate ?value WHERE { ${iri(entity)} ?predicate ?value } ORDER BY ?predicate`;
      const result = await ask({cmd:'query', sparql:query});
      if (current !== selectionNumber) return;
      if (!result.rows?.length) throw new Error('This node has no bundled facts. The snapshot needs a refresh.');
      selection.dataset.entity = entity; queryInput.value = query;
      for (const row of result.rows) {
        facts.append(node('dt', row.predicate));
        const value = row.value && typeof row.value === 'object' ? JSON.stringify(row.value) : String(row.value);
        const dd = node('dd');
        if (/^(https?:|urn:)/.test(value)) {
          const link = node('button', value); link.onclick = () => open(value); dd.append(link);
        } else dd.textContent = value;
        facts.append(dd);
      }
      if (snapshot.omissions?.some(record => record.entity === entity)) {
        facts.append(node('dt', 'Privacy'), node('dd', 'Description withheld because it contains private identifiers.'));
      }
      status.classList.remove('quipu-error');
      status.textContent = `Snapshot ${snapshot.generated_at}. Selected fields and one-hop links; live rows may be newer.`;
      root.scrollIntoView({behavior:'instant', block:'start'}); selection.focus({preventScroll:true});
    } catch (error) {if (current === selectionNumber) showError(error);}
  };
  const openItem = (id, title) => {
    const entity = snapshot?.items[id];
    if (!entity) {
      selectionNumber++; facts.replaceChildren(); delete selection.dataset.entity;
      selection.textContent = title || id;
      showError(new Error('This live row has no bundled record. Ask for a snapshot refresh.'));return;
    }
    return open(entity, title || id);
  };
  run.onclick = async () => {
    run.disabled = true;
    try {output.textContent = JSON.stringify(await ask({cmd:'query',sparql:queryInput.value}), null, 2);}
    catch (error) {output.textContent = error.message;}
    finally {run.disabled = false;}
  };
  desk.onclick = async () => {
    const current = ++selectionNumber; facts.replaceChildren();
    selection.textContent = 'All Desk cards'; delete selection.dataset.entity;
    try {
      const sparql = "SELECT ?document ?title ?item WHERE { ?document <https://schema.org/category> 'Decision Desk' ; <http://www.w3.org/2000/01/rdf-schema#label> ?title ; <https://schema.org/about> ?item }";
      const result = await ask({cmd:'query', sparql});
      if (current !== selectionNumber) return;
      if (result.rows?.length !== snapshot.desk_count) throw new Error('Desk query count differs from generated coverage');
      for (const row of result.rows) {
        const dd = node('dd'); const link = node('button', row.title);
        link.onclick = () => open(row.item, row.title);
        dd.append(link); facts.append(node('dt', 'Desk'), dd);
      }
      status.classList.remove('quipu-error');
      status.textContent = `${result.rows.length} Desk cards in snapshot ${snapshot.generated_at}.`;
      root.scrollIntoView({behavior:'instant',block:'start'});selection.focus({preventScroll:true});
    } catch(error) {if(current === selectionNumber) showError(error);}
  };
  try {
    const fetchJSON = async url => {
      const response = await fetch(url); if (!response.ok) throw new Error(`Asset HTTP ${response.status}: ${url}`);
      return response.json();
    };
    const [encoded, metadata, version] = await Promise.all([
      fetchJSON('./pendant.json'), fetchJSON('./snapshot.json'), ask({cmd:'version'}),
    ]);
    if (encoded.encoding !== 'base64' || typeof encoded.data !== 'string') throw new Error('Unknown pendant transport');
    const bytes = Uint8Array.from(atob(encoded.data), character => character.charCodeAt(0));
    // The native receiving ceremony verifies the manifest and each payload hash.
    const report = await ask({cmd:'load', bytes});
    snapshot = metadata; verified = true; run.disabled = false;
    desk.disabled = false; desk.textContent = `All Desk cards (${snapshot.desk_count})`;
    receipt.textContent = JSON.stringify({version, snapshot:metadata, report}, null, 2);
    status.textContent = `Snapshot ${snapshot.generated_at} · ${snapshot.selected_items} selected items · verified. Choose a row to explore.`;
    root.dataset.ready = 'true';
  } catch (error) {showError(error);root.dataset.ready = 'false';}
  return {openItem, open, openDesk:() => desk.click(), deskCount:snapshot?.desk_count,
    query:sparql => ask({cmd:'query',sparql})};
}
