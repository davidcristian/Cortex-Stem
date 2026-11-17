// Checks the file size limits of AGENTS.md: a markdown file has at most 250 lines, and a source
// file (.rs, .ts, .svelte, .js, .mjs) has at most 300. Test files (tests.rs, *_tests.rs, anything
// under a tests/ folder, *.test.ts, *.spec.ts, e2e/) and the generated bindings in
// src/lib/ipc/generated/ have no limit.
// Usage: node scripts/linecap.mjs [file ...]   (default: every tracked markdown and source file)
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

const MARKDOWN_LIMIT = 250;
const SOURCE_LIMIT = 300;
const SOURCE = /\.(rs|ts|svelte|js|mjs)$/;
const EXEMPT =
  /(^|\/)tests\.rs$|_tests\.rs$|(^|\/)tests\/|\.test\.ts$|\.spec\.ts$|^e2e\/|^src\/lib\/ipc\/generated\//;

function limitOf(file) {
  if (file.endsWith('.md')) return MARKDOWN_LIMIT;
  if (SOURCE.test(file) && !EXEMPT.test(file)) return SOURCE_LIMIT;
  return null;
}

const args = process.argv.slice(2);
const files = args.length
  ? args
  : execFileSync('git', ['ls-files', '*.md', '*.rs', '*.ts', '*.svelte', '*.js', '*.mjs'], {
      encoding: 'utf8',
    })
      .split('\n')
      .filter(Boolean);
const problems = [];
let checked = 0;
for (const file of files) {
  const limit = limitOf(file.replaceAll('\\', '/'));
  if (limit === null) continue;
  checked += 1;
  const text = readFileSync(file, 'utf8');
  const lines = text.endsWith('\n') ? text.split('\n').length - 1 : text.split('\n').length;
  if (lines > limit) problems.push(`${file}: ${lines} lines, the limit is ${limit}`);
}
for (const line of problems) console.error(line);
console.info(`linecap: ${checked} files checked, ${problems.length} over the limit`);
process.exit(problems.length ? 1 : 0);
