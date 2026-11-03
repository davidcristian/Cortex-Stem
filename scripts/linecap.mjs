// Checks the file size limit of AGENTS.md: a markdown file has at most 250 lines.
// Usage: node scripts/linecap.mjs [file ...]   (default: every tracked .md file)
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

const MARKDOWN_LIMIT = 250;

const args = process.argv.slice(2);
const files = args.length
  ? args
  : execFileSync('git', ['ls-files', '*.md'], { encoding: 'utf8' }).split('\n').filter(Boolean);
const problems = [];
for (const file of files) {
  const text = readFileSync(file, 'utf8');
  const lines = text.endsWith('\n') ? text.split('\n').length - 1 : text.split('\n').length;
  if (file.endsWith('.md') && lines > MARKDOWN_LIMIT) {
    problems.push(`${file}: ${lines} lines, the limit is ${MARKDOWN_LIMIT}`);
  }
}
for (const line of problems) console.error(line);
console.info(`linecap: ${files.length} files, ${problems.length} over the limit`);
process.exit(problems.length ? 1 : 0);
