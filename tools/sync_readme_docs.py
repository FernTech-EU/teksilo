#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Sync marked book sections from README.md; --check rejects stale copies.

README.md remains readable on GitHub. Only marked regions are generated.
Book links are rebased, repository policy links are made absolute, and prose
punctuation is normalized. Run before mdbook build or the book link fixer.
"""

import argparse
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REPO = 'https://github.com/ferntech-eu/teksilo/blob/main/'
PAGES = (
    'introduction.md', 'features.md', 'first-application.md',
    'examples-and-tooling.md', 'status-and-limitations.md',
    'project-information.md',
)
REGION = re.compile(
    r'<!-- BEGIN README: (.+?) -->\n.*?<!-- END README: \1 -->', re.DOTALL
)


def normalize(text):
    text = re.sub(r'^(#{1,6} .+?) \u2014 ', r'\1: ', text, flags=re.MULTILINE)
    return text.replace(' \u2014 ', ', ').replace('\u2014', ', ')


def sections(source):
    """Index headings outside code fences, retaining nested subsections."""
    lines = source.splitlines(keepends=True)
    headings = []
    fenced = False
    for index, line in enumerate(lines):
        if line.startswith('```'):
            fenced = not fenced
        if not fenced and (match := re.match(r'^(#{2,3}) (.+)\n$', line)):
            headings.append((index, len(match[1]), normalize('## ' + match[2])[3:]))
    result = {}
    for pos, (start, level, title) in enumerate(headings):
        end = next((i for i, depth, _ in headings[pos + 1:] if depth <= level), len(lines))
        # The README has two Documentation sections, neither is mirrored.
        if title in result and title != 'Documentation':
            raise ValueError(f'Duplicate README heading: {title}')
        result[title] = ''.join(lines[start:end]).strip()
    examples = re.findall(r'```rust\n.*?```', source, re.DOTALL)
    result['hello'], result['counter'] = examples[:2]
    result['install'] = re.search(r'```sh\n.*?```', result['Getting started'], re.DOTALL)[0]
    return result


def book_text(text, key):
    text = normalize(text)
    text = re.sub(r'\]\(docs/([^)]*)\)', r'](\1)', text)
    text = re.sub(r'\[docs/([^\]]+)\]', r'[\1]', text)
    for name in ('LICENSE', 'TRADEMARKS.md', 'NOTICE'):
        text = re.sub(rf'\]\({re.escape(name)}\)', f']({REPO}{name})', text)
        text = text.replace(f'`{name}`', f'[{name}]({REPO}{name})')
        text = re.sub(rf'(?<![\w/\[\\`]){re.escape(name)}(?![\w\]/])',
                      f'[{name}]({REPO}{name})', text)
    text = text.replace('the boilerplate above',
                        'the [first application example](first-application.md)')
    text = text.replace('The known gaps are listed at the end of this README.',
                        'Known gaps are listed below.')
    text = text.replace('For depth on any of these, see `docs/`.',
                        'See the [guide index](guide-index.md) for detailed documentation.')
    if key.startswith('Tooling') or key == 'Running the demos':
        text = re.sub(r'^### ', '## ', text, count=1)
    if key.startswith("What's in the box"):
        text = text.split('\n', 1)[1].lstrip()
    return text


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    shared = sections((ROOT / 'README.md').read_text())
    stale = []
    for name in PAGES:
        path = ROOT / 'docs' / name
        original = path.read_text()

        def replace(match):
            key = match[1]
            body = book_text(shared[key], key)
            return f'<!-- BEGIN README: {key} -->\n{body}\n<!-- END README: {key} -->'

        updated, count = REGION.subn(replace, original)
        if not count:
            raise ValueError(f'No shared sections in {path}')
        if original != updated:
            stale.append(name)
            if not args.check:
                path.write_text(updated)
    if args.check and stale:
        parser.exit(1, 'Stale README sections: ' + ', '.join(stale)
                    + '\nRun python3 tools/sync_readme_docs.py\n')
    print('README documentation sections are current.' if args.check
          else f'Updated {len(stale)} documentation pages.')


if __name__ == '__main__':
    main()
