const el = id => document.getElementById(id);
let selected = null, cursor = 0, generation = 0, inFlight = false, more = false;
const seen = new Set();
let humanPosting = false, revision = null, pending = null, sending = false;
const authors = new Map();
async function request(path) { const r = await fetch(path); const body = await r.json(); if (!r.ok) throw new Error(body.error); return body; }
function status(text) { el('status').textContent = text; }
async function rooms() {
  try { const data = await request('/api/rooms'); humanPosting = data.principal_kind === 'user' && data.can_post; el('composer').hidden = !humanPosting || !selected; el('rooms').replaceChildren(); for (const room of data.rooms) { const button = document.createElement('button'); button.textContent = room.project_title + (room.title ? ` · ${room.title}` : ''); button.onclick = () => choose(room, button); el('rooms').append(button); } status(data.rooms.length ? 'Connected to shared projects' : 'No projects shared yet'); }
  catch (e) { status(e.message); }
}
function choose(room, button) {
  if (pending) { status('Retry your unconfirmed message before switching projects.'); return; }
  revision = room.policy_revision; el('composer').hidden = !humanPosting;
  selected = room.room_id; cursor = 0; generation++; seen.clear(); more = false;
  el('updates').replaceChildren(); el('title').textContent = room.project_title;
  for (const b of el('rooms').children) b.classList.remove('selected'); button.classList.add('selected');
  el('empty').hidden = false; el('empty').textContent = 'Loading project updates…'; el('older').hidden = true; updates();
}
async function updates() {
  if (!selected || inFlight) return;
  inFlight = true; const current = generation;
  try {
    const data = await request(`/api/updates?room=${encodeURIComponent(selected)}&after=${cursor}`);
    if (generation !== current) return;
    revision = data.room.policy_revision;
    el('participants').replaceChildren();
    for (const participant of data.participants || []) {
      const name = participant.name || `${participant.kind === 'user' ? 'Human' : 'Agent'} · ${participant.id.slice(0,8)}`;
      authors.set(`${participant.kind}:${participant.id}`, name);
      const label = document.createElement('span'); label.textContent = `${name} · ${participant.kind === 'user' ? 'Human' : 'Agent (availability unverified)'}`; el('participants').append(label);
    }
    for (const update of data.updates) {
      if (seen.has(update.message_id)) continue; seen.add(update.message_id);
      const article = document.createElement('article'), who = document.createElement('strong'), body = document.createElement('p');
      who.textContent = authors.get(`${update.author_kind}:${update.author_id}`) || `${update.author_kind === 'user' ? 'Human' : 'Agent'} · ${update.author_id.slice(0, 8)}`;
      body.textContent = update.body || 'Update without text'; article.append(who, body); el('updates').append(article);
    }
    cursor = data.next_sequence; more = data.updates.length === 100; el('older').hidden = !more;
    el('empty').hidden = seen.size > 0; el('empty').textContent = 'No shared updates yet.';
    status('Project history connected');
  } catch (e) { if (generation === current) status(e.message); }
  finally { inFlight = false; if (generation !== current) updates(); }
}
el('refresh').onclick = rooms; el('older').onclick = updates;
setInterval(() => { if (!document.hidden && !more) updates(); }, 5000);
rooms();

async function send() {
  if (!humanPosting || !selected || sending || (!pending && !el('message').value.trim())) return;
  pending ||= { room_id: selected, request_id: crypto.randomUUID(), policy_revision: revision, body: el('message').value };
  sending = true; el('send').disabled = true; el('message').disabled = true;
  try {
    const response = await fetch('/api/messages', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(pending) });
    const result = await response.json(); if (!response.ok) { if ([400,403,413].includes(response.status)) { pending = null; el('send').textContent = 'Send'; } throw new Error(result.error); }
    pending = null; el('message').value = ''; el('send').textContent = 'Send'; status('Message saved'); updates();
  } catch (error) { status(error.message); if (pending) el('send').textContent = 'Retry same message'; }
  finally { sending = false; el('send').disabled = false; el('message').disabled = Boolean(pending); }
}
el('send').onclick = send;
el('message').onkeydown = event => { if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) { event.preventDefault(); send(); } };
