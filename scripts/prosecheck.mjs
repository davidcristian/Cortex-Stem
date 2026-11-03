// Checks the prose rules of AGENTS.md in the markdown files: no banned word and no dash used as
// punctuation. Usage: node scripts/prosecheck.mjs [file ...]   (default: every tracked .md file)
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

const BANNED = [
  'load-bearing', 'gate', 'gates', 'gated', 'gating', 'ungated',
  'spell', 'spells', 'spelled', 'spelling', 'spellings', 'respell',
  'carry', 'carries', 'carried', 'carrying', 'land', 'lands', 'landed', 'landing',
  'seam', 'seams', 'arm', 'arms', 'armed', 'arming', 'sitting', 'sittings', 'sweep', 'sweeps',
  'heal', 'heals', 'healed', 'healing', 'bite', 'bites', 'ride', 'rides', 'riding', 'standing',
  'in force', 'needle', 'needles', 'knob', 'knobs', 'lever', 'levers', 'honest', 'honestly',
  'verdict', 'verdicts', 're-derive', 're-derives', 're-derived', 're-deriving', 'rederive',
  'earn', 'earns', 'earned', 'backstop', 'tripwire', 'ratchet', 'footgun', 'chokepoint',
  'robust', 'seamless', 'seamlessly', 'comprehensive', 'crucial', 'crucially', 'critically',
  'fundamentally', 'leverage', 'leverages', 'delve', 'utilize', 'worth noting', 'under the hood',
  'boils down', 'low-hanging', 'rabbit hole', 'north star', 'belt and braces', 'silver bullet',
];
const escape = (w) => w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&').replace(/ /g, '\\s+');
const BANNED_RE = new RegExp(`(?<![\\w-])(${BANNED.map(escape).join('|')})(?![\\w-])`, 'gi');
const DASH_RE = /[\u2014\u2013]|(?<=\s)--(?=\s)/g;

// Blank out code fences, inline code, link targets and HTML tags, keeping line breaks.
function prose(text) {
  const blank = (s) => s.replace(/[^\n]/g, ' ');
  return text
    .replace(/^([ \t]*)(```|~~~)[^\n]*\n[\s\S]*?\n[ \t]*\2[^\n]*$/gm, blank)
    .replace(/`[^`\n]*`/g, blank)
    .replace(/\]\([^)\n]*\)/g, (s) => '](' + blank(s.slice(2)))
    .replace(/<[^>\n]+>/g, blank)
    .replace(/https?:\/\/\S+/g, blank);
}

function lineOf(text, index) {
  return text.slice(0, index).split('\n').length;
}

const args = process.argv.slice(2);
const files = args.length
  ? args
  : execFileSync('git', ['ls-files', '*.md'], { encoding: 'utf8' }).split('\n').filter(Boolean);
const problems = [];
for (const file of files) {
  const p = prose(readFileSync(file, 'utf8'));
  for (const m of p.matchAll(BANNED_RE)) {
    problems.push(`${file}:${lineOf(p, m.index)}: banned word "${m[0].replace(/\s+/g, ' ')}"`);
  }
  for (const m of p.matchAll(DASH_RE)) {
    problems.push(`${file}:${lineOf(p, m.index)}: dash used as punctuation`);
  }
}
for (const line of problems) console.error(line);
console.info(`prosecheck: ${files.length} files, ${problems.length} problems`);
process.exit(problems.length ? 1 : 0);
