#!/usr/bin/env node
'use strict';
// agent-notes — scaffold, write, move, archive and verify Agent Notes under .agents/notes/.
//
// Zero dependencies on purpose: `init` vendors this one file into <repo>/.agents/bin/ so a
// repository can create and verify its notes (locally or in CI) without this skill installed.
// Everything except `init` works from the vendored copy.

const fs = require('fs');
const os = require('os');
const path = require('path');
const crypto = require('crypto');
const { execFileSync } = require('child_process');

const VERSION = '1.5.1';
const NOTES_REL = '.agents/notes';
const VENDOR_REL = '.agents/bin/agent-notes.js';
const LIFECYCLES = ['proposed', 'implemented', 'rejected'];
const ARCHIVED = 'archived';
const DEFAULT_CLASSES = ['feature', 'bug-fix', 'simplification', 'architecture', 'process', 'testing'];
const NAME_RE = /^(\d{4}-\d{2}-\d{2})-([a-z0-9]+(?:-[a-z0-9]+)*)\.md$/;
const PLACEHOLDER = '<!-- fill:';
const ALT_ESCAPE = '<!-- agent-note-format: alternatives-not-recorded -->';
const WIRE_BEGIN = '<!-- agent-notes:begin -->';
const WIRE_END = '<!-- agent-notes:end -->';

// Canonical section -> accepted `## ` headings. English tokens and their zh equivalents are
// both always accepted, so a repo can switch language without re-checking old notes.
const SECTIONS = {
  problem: ['Problem', '问题'],
  proposal: ['Proposal', '提案'],
  decision: ['Decision', '决策'],
  alternatives: ['Alternatives considered', '备选方案'],
  acceptance: ['Acceptance criteria', '验收标准'],
  risks: ['Risks', '风险'],
  consequences: ['Consequences', '影响'],
  plan: ['Plan', '计划'],
  migration: ['Migration plan', '迁移计划'],
};
const REQUIRED = {
  proposed: ['problem', 'proposal', 'alternatives', 'acceptance', 'risks'],
  implemented: ['problem', 'decision', 'alternatives', 'consequences'],
  rejected: ['problem', 'proposal', 'alternatives'],
};
// Proposal-era headings are spec-speak once a decision has shipped.
const FORBIDDEN = { implemented: ['proposal', 'plan', 'migration', 'acceptance'], proposed: ['decision', 'consequences'] };
const TRANSITIONS = { proposed: ['implemented', 'rejected'] };

class UsageError extends Error {}

// ---------------------------------------------------------------- note templates

const H = {
  en: (k) => SECTIONS[k][0],
  zh: (k) => SECTIONS[k][1],
};
const FILL = {
  en: {
    problem: 'The motivation, written to stand without the solution: what is wrong or missing, and for whom.',
    proposal: 'The intended change. Future tense, plans, migration steps and open questions belong here.',
    decision: 'What shipped, in the present tense: the mechanism, the owner, the paths and names a reader will look for.',
    alternatives: 'One bold-led paragraph per genuine alternative and why it lost — **Option X.** Why not. Record, never invent.',
    acceptance: 'The observable state that means done.',
    risks: 'What could go wrong, and what the change knowingly gives up.',
    consequences: 'What the trade-off cost and what it bought; what now pins the behavior (tests, gates).',
  },
  zh: {
    problem: '动机。不依赖解决方案也能读懂：缺了什么、错在哪里、影响了谁。',
    proposal: '计划中的改动。可以用将来时，计划、迁移步骤和未决问题都写在这里。',
    decision: '已上线的事实，用现在时：机制、归属、读者会去找的路径和名称。',
    alternatives: '每个真实考虑过的方案一段，加粗开头并说明为什么没选 —— **方案 X。** 原因。只记录，不编造。',
    acceptance: '什么可观察的状态算完成。',
    risks: '可能出什么问题，以及这个改动主动放弃了什么。',
    consequences: '这个取舍付出了什么、换来了什么；现在由什么（测试、检查）钉住这个行为。',
  },
};

function statusLine(lifecycle, reason) {
  return lifecycle === 'rejected' ? `Status: rejected — ${reason}` : `Status: ${lifecycle}`;
}

function renderNote(lifecycle, title, lang, reason) {
  const h = H[lang] || H.en;
  const fill = FILL[lang] || FILL.en;
  const body = REQUIRED[lifecycle].map((k) => `## ${h(k)}\n\n${PLACEHOLDER} ${fill[k]} -->\n`).join('\n');
  return `# Agent Note: ${title}\n\n${statusLine(lifecycle, reason)}\n\n${body}`;
}

// ---------------------------------------------------------------- repo + config

function gitToplevel(cwd) {
  try {
    return execFileSync('git', ['rev-parse', '--show-toplevel'], { cwd, stdio: ['ignore', 'pipe', 'ignore'] }).toString().trim();
  } catch {
    return null;
  }
}

function findRoot(opts) {
  if (opts.root) return path.resolve(opts.root);
  for (let dir = process.cwd(); ; dir = path.dirname(dir)) {
    if (fs.existsSync(path.join(dir, NOTES_REL, 'config.json'))) return dir;
    if (path.dirname(dir) === dir) break;
  }
  return gitToplevel(process.cwd()) || process.cwd();
}

function notesDir(root) {
  return path.join(root, NOTES_REL);
}

function loadConfig(root) {
  const file = path.join(notesDir(root), 'config.json');
  if (!fs.existsSync(file)) throw new UsageError(`no ${NOTES_REL}/config.json under ${root} — run \`agent-notes init\` first`);
  const cfg = JSON.parse(fs.readFileSync(file, 'utf8'));
  return { lang: cfg.lang === 'zh' ? 'zh' : 'en', classes: Array.isArray(cfg.classes) && cfg.classes.length ? cfg.classes : DEFAULT_CLASSES };
}

function today() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

function validDate(s) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(s)) return false;
  const d = new Date(`${s}T00:00:00Z`);
  return !Number.isNaN(d.getTime()) && d.toISOString().slice(0, 10) === s;
}

function slugify(title) {
  return title.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 60).replace(/-+$/, '');
}

function sha256(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

function toPosix(p) {
  return p.split(path.sep).join('/');
}

// ---------------------------------------------------------------- note inventory

// Every note file under the tree, active and archived, with its path-encoded axes.
function scanNotes(root) {
  const base = notesDir(root);
  const out = [];
  for (const lifecycle of [...LIFECYCLES, ARCHIVED]) {
    const ldir = path.join(base, lifecycle);
    if (!fs.existsSync(ldir)) continue;
    for (const cls of fs.readdirSync(ldir)) {
      const cdir = path.join(ldir, cls);
      if (!fs.statSync(cdir).isDirectory()) continue;
      for (const name of fs.readdirSync(cdir)) {
        const m = NAME_RE.exec(name);
        if (!m) continue;
        const abs = path.join(cdir, name);
        out.push({ abs, rel: toPosix(path.relative(base, abs)), lifecycle, cls, date: m[1], slug: m[2], name });
      }
    }
  }
  return out;
}

function readHeader(abs) {
  const lines = fs.readFileSync(abs, 'utf8').split('\n');
  const title = (lines[0] || '').replace(/^# Agent Note:\s*/, '').trim();
  const status = (lines[2] || '').replace(/^Status:\s*/, '').trim();
  return { title, status };
}

// Resolve a user-supplied note reference: a path (absolute, or relative to cwd, the repo root
// or the notes dir), a filename, or a bare slug.
function resolveNote(root, ref) {
  const notes = scanNotes(root);
  for (const cand of [path.resolve(ref), path.join(root, ref), path.join(notesDir(root), ref)]) {
    const hit = notes.find((n) => n.abs === cand);
    if (hit) return hit;
  }
  const byName = notes.filter((n) => n.name === ref || n.name === `${ref}.md` || n.slug === ref);
  if (byName.length === 1) return byName[0];
  if (byName.length > 1) throw new UsageError(`"${ref}" is ambiguous: ${byName.map((n) => n.rel).join(', ')}`);
  throw new UsageError(`no Agent Note matches "${ref}"`);
}

// ---------------------------------------------------------------- markdown parsing

// Blank out fenced code, inline code and HTML comments, keeping offsets and newlines, so that
// headings and links inside them are ignored.
function maskCode(text) {
  const blank = (s) => s.replace(/[^\n]/g, ' ');
  let out = text.replace(/^(```|~~~)[^\n]*\n[\s\S]*?^\1[^\n]*$/gm, blank);
  out = out.replace(/<!--[\s\S]*?-->/g, blank);
  out = out.replace(/`[^`\n]*`/g, blank);
  return out;
}

function lineOf(text, offset) {
  let n = 1;
  for (let i = 0; i < offset; i++) if (text.charCodeAt(i) === 10) n++;
  return n;
}

// Relative link targets (inline and reference-style), with the offsets of the target text.
function extractLinks(text) {
  const masked = maskCode(text);
  const links = [];
  const push = (target, start) => {
    if (!target || /^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith('#') || target.startsWith('//')) return;
    links.push({ target, start, end: start + target.length });
  };
  for (const m of masked.matchAll(/\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)/g)) {
    push(m[1], m.index + m[0].indexOf(m[1], 2));
  }
  for (const m of masked.matchAll(/^\s{0,3}\[[^\]\n]+\]:\s*<?(\S+?)>?(?:\s+"[^"]*")?\s*$/gm)) {
    push(m[1], m.index + m[0].indexOf(m[1]));
  }
  return links;
}

function splitAnchor(target) {
  const i = target.search(/[?#]/);
  return i === -1 ? [target, ''] : [target.slice(0, i), target.slice(i)];
}

function resolveLink(root, fromAbs, target) {
  const [p] = splitAnchor(target);
  if (!p) return null;
  let decoded = p;
  try {
    decoded = decodeURI(p);
  } catch {}
  return decoded.startsWith('/') ? path.join(root, decoded) : path.resolve(path.dirname(fromAbs), decoded);
}

function relLink(fromAbs, toAbs) {
  return toPosix(path.relative(path.dirname(fromAbs), toAbs)).replace(/ /g, '%20');
}

function sections(text) {
  const masked = maskCode(text);
  const out = [];
  const re = /^## (.+?)[ \t]*$/gm;
  let m;
  while ((m = re.exec(masked))) out.push({ heading: text.slice(m.index + 3, m.index + 3 + m[1].length).trim(), start: m.index, bodyStart: m.index + m[0].length });
  out.forEach((s, i) => {
    s.end = i + 1 < out.length ? out[i + 1].start : text.length;
    s.body = text.slice(s.bodyStart, s.end);
    s.line = lineOf(text, s.start);
    s.key = canonical(s.heading);
  });
  return out;
}

function canonical(heading) {
  const h = heading.toLowerCase();
  for (const [key, aliases] of Object.entries(SECTIONS)) if (aliases.some((a) => a.toLowerCase() === h)) return key;
  return null;
}

// ---------------------------------------------------------------- verify

function verifyNote(root, note, allNotes) {
  const problems = [];
  const add = (line, msg) => problems.push({ file: note.abs, line, msg });
  const text = fs.readFileSync(note.abs, 'utf8');
  const lines = text.split('\n');
  const archived = note.lifecycle === ARCHIVED;

  if (!validDate(note.date)) add(1, `filename date ${note.date} is not a real date`);
  if (!/^# Agent Note: \S/.test(lines[0] || '')) add(1, 'line 1 must be "# Agent Note: <title>"');
  if ((lines[1] || '') !== '') add(2, 'line 2 must be blank');

  const status = lines[2] || '';
  if (archived) {
    if (status !== 'Status: implemented') add(3, 'archived note must keep "Status: implemented"');
    const m = /^Archived: (\d{4}-\d{2}-\d{2})$/.exec(lines[3] || '');
    if (!m || !validDate(m[1])) add(4, 'archived note needs "Archived: YYYY-MM-DD" directly below the Status line');
    // Archived notes are frozen: format and outbound links are not re-checked.
    return problems;
  }

  if (note.lifecycle === 'rejected') {
    if (!/^Status: rejected — \S/.test(status)) add(3, 'rejected note needs "Status: rejected — <why, in one line>"');
  } else if (status !== `Status: ${note.lifecycle}`) {
    add(3, `Status line must be exactly "Status: ${note.lifecycle}" to match its folder (found "${status}")`);
  }
  if ((lines[3] || '') !== '') add(4, 'line 4 must be blank');

  const dup = allNotes.filter((n) => n.slug === note.slug && n.abs !== note.abs);
  if (dup.length) add(1, `slug "${note.slug}" also exists at ${dup.map((n) => n.rel).join(', ')} — one decision, one file`);

  const secs = sections(text);
  if (!secs.length || secs[0].key !== 'problem') add(secs[0] ? secs[0].line : 1, 'the body must open with "## Problem" (or "## 问题")');
  const present = new Set(secs.map((s) => s.key));
  for (const key of REQUIRED[note.lifecycle]) {
    if (present.has(key)) continue;
    if (key === 'alternatives' && text.includes(ALT_ESCAPE)) continue;
    add(1, `missing required section "## ${SECTIONS[key][0]}" (or "## ${SECTIONS[key][1]}")`);
  }
  for (const s of secs) {
    if ((FORBIDDEN[note.lifecycle] || []).includes(s.key)) add(s.line, `"## ${s.heading}" does not belong in a ${note.lifecycle} note${note.lifecycle === 'implemented' ? ' — describe what shipped under Decision / Consequences' : ''}`);
    if (s.body.includes(PLACEHOLDER)) add(s.line, `section "## ${s.heading}" still has its template placeholder ("<!-- fill: ... -->")`);
    else if (REQUIRED[note.lifecycle].includes(s.key) && !s.body.replace(/<!--[\s\S]*?-->/g, '').trim()) add(s.line, `section "## ${s.heading}" is empty`);
  }
  const ph = text.indexOf(PLACEHOLDER);
  if (ph !== -1 && (!secs.length || ph < secs[0].start)) add(lineOf(text, ph), 'template placeholder ("<!-- fill: ... -->") outside any section');

  for (const l of extractLinks(text)) {
    const abs = resolveLink(root, note.abs, l.target);
    if (abs && !fs.existsSync(abs)) add(lineOf(text, l.start), `broken link: ${l.target}`);
  }
  return problems;
}

function verifyTree(root, classes) {
  const base = notesDir(root);
  const problems = [];
  const add = (abs, msg) => problems.push({ file: abs, line: 0, msg });
  const allowedTop = new Set(['README.md', 'AGENTS.md', 'config.json', '.gitkeep', ...LIFECYCLES, ARCHIVED]);
  for (const e of fs.readdirSync(base)) {
    if (e === '.DS_Store') continue; // Finder metadata is not part of the notes tree.
    if (!allowedTop.has(e) && !/^README\.[a-z-]+\.md$/i.test(e)) add(path.join(base, e), `unexpected entry in ${NOTES_REL}/ (lifecycles are ${[...LIFECYCLES, ARCHIVED].join(', ')})`);
  }
  for (const lifecycle of [...LIFECYCLES, ARCHIVED]) {
    const ldir = path.join(base, lifecycle);
    if (!fs.existsSync(ldir)) continue;
    for (const cls of fs.readdirSync(ldir)) {
      if (cls === '.DS_Store') continue;
      const cdir = path.join(ldir, cls);
      if (!fs.statSync(cdir).isDirectory()) {
        const ok = ['AGENTS.md', '.gitkeep'].includes(cls) || (lifecycle === ARCHIVED && cls === 'manifest.json');
        if (!ok) add(cdir, 'unexpected file; notes live in <lifecycle>/<class>/');
        continue;
      }
      if (!classes.includes(cls)) add(cdir, `unknown class "${cls}" — allowed: ${classes.join(', ')} (edit config.json and README.md to add one)`);
      for (const name of fs.readdirSync(cdir)) {
        if (name === '.gitkeep' || name === '.DS_Store') continue;
        if (!NAME_RE.test(name)) add(path.join(cdir, name), 'note filenames are yyyy-mm-dd-lowercase-slug.md');
      }
    }
  }
  return problems;
}

function loadManifest(root) {
  const file = path.join(notesDir(root), ARCHIVED, 'manifest.json');
  if (!fs.existsSync(file)) return { file, data: { sealed: {} } };
  const data = JSON.parse(fs.readFileSync(file, 'utf8'));
  data.sealed = data.sealed || {};
  return { file, data };
}

function verifyArchive(root, notes) {
  const problems = [];
  const { file, data } = loadManifest(root);
  const archived = notes.filter((n) => n.lifecycle === ARCHIVED);
  for (const n of archived) {
    const want = data.sealed[n.rel];
    if (!want) problems.push({ file: n.abs, line: 0, msg: 'archived note is not sealed in archived/manifest.json — archive with `agent-notes archive`, do not move files by hand' });
    else if (want !== sha256(n.abs)) problems.push({ file: n.abs, line: 0, msg: 'archived note was modified after sealing — archived notes are frozen; revert it' });
  }
  for (const rel of Object.keys(data.sealed)) {
    if (!archived.some((n) => n.rel === rel)) problems.push({ file, line: 0, msg: `sealed note ${rel} is missing — archived notes may not be moved or deleted` });
  }
  return problems;
}

function verify(root) {
  const { classes } = loadConfig(root);
  const notes = scanNotes(root);
  const problems = [...verifyTree(root, classes)];
  for (const n of notes) problems.push(...verifyNote(root, n, notes));
  problems.push(...verifyArchive(root, notes));
  problems.sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line);
  return { notes, problems };
}

// ---------------------------------------------------------------- link repair

function listMarkdown(root) {
  try {
    const out = execFileSync('git', ['ls-files', '-co', '--exclude-standard', '-z', '--', '*.md'], { cwd: root, stdio: ['ignore', 'pipe', 'ignore'] });
    return out.toString().split('\0').filter(Boolean).map((f) => path.join(root, f)).filter((f) => fs.existsSync(f));
  } catch {
    const files = [];
    const walk = (dir) => {
      for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
        if (e.name === 'node_modules' || e.name === '.git') continue;
        const abs = path.join(dir, e.name);
        if (e.isDirectory()) walk(abs);
        else if (e.name.endsWith('.md')) files.push(abs);
      }
    };
    walk(root);
    return files;
  }
}

// Apply `map(resolvedAbs, target) -> newPath | null` to every relative link; anchors are kept.
function rewriteLinks(root, text, fileAbs, map) {
  const edits = [];
  for (const l of extractLinks(text)) {
    const abs = resolveLink(root, fileAbs, l.target);
    const next = abs && map(abs, l.target);
    if (next == null) continue;
    const value = next + splitAnchor(l.target)[1];
    if (value !== l.target) edits.push({ ...l, value });
  }
  let out = text;
  for (const e of edits.sort((a, b) => b.start - a.start)) out = out.slice(0, e.start) + e.value + out.slice(e.end);
  return { text: out, changed: edits.length };
}

// Point every inbound link at `newAbs`. Archived notes are frozen and never rewritten.
function repairInbound(root, oldAbs, newAbs) {
  const archivedDir = path.join(notesDir(root), ARCHIVED) + path.sep;
  const touched = [];
  for (const file of listMarkdown(root)) {
    if (file.startsWith(archivedDir) || file === newAbs) continue;
    const text = fs.readFileSync(file, 'utf8');
    const res = rewriteLinks(root, text, file, (abs) => (abs === oldAbs ? relLink(file, newAbs) : null));
    if (res.changed) {
      fs.writeFileSync(file, res.text);
      touched.push(file);
    }
  }
  return touched;
}

// A moved file's own relative links are re-based on its new directory; repo-root links
// ("/docs/x.md") are left alone.
function rebaseOutbound(root, text, oldAbs, newAbs) {
  return rewriteLinks(root, text, oldAbs, (abs, target) => (target.startsWith('/') ? null : relLink(newAbs, abs))).text;
}

// ---------------------------------------------------------------- commands

function cmdNew(root, opts, args) {
  const [lifecycle, cls, ...titleParts] = args;
  const title = titleParts.join(' ').trim();
  if (!lifecycle || !cls || !title) throw new UsageError('usage: new <proposed|implemented|rejected> <class> "<title>" [--slug s] [--date yyyy-mm-dd] [--reason r]');
  const { lang, classes } = loadConfig(root);
  if (!LIFECYCLES.includes(lifecycle)) throw new UsageError(`lifecycle must be one of ${LIFECYCLES.join(', ')}`);
  if (!classes.includes(cls)) throw new UsageError(`class must be one of ${classes.join(', ')}`);
  if (lifecycle === 'rejected' && !opts.reason) throw new UsageError('a rejected note needs --reason "<why, in one line>"');
  const slug = opts.slug || slugify(title);
  if (!slug || !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(slug)) throw new UsageError('could not derive an ASCII slug from the title — pass --slug lowercase-words');
  const date = opts.date || today();
  if (!validDate(date)) throw new UsageError(`--date ${date} is not yyyy-mm-dd`);
  const existing = scanNotes(root).find((n) => n.slug === slug);
  if (existing) throw new UsageError(`slug "${slug}" already exists at ${existing.rel} — update that note instead, or pick another --slug`);
  const abs = path.join(notesDir(root), lifecycle, cls, `${date}-${slug}.md`);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, renderNote(lifecycle, title, lang, opts.reason));
  // The supersession check runs in the CLI layer, which can await Context Engine.
  return { abs, rel: toPosix(path.relative(root, abs)), title, slug };
}

function findRelated(root, query) {
  const terms = [...new Set(query.toLowerCase().split(/[\s,，、/_-]+/).filter((t) => t.length >= 3 || /[^\x00-\x7f]/.test(t)))];
  if (!terms.length) return [];
  const out = [];
  for (const note of scanNotes(root)) {
    if (note.lifecycle === ARCHIVED) continue;
    const text = fs.readFileSync(note.abs, 'utf8').toLowerCase();
    const head = `${note.slug} ${text.split('\n')[0]}`;
    let score = 0;
    const hits = [];
    for (const t of terms) {
      const inHead = head.includes(t);
      const count = text.split(t).length - 1;
      if (!inHead && !count) continue;
      hits.push(t);
      score += (inHead ? 5 : 0) + Math.min(count, 5);
    }
    if (score) out.push({ note, score, hits, ...readHeader(note.abs) });
  }
  return out.sort((a, b) => b.score - a.score);
}

// ---------------------------------------------------------------- semantic related
//
// A supersession check fails quietly when the old note says "browser launcher" and you search
// "浏览器 启动". CE supplies semantic ranking; a portable local substring fallback remains.
//
// Retrieval delegates to CE; the zero-dependency adapter is vendored with governance.
const DEFAULT_MIN_SCORE = 0.4;


// Notify only after governance has completed; CE owns all derived maintenance.
async function notifyIndex(config, adapter, sourceRoot) {
  if (config.contextEngine?.autoIndex === false) return;
  const containerClient = '/home/agent/.config/context-engine/ce-client.cjs';
  const containerConfig = process.env.CONTEXT_ENGINE_CLIENT_CONFIG || '/home/agent/.config/context-engine/client.json';
  let target;
  if (process.platform === 'linux' && fs.existsSync(containerClient) && fs.existsSync(containerConfig)) {
    target = {command:process.execPath, args:[containerClient]};
  } else {
    try {target = ceCommand(config);} catch {return;} // Portable standalone governance.
  }
  try {
    const stdout = await new Promise((resolve,reject) => require('child_process').execFile(target.command,
      [...target.args,'notify','--adapter',adapter,'--root',sourceRoot,'--json'],
      {encoding:'utf8',timeout:5000,maxBuffer:65536}, (error,stdout,stderr) => {
        if(error)reject(new Error((stderr || error.message).trim())); else resolve(stdout);
      }));
    const result = JSON.parse(stdout);
    if(result.status !== 'queued')throw Error('unexpected notification response');
    console.log('CE index queued · ' + result.source);
  } catch(error) {console.error('CE index notification failed: ' + error.message.slice(0,400) + ' — rerun sync/verify to retry');}
}

function ceCommand(config) {
  const setting = config.contextEngine;
  const command = process.env.CONTEXT_ENGINE_COMMAND || (setting && setting.command);
  if (!command) throw new Error('Context Engine is not configured');
  return {command, args: setting && Array.isArray(setting.args) ? setting.args : []};
}
async function ceCall(config, method, args) {
  const target = ceCommand(config);
  const argv = [...target.args, method];
  if (args.query !== undefined) argv.push(args.query);
  for (const [key,value] of Object.entries(args)) {
    if (key === 'query' || value === undefined || value === null || value === false) continue;
    argv.push('--' + key.replaceAll('_','-'));
    if (value !== true) argv.push(String(value));
  }
  argv.push('--json');
  const {stdout} = await new Promise((resolve,reject) => require('child_process').execFile(target.command, argv,
    {encoding:'utf8', timeout:120000, maxBuffer:16*1024*1024}, (error,stdout,stderr) => {
      if (error) return reject(new Error('Context Engine failed: ' + (stderr || error.message).trim().slice(0,400)));
      resolve({stdout});
    }));
  const value = JSON.parse(stdout);
  if (value.error) throw new Error(value.error.message || 'Context Engine error');
  if (value.retrieval && value.retrieval.stale_evidence && value.retrieval.stale_evidence.length) throw new Error('CE source is stale; sync it before semantic retrieval');
  return value;
}
function cePath(p) { try { return fs.realpathSync(p); } catch { return path.resolve(p); } }

function retrievalConfig(root) {
  const read=file=>{try{return JSON.parse(fs.readFileSync(file,'utf8'));}catch{return {};}};
  const global=read(path.join(process.env.AGENT_MEMORY_DIR || path.join(os.homedir(),'.agent-memory'),'config.json'));
  return {...global,...read(path.join(notesDir(root),'config.json'))};
}
async function relatedNotes(root,query,opts={}) {
  const cfg=retrievalConfig(root);
  try {
    const sources=await ceCall(cfg,'sources',{});
    const source=sources.find(s=>s.adapter==='agent-notes' && cePath(s.root)===cePath(notesDir(root)));
    if(!source)throw new Error('this repository is not registered in CE');
    const notes=scanNotes(root);
    if(source.indexed_instances!==notes.length)throw new Error('CE notes source changed; sync first');
    const requestedMode=opts.lexical?'lexical':'auto';
    const result=await ceCall(cfg,'search',{query,source:source.id,type:'decision',repo:source.repo_id,history:true,mode:requestedMode,limit:100});
    const profile=result.retrieval?.profiles?.decision || {};
    const reported=profile.mode || result.retrieval?.mode;
    const mode=['semantic','lexical'].includes(reported)?reported:requestedMode;
    const fallback=profile.fallback;
    const note=mode==='lexical' && fallback ? 'CE lexical fallback: '+(fallback.code || fallback.reason || String(fallback)) : null;
    const byPath=new Map(notes.filter(n=>n.lifecycle!==ARCHIVED).map(n=>[cePath(n.abs),n]));
    const hits=result.results.map(hit=>{
      const note=byPath.get(cePath(hit.local_path));
      if(!note)return null;
      return {note,score:hit.score,heading:hit.section,...readHeader(note.abs),uri:hit.uri,revision:hit.revision,anchor:hit.anchor};
    }).filter(Boolean).slice(0,Number(opts.k)||15);
    const floor=mode==='semantic'?(opts.min===undefined ? cfg.minScore ?? DEFAULT_MIN_SCORE : Number(opts.min)):null;
    const kept=floor===null?hits:hits.filter(h=>h.score>=floor);
    return {mode:'ce · '+mode,note,floor,hidden:hits.length-kept.length,hits:kept};
  }catch(error){
    return {mode:'lexical',note:error.message+' — falling back to local substring matching',hits:findRelated(root,query).slice(0,Number(opts.k)||15)};
  }
}

function cmdMove(root, opts, args) {
  const [ref, to] = args;
  if (!ref || !to) throw new UsageError('usage: move <note> <implemented|rejected> [--reason r]');
  const note = resolveNote(root, ref);
  const allowed = TRANSITIONS[note.lifecycle] || [];
  if (!allowed.includes(to)) {
    const hint = to === ARCHIVED ? 'only implemented notes are archived, with `archive <note>`; reject an obsolete proposal instead'
      : note.lifecycle === 'implemented' ? 'a shipped decision is never moved: reverse it with a new note that supersedes it, or `archive` it'
      : note.lifecycle === 'rejected' ? 'a rejected note is frozen: write a new proposal that links to it'
      : note.lifecycle === ARCHIVED ? 'archived notes are frozen'
      : `a ${note.lifecycle} note can move to: ${allowed.join(', ')}`;
    throw new UsageError(`cannot move ${note.lifecycle} → ${to}; ${hint}`);
  }
  if (to === 'rejected' && !opts.reason) throw new UsageError('rejecting needs --reason "<why, in one line>"');
  const newAbs = path.join(notesDir(root), to, note.cls, note.name);
  let text = fs.readFileSync(note.abs, 'utf8').replace(/^Status: .*$/m, statusLine(to, opts.reason));
  text = rebaseOutbound(root, text, note.abs, newAbs);
  fs.mkdirSync(path.dirname(newAbs), { recursive: true });
  fs.writeFileSync(newAbs, text);
  fs.unlinkSync(note.abs);
  const touched = repairInbound(root, note.abs, newAbs);
  return { from: note.rel, to: toPosix(path.relative(notesDir(root), newAbs)), abs: newAbs, touched };
}

function cmdArchive(root, opts, args) {
  const [ref] = args;
  if (!ref) throw new UsageError('usage: archive <note> [--date yyyy-mm-dd]');
  const note = resolveNote(root, ref);
  if (note.lifecycle !== 'implemented') {
    throw new UsageError(note.lifecycle === 'proposed'
      ? 'proposed notes are never archived — reject an obsolete proposal instead (`move <note> rejected --reason ...`)'
      : `only implemented notes can be archived (this one is ${note.lifecycle})`);
  }
  const problems = verifyNote(root, note, scanNotes(root));
  if (problems.length) throw new UsageError(`fix the note before sealing it:\n${formatProblems(root, problems)}`);
  const date = opts.date || today();
  if (!validDate(date)) throw new UsageError(`--date ${date} is not yyyy-mm-dd`);
  const rel = `${ARCHIVED}/${note.cls}/${note.name}`;
  const newAbs = path.join(notesDir(root), rel);
  const { file: manifestFile, data } = loadManifest(root);
  if (data.sealed[rel] || fs.existsSync(newAbs)) throw new UsageError(`${rel} is already sealed`);
  const text = fs.readFileSync(note.abs, 'utf8').replace(/^(Status: implemented)$/m, `$1\nArchived: ${date}`);
  fs.mkdirSync(path.dirname(newAbs), { recursive: true });
  fs.writeFileSync(newAbs, text);
  fs.unlinkSync(note.abs);
  data.sealed[rel] = sha256(newAbs);
  data.sealed = Object.fromEntries(Object.entries(data.sealed).sort(([a], [b]) => a.localeCompare(b)));
  fs.writeFileSync(manifestFile, `${JSON.stringify(data, null, 2)}\n`);
  const touched = repairInbound(root, note.abs, newAbs);
  return { rel, abs: newAbs, touched };
}

function cmdList(root, opts) {
  const notes = scanNotes(root).filter((n) => (opts.all || n.lifecycle !== ARCHIVED)
    && (!opts.lifecycle || n.lifecycle === opts.lifecycle) && (!opts.class || n.cls === opts.class));
  return notes.map((n) => ({ ...n, ...readHeader(n.abs) })).sort((a, b) => a.rel.localeCompare(b.rel));
}

// ---------------------------------------------------------------- init (skill-only)

function templatesDir(lang) {
  return path.join(__dirname, '..', 'templates', lang);
}

const CLASS_DESC = {
  en: {
    feature: 'A new user- or model-facing capability.',
    'bug-fix': 'Corrects a defect or closes a gap a postmortem surfaced.',
    simplification: 'Removes code, behavior or surface area without adding a capability.',
    architecture: 'A structural decision about the shipped source — how modules relate, what the runtime vocabulary is.',
    process: 'Tooling, policy or workflow around the code — gates, package manager, CI — not runtime behavior.',
    testing: 'Test infrastructure and strategy.',
  },
  zh: {
    feature: '新增一个面向用户或模型的能力。',
    'bug-fix': '修复缺陷，或补上复盘暴露出的缺口。',
    simplification: '删除代码、行为或对外面积，不新增能力。',
    architecture: '关于所交付源码的结构性决策 —— 模块如何关联、运行时词汇是什么。',
    process: '代码之外的工具、规范或流程 —— 检查、包管理、CI —— 不是运行时行为。',
    testing: '测试基础设施与测试策略。',
  },
};

function fillTemplate(text, vars) {
  return text.replace(/\{\{(\w+)\}\}/g, (m, k) => (k in vars ? vars[k] : m));
}

function writeIfAbsent(root, abs, content, force, report) {
  const rel = toPosix(path.relative(root, abs));
  if (fs.existsSync(abs) && !force) {
    report.push(`skip   ${rel} (exists; --force to overwrite)`);
    return;
  }
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, content);
  report.push(`write  ${rel}`);
}

function upsertBlock(file, block) {
  const exists = fs.existsSync(file);
  const text = exists ? fs.readFileSync(file, 'utf8') : '';
  const wrapped = `${WIRE_BEGIN}\n${block.trim()}\n${WIRE_END}`;
  const re = new RegExp(`${WIRE_BEGIN}[\\s\\S]*?${WIRE_END}`);
  const next = re.test(text) ? text.replace(re, wrapped) : `${text}${text && !text.endsWith('\n') ? '\n' : ''}${text ? '\n' : ''}${wrapped}\n`;
  if (next === text) return 'unchanged';
  fs.writeFileSync(file, next);
  return re.test(text) ? 'updated' : exists ? 'appended' : 'created';
}

function cmdInit(root, opts) {
  if (opts.lang && !['en', 'zh'].includes(opts.lang)) throw new UsageError('--lang must be en or zh');
  const classes = opts.classes ? String(opts.classes).split(',').map((s) => s.trim()).filter(Boolean) : DEFAULT_CLASSES;
  for (const c of classes) if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(c)) throw new UsageError(`class "${c}" must be lowercase-kebab`);
  const force = !!opts.force;
  const vendor = !opts['no-vendor'];
  const base = notesDir(root);
  const report = [];

  // A re-run keeps the repo's existing language and classes unless --force replaces them.
  const cfgFile = path.join(base, 'config.json');
  const existing = fs.existsSync(cfgFile) && !force ? JSON.parse(fs.readFileSync(cfgFile, 'utf8')) : null;
  const lang = existing ? (existing.lang === 'zh' ? 'zh' : 'en') : opts.lang || 'en';
  if (existing && opts.lang && opts.lang !== lang) report.push(`note   config.json says lang=${lang}; --lang ${opts.lang} ignored without --force`);
  const cfg = { lang, classes: existing && Array.isArray(existing.classes) ? existing.classes : classes };
  const tdir = templatesDir(lang);
  if (!fs.existsSync(tdir)) throw new UsageError('init needs the skill templates; run it from the agent-notes skill, not a vendored copy');
  const cli = vendor ? `node ${VENDOR_REL}` : `node ${path.resolve(__filename)}`;
  const desc = CLASS_DESC[lang];
  const vars = {
    CLI: cli,
    CLASSES_TABLE: [lang === 'zh' ? '| 分类 | 覆盖范围 |' : '| Class | What it covers |', '|---|---|',
      ...cfg.classes.map((c) => `| \`${c}\` | ${desc[c] || (lang === 'zh' ? '（补充说明）' : '(describe)')} |`)].join('\n'),
    CLASSES: cfg.classes.map((c) => `\`${c}\``).join(', '),
    DATE: today(),
  };

  const doc = (tpl) => fillTemplate(fs.readFileSync(path.join(tdir, tpl), 'utf8'), vars);
  writeIfAbsent(root, cfgFile, `${JSON.stringify({ version: 1, lang, classes: cfg.classes }, null, 2)}\n`, force, report);
  writeIfAbsent(root, path.join(base, 'README.md'), doc('README.md'), force, report);
  writeIfAbsent(root, path.join(base, 'AGENTS.md'), doc('AGENTS.md'), force, report);
  writeIfAbsent(root, path.join(base, 'implemented', 'AGENTS.md'), doc('implemented-AGENTS.md'), force, report);
  writeIfAbsent(root, path.join(base, ARCHIVED, 'AGENTS.md'), doc('archived-AGENTS.md'), force, report);
  const manifest = path.join(base, ARCHIVED, 'manifest.json');
  if (!fs.existsSync(manifest)) writeIfAbsent(root, manifest, `${JSON.stringify({ version: 1, sealed: {} }, null, 2)}\n`, false, report);
  for (const l of ['proposed', 'rejected']) {
    const dir = path.join(base, l);
    if (!fs.existsSync(dir) || !fs.readdirSync(dir).length) writeIfAbsent(root, path.join(dir, '.gitkeep'), '', false, report);
  }

  if (vendor) {
    const dest = path.join(root, VENDOR_REL);
    const src = fs.readFileSync(__filename);
    if (!fs.existsSync(dest) || !fs.readFileSync(dest).equals(src)) {
      fs.mkdirSync(path.dirname(dest), { recursive: true });
      fs.writeFileSync(dest, src, { mode: 0o755 });
      report.push(`vendor ${VENDOR_REL} (v${VERSION})`);
    } else report.push(`vendor ${VENDOR_REL} (up to date)`);
  }

  if (vendor && registerRepo(root)) report.push(`listed ${toPosix(root)} in ${toPosix(registryFile())} — \`update --all\` re-vendors every repo at once`);

  const wire = opts.wire || 'auto';
  if (!['auto', 'agents', 'claude', 'both', 'none'].includes(wire)) throw new UsageError('--wire must be auto, agents, claude, both or none');
  if (wire !== 'none') {
    const block = renderWire(lang, cfg.classes, vendor);
    const agents = path.join(root, 'AGENTS.md');
    const claude = path.join(root, 'CLAUDE.md');
    let targets;
    if (wire === 'agents') targets = [agents];
    else if (wire === 'claude') targets = [claude];
    else if (wire === 'both') targets = [agents, claude];
    else {
      // A CLAUDE.md that only imports AGENTS.md already sees the block through the import.
      const claudeImports = fs.existsSync(claude) && /^@AGENTS\.md\s*$/m.test(fs.readFileSync(claude, 'utf8'));
      targets = [agents, claude].filter((f) => fs.existsSync(f) && !(f === claude && claudeImports && fs.existsSync(agents)));
      if (!targets.length) targets = [agents];
    }
    for (const f of targets) report.push(`wire   ${path.basename(f)} (${upsertBlock(f, block)})`);
  }
  return { report, lang };
}

// ---------------------------------------------------------------- registry and update
//
// Each repo keeps a *copy* of this CLI, not a link to the skill: `.agents/` is committed in some
// repos, where a symlink or an absolute path would be broken for every teammate and every CI
// runner — and would write this machine's home directory into a shared repository. The copies are
// therefore updated deliberately, and `init` lists each repo here so one command reaches them all.
// (A repo whose notes never leave this machine can opt out with `init --no-vendor`, which points
// its docs at the skill path instead and has nothing to update.)

const registryFile = () => process.env.AGENT_NOTES_REGISTRY || path.join(os.homedir(), '.config', 'agent-notes', 'repos');

function readRegistry() {
  try {
    return [...new Set(fs.readFileSync(registryFile(), 'utf8').split('\n').map((s) => s.trim()).filter(Boolean))];
  } catch {
    return [];
  }
}

function registerRepo(root) {
  let phys = root;
  try {
    phys = fs.realpathSync(root);
  } catch {}
  const list = readRegistry();
  if (list.includes(phys)) return false;
  const file = registryFile();
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, `${[...list, phys].join('\n')}\n`);
  return true;
}

function renderWire(lang, classes, vendored) {
  const cli = vendored ? `node ${VENDOR_REL}` : `node ${path.resolve(__filename)}`;
  return fillTemplate(fs.readFileSync(path.join(templatesDir(lang), 'wire.md'), 'utf8'), { CLI: cli, CLASSES: classes.map((c) => `\`${c}\``).join(', ') });
}

// Which instruction files carry a wiring block that no longer matches what this version writes.
// Reported, never rewritten: AGENTS.md is shared instruction surface, so that edit stays a
// deliberate `init` inside the repo, with its diff read by a human.
function wiringDrift(root) {
  const { lang, classes } = loadConfig(root);
  const expected = `${WIRE_BEGIN}\n${renderWire(lang, classes, fs.existsSync(path.join(root, VENDOR_REL))).trim()}\n${WIRE_END}`;
  const out = [];
  for (const f of ['AGENTS.md', 'CLAUDE.md']) {
    const abs = path.join(root, f);
    if (!fs.existsSync(abs)) continue;
    const m = new RegExp(`${WIRE_BEGIN}[\\s\\S]*?${WIRE_END}`).exec(fs.readFileSync(abs, 'utf8'));
    if (m && m[0] !== expected) out.push(f);
  }
  return out;
}

const vendoredVersion = (text) => (/^const VERSION = '([^']+)'/m.exec(text) || [, '?'])[1];

// Re-vendor this CLI into every registered repo (or the ones named). Touches exactly one file per
// repo; anything else it finds, it reports.
function cmdUpdate(opts, args) {
  if (!fs.existsSync(templatesDir('en'))) throw new UsageError('update needs the skill copy of this CLI (it is the source), not a vendored one');
  const targets = args.length ? args.map((p) => path.resolve(p)) : readRegistry();
  if (!targets.length) throw new UsageError(`no repositories listed in ${registryFile()} — name them as arguments, or run \`init\` in each once`);
  const src = fs.readFileSync(__filename);
  return targets.map((root) => {
    const dest = path.join(root, VENDOR_REL);
    if (!fs.existsSync(root)) return { root, status: 'gone — remove the line from the registry' };
    if (!fs.existsSync(path.join(notesDir(root), 'config.json'))) return { root, status: `no ${NOTES_REL}/ — not an agent-notes repo` };
    const drift = wiringDrift(root);
    if (!fs.existsSync(dest)) return { root, status: 'no vendored copy (--no-vendor: it runs the skill directly)', drift };
    if (fs.readFileSync(dest).equals(src)) return { root, status: `already v${VERSION}`, drift };
    const was = vendoredVersion(fs.readFileSync(dest, 'utf8'));
    fs.writeFileSync(dest, src, { mode: 0o755 });
    return { root, status: `v${was} → v${VERSION}`, updated: true, drift };
  });
}

// ---------------------------------------------------------------- CLI

const BOOLEAN = new Set(['force', 'no-vendor', 'all', 'json', 'lexical', 'help', 'version']);

function parseArgs(argv) {
  const opts = {};
  const args = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '-h') opts.help = true;
    else if (a === '-k') opts.k = argv[++i];
    else if (a.startsWith('--')) {
      const eq = a.indexOf('=');
      const key = eq === -1 ? a.slice(2) : a.slice(2, eq);
      if (eq !== -1) opts[key] = a.slice(eq + 1);
      else if (BOOLEAN.has(key)) opts[key] = true;
      else if (i + 1 < argv.length) opts[key] = argv[++i];
      else throw new UsageError(`--${key} needs a value`);
    } else args.push(a);
  }
  return { opts, args };
}

function formatProblems(root, problems) {
  return problems.map((p) => `${toPosix(path.relative(root, p.file))}${p.line ? `:${p.line}` : ''}: ${p.msg}`).join('\n');
}

const HELP = `agent-notes v${VERSION} — decision records for agents, under ${NOTES_REL}/

  init    [--lang en|zh] [--classes a,b,..] [--wire auto|agents|claude|both|none] [--no-vendor] [--force]
          scaffold ${NOTES_REL}/, vendor this CLI to ${VENDOR_REL}, wire the rule into AGENTS.md/CLAUDE.md
  new     <proposed|implemented|rejected> <class> "<title>" [--slug s] [--date d] [--reason r]
          create a note from the template and list notes it may supersede
  related <keywords...> [-k n] [--min 0.4] [--lexical]
          rank active notes that already cover a topic (supersession check); Context Engine when configured; otherwise
          registered CE source is available, explicit local substring fallback otherwise
  list    [--lifecycle l] [--class c] [--all] [--json]
  move    <note> <implemented|rejected> [--reason r]   change lifecycle, repair links
  archive <note> [--date d]    freeze an implemented note into archived/ and seal its hash
  verify                       check format, lifecycle/status agreement, links and archive seals
  update  [<repo>...]          re-vendor this CLI into every repo init has listed (skill copy only);
                               reports wiring blocks that drifted, never rewrites them

  --root DIR  repository root (default: nearest dir with ${NOTES_REL}/config.json, else git toplevel)`;

async function main(argv) {
  const { opts, args } = parseArgs(argv);
  const [cmd, ...rest] = args;
  if (opts.version) return console.log(VERSION);
  if (!cmd || opts.help || cmd === 'help') return console.log(HELP);
  const root = findRoot(opts);
  switch (cmd) {
    case 'init': {
      const { report } = cmdInit(root, opts);
      console.log(report.join('\n'));
      console.log(`\nNext: read ${NOTES_REL}/README.md, then \`${opts['no-vendor'] ? `node ${__filename}` : `node ${VENDOR_REL}`} verify\`.`);
      return;
    }
    case 'new': {
      const r = cmdNew(root, opts, rest);
      console.log(r.rel);
      const similar = await relatedNotes(root, `${r.title} ${r.slug.replace(/-/g, ' ')}`, { k: 6 });
      const hits = similar.hits.filter((x) => x.note.abs !== r.abs).slice(0, 5);
      if (hits.length) {
        console.log(`\nPossibly superseded or overlapping (${similar.mode}) — read these before writing:`);
        for (const x of hits) console.log(`  ${x.note.rel}  (${x.status})  ${x.title}`);
      }
      return;
    }
    case 'related': {
      if (!rest.length) throw new UsageError('usage: related <keywords...>');
      const r = await relatedNotes(root, rest.join(' '), opts);
      console.log(r.note ? `${r.mode} · ${r.note}` : r.mode);
      if (!r.hits.length) {
        return console.log(r.hidden ? `every active note scored below ${r.floor} — none of them is even near this topic` : 'no active note mentions those terms');
      }
      for (const x of r.hits) {
        const where = x.heading ? `  §${x.heading}` : `  [${x.hits.join(', ')}]`;
        console.log(`${(typeof x.score === 'number' && x.score <= 1 ? x.score.toFixed(2) : String(x.score)).padStart(5)}  ${x.note.rel}  (${x.status})  ${x.title}${where}`);
      }
      if (r.hidden) console.log(`(${r.hidden} below the relevance floor ${r.floor}, hidden)`);
      return;
    }
    case 'list': {
      const rows = cmdList(root, opts);
      if (opts.json) return console.log(JSON.stringify(rows.map(({ abs, ...r }) => r), null, 2));
      if (!rows.length) return console.log('no notes');
      for (const r of rows) console.log(`${r.rel}\n    ${r.title} — ${r.status}`);
      return;
    }
    case 'move': {
      const r = cmdMove(root, opts, rest);
      console.log(`moved ${r.from} → ${r.to}`);
      for (const f of r.touched) console.log(`  relinked ${toPosix(path.relative(root, f))}`);
      if (r.to.startsWith('implemented/')) {
        console.log('\nNow rewrite it for shipped reality (verify fails until you do):\n'
          + '  Proposal → Decision (present tense, what actually shipped)\n'
          + '  Acceptance criteria + Risks → Consequences (what it cost and bought, what pins it)\n'
          + '  drop Plan / Migration plan; keep Alternatives considered');
      }
      return;
    }
    case 'archive': {
      const r = cmdArchive(root, opts, rest);
      console.log(`sealed ${r.rel}`);
      if (r.touched.length) {
        console.log('Inbound links now point at the frozen copy — redirect each to current authority, or delete it, unless it cites history on purpose:');
        for (const f of r.touched) console.log(`  ${toPosix(path.relative(root, f))}`);
      }
      return;
    }
    case 'update': {
      const rows = cmdUpdate(opts, rest);
      for (const r of rows) {
        console.log(`${r.updated ? '✓' : ' '} ${toPosix(r.root)}  ${r.status}`);
        for (const f of r.drift || []) console.log(`    ${f}: the wiring block differs from this version — re-run \`init --root ${toPosix(r.root)}\` and read the diff`);
      }
      const n = rows.filter((r) => r.updated).length;
      console.log(`\n${n} of ${rows.length} updated to v${VERSION}`);
      if (rows.some((r) => /^gone|not an agent-notes/.test(r.status))) process.exitCode = 1;
      return;
    }
    case 'verify': {
      const { notes, problems } = verify(root);
      if (problems.length) {
        console.log(formatProblems(root, problems));
        console.log(`\n${notes.length} notes, ${problems.length} problem${problems.length === 1 ? '' : 's'}`);
        process.exitCode = 1;
      } else {
        console.log(`${notes.length} notes OK`);
        await notifyIndex(retrievalConfig(root), 'agent-notes', notesDir(root));
      }
      return;
    }
    default:
      throw new UsageError(`unknown command "${cmd}"\n\n${HELP}`);
  }
}

module.exports = {
  VERSION, SECTIONS, REQUIRED, parseArgs, renderNote, slugify, validDate, extractLinks, sections,
  scanNotes, verify, cmdInit, cmdNew, cmdMove, cmdArchive, cmdList, cmdUpdate, findRelated, relatedNotes,
  retrievalConfig, readRegistry, registryFile, wiringDrift, upsertBlock, UsageError,
};

if (require.main === module) {
  main(process.argv.slice(2)).catch((e) => {
    if (e instanceof UsageError) {
      console.error(`agent-notes: ${e.message}`);
      process.exitCode = 2;
    } else {
      console.error(e.stack || e.message);
      process.exitCode = 1;
    }
  });
}
