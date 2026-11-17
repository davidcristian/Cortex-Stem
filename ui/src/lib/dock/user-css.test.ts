import { describe, expect, it } from 'vitest';

import type { UserCss } from '$lib/ipc/generated/UserCss';
import type { UserCssProblem } from '$lib/ipc/generated/UserCssProblem';

// The sources themselves, as text (Vite's `?raw`), for the drift checks.
import theming from '../../../../docs/THEMING.md?raw';
import dock from '../../dock/Dock.svelte?raw';

import {
  applyUserCss,
  cssOf,
  nextUserCss,
  NO_USER_CSS,
  STEM_CSS_VARIABLES,
  USER_CSS_ELEMENT_ID,
  userCssStatus,
} from './user-css';

const PATH = 'C:\\Users\\me\\AppData\\Roaming\\dev.davidcristian.stem\\dock.css';
/** A read of `dock.css`: its text, or no text and why (`notFound` unless named). */
const file = (css: string | null, problem: UserCssProblem = 'notFound'): UserCss => ({
  path: PATH,
  css,
  problem: css === null ? problem : null,
});

/** Every `--stem-<name>` in `text`. */
function variables(text: string): string[] {
  return [...new Set(text.match(/--stem-[a-z0-9-]*[a-z0-9]/g) ?? [])].sort();
}

describe('the stable variables (docs/THEMING.md)', () => {
  const stable = [...STEM_CSS_VARIABLES].sort();

  it('are the ones docs/THEMING.md documents', () => {
    expect(variables(theming)).toEqual(stable);
  });

  it('are the ones the dock page reads, each with a preset fallback, and it sets none of them', () => {
    const read = [...dock.matchAll(/var\((--stem-[a-z0-9-]+),\s*var\(--preset-/g)].map((m) => m[1]);
    expect([...new Set(read)].sort()).toEqual(stable);
    // A value the page defined itself would hide one the user set further up.
    expect(dock).not.toMatch(/--stem-[a-z0-9-]+\s*:/);
  });
});

describe('nextUserCss', () => {
  it('applies the start answer, then every event', () => {
    let view = nextUserCss(NO_USER_CSS, 'fetch', file('a {}'));
    expect(view).toEqual({ css: 'a {}', fromEvent: false });
    view = nextUserCss(view, 'event', file('b {}'));
    expect(view).toEqual({ css: 'b {}', fromEvent: true });
    view = nextUserCss(view, 'event', file(null));
    expect(view).toEqual({ css: null, fromEvent: true });
  });

  it('drops a start answer that arrives after an event: the event is newer', () => {
    const view = nextUserCss(NO_USER_CSS, 'event', file('new {}'));
    expect(nextUserCss(view, 'fetch', file('old {}'))).toBe(view);
  });

  it('treats no file and a blank one alike: no element', () => {
    expect(cssOf(file(null))).toBeNull();
    expect(cssOf(file(' \n\t'))).toBeNull();
    expect(cssOf(file('/* template */'))).toBe('/* template */');
  });
});

/** Just enough of a document for `applyUserCss`: a head with element children and lookup by id. */
class FakeElement {
  id = '';
  textContent: string | null = null;
  readonly dataset: Record<string, string> = {};
  parent: FakeHead | null = null;

  constructor(readonly tagName: string) {}

  remove(): void {
    this.parent?.children.splice(this.parent.children.indexOf(this), 1);
    this.parent = null;
  }
}

class FakeHead {
  readonly children: FakeElement[] = [];

  get lastElementChild(): FakeElement | null {
    return this.children.at(-1) ?? null;
  }

  append(element: FakeElement): void {
    element.remove();
    element.parent = this;
    this.children.push(element);
  }
}

class FakeDocument {
  readonly head = new FakeHead();

  getElementById(id: string): FakeElement | null {
    return this.head.children.find((e) => e.id === id) ?? null;
  }

  createElement(tag: string): FakeElement {
    return new FakeElement(tag);
  }
}

describe('applyUserCss', () => {
  // The page's own stylesheets, as Vite links them.
  function page(): { fake: FakeDocument; doc: Document } {
    const fake = new FakeDocument();
    fake.head.append(new FakeElement('link'));
    fake.head.append(new FakeElement('link'));
    return { fake, doc: fake as unknown as Document };
  }

  it('puts the css in one style element after the page\u2019s own styles and replaces its text', () => {
    const { fake, doc } = page();
    expect(applyUserCss(doc, 'a {}')).toBe(true);
    const element = fake.head.lastElementChild!;
    expect(element.tagName).toBe('style');
    expect(element.id).toBe(USER_CSS_ELEMENT_ID);
    expect(element.dataset.source).toBe('dock.css');
    expect(element.textContent).toBe('a {}');
    expect(fake.head.children).toHaveLength(3);

    expect(applyUserCss(doc, 'a {}')).toBe(false);
    expect(applyUserCss(doc, 'b {}')).toBe(true);
    expect(fake.head.children).toHaveLength(3);
    expect(fake.head.lastElementChild).toBe(element);
    expect(element.textContent).toBe('b {}');
  });

  it('moves the element back to the end when a stylesheet was added after it', () => {
    const { fake, doc } = page();
    applyUserCss(doc, 'a {}');
    const element = fake.head.lastElementChild!;
    fake.head.append(new FakeElement('style'));
    expect(applyUserCss(doc, 'a {}')).toBe(true);
    expect(fake.head.lastElementChild).toBe(element);
    expect(fake.head.children).toHaveLength(4);
  });

  it('takes the text as text: markup in it is never parsed', () => {
    const { fake, doc } = page();
    const hostile = '</style><script>alert(1)</script><style>';
    applyUserCss(doc, hostile);
    expect(fake.head.lastElementChild!.textContent).toBe(hostile);
    expect(fake.head.children.map((e) => e.tagName)).toEqual(['link', 'link', 'style']);
  });

  it('removes the element for no css, and does nothing when there is none', () => {
    const { fake, doc } = page();
    expect(applyUserCss(doc, null)).toBe(false);
    applyUserCss(doc, 'a {}');
    expect(applyUserCss(doc, null)).toBe(true);
    expect(fake.head.children.map((e) => e.tagName)).toEqual(['link', 'link']);
  });
});

describe('userCssStatus', () => {
  it('says whether the dock uses the file', () => {
    expect(userCssStatus(null)).toBe('Not read yet');
    expect(userCssStatus(file('a {}'))).toMatch(/^In use/);
    for (const last of ['none', 'edit', 'reload'] as const) {
      expect(userCssStatus(file('a {}'), last)).toMatch(/^In use/);
      expect(userCssStatus(null, last)).toBe('Not read yet');
    }
  });

  it('with no file yet, the normal case, says Edit creates it, not that something failed', () => {
    const none = userCssStatus(file(null));
    expect(none).toBe(userCssStatus(file(null), 'none'));
    expect(none).toMatch(/^Not in use: there is no dock\.css yet\. Edit dock\.css creates one/);
    expect(none).not.toMatch(/missing|unreadable|UTF-8|64 KiB/);
  });

  it('after Edit, asks for a Reload, whatever the last read found', () => {
    for (const problem of ['notFound', 'unreadable', 'notUtf8', 'tooLarge'] as const) {
      expect(userCssStatus(file(null, problem), 'edit')).toBe(
        'Not in use yet: save the file, then press Reload',
      );
    }
  });

  it('says why a file is not used, from the problem the read found, Reload or not', () => {
    for (const last of ['none', 'reload'] as const) {
      expect(userCssStatus(file(null, 'notFound'), last)).toBe(
        'Not in use: there is no dock.css yet. Edit dock.css creates one from a template',
      );
      expect(userCssStatus(file(null, 'unreadable'), last)).toBe(
        'Not in use: Stem cannot read the file (the log says why)',
      );
      expect(userCssStatus(file(null, 'notUtf8'), last)).toBe(
        'Not in use: the file is not UTF-8 text. Save it as UTF-8, then press Reload',
      );
      expect(userCssStatus(file(null, 'tooLarge'), last)).toBe(
        'Not in use: the file is larger than 64 KiB, the most Stem reads',
      );
    }
    // An older build's payload, without the field: no file.
    const old = { path: PATH, css: null } as unknown as UserCss;
    expect(userCssStatus(old)).toBe(userCssStatus(file(null, 'notFound')));
  });

  it('says a run that saves no files has no dock.css, whatever the user did', () => {
    const noFiles: UserCss = {
      path: '(not saved: the mock platform)',
      css: null,
      problem: 'notFound',
    };
    for (const last of ['none', 'edit', 'reload'] as const) {
      expect(userCssStatus(noFiles, last)).toBe(
        'Not in use: this run saves no files, so it has no dock.css',
      );
    }
  });
});
