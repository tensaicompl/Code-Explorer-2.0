# Third-party notices

Legal notices for third-party code and models in this repository. This file is a
legal document, not code: it is the one place where upstream projects, authors
and licences are named, and it is exempt from the rule that forbids such
references elsewhere in the tree.

Every entry gives the component, its licence, its copyright holders and where the
full licence text lives. Retain this file in every copy of the software.

## Incorporated source

Source code from other projects that is present in this repository.

### Web dashboard (user interface of the predecessor application)

- **Paths**: `frontend/packages/dashboard/`, `frontend/packages/core/`
  (moved to `legacy/frontend/` when the tree is restructured, and removed when
  the replacement interface supersedes it)
- **Licence**: MIT
- **Copyright**: Copyright (c) 2026 Yuxiang Lin; Copyright (c) 2026 Infinite
  Universe, Inc.
- **Upstream project**: Understand-Anything, originally published by Lum1104 and
  later maintained under the Egonex-AI organisation
- **Licence text**: reproduced in full below

The interface code in those directories was ported from that project. The MIT
licence requires its copyright notice and permission notice to be included in
all copies or substantial portions of the software; that requirement is met by
this entry.

    MIT License

    Copyright (c) 2026 Yuxiang Lin
    Copyright (c) 2026 Infinite Universe, Inc.

    Permission is hereby granted, free of charge, to any person obtaining a copy
    of this software and associated documentation files (the "Software"), to deal
    in the Software without restriction, including without limitation the rights
    to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
    copies of the Software, and to permit persons to whom the Software is
    furnished to do so, subject to the following conditions:

    The above copyright notice and this permission notice shall be included in all
    copies or substantial portions of the Software.

    THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
    IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
    FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
    AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
    LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
    OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
    SOFTWARE.

## Dependencies

Not written by Praxevia and not incorporated as source. Their own terms apply and
this attribution is retained. Licence obligations of ordinary dependency use are
satisfied by this list.

| Component | Licence |
|---|---|
| PostgreSQL | PostgreSQL Licence |
| pgvector | PostgreSQL Licence |
| sentence-transformers | Apache-2.0 |
| Alibaba-NLP/gte-base-en-v1.5 | Apache-2.0 |
| tree-sitter and its grammars | MIT / Apache-2.0 |
| React, Vite, TailwindCSS | MIT |
| @xyflow/react | MIT |
| elkjs | EPL-2.0 |
| zod, react-markdown, remark | MIT |
| FastAPI, Uvicorn, Pydantic | MIT / BSD |
| psycopg2 | LGPL-3.0-with-exceptions |
| Anthropic SDK | MIT |

If the software is ever shipped as a self-contained bundle that embeds the source
or object code of these components, each licence's redistribution terms must be
reviewed first: elkjs (EPL-2.0) and psycopg2 (LGPL-3.0-with-exceptions) carry
obligations that ordinary dependency use does not trigger.

## Algorithms and designs consulted

Re-implementations written from scratch in this repository, with the reference
whose algorithm or design was consulted. These are original expression and are
not incorporated source; they are listed for completeness, without file paths.

No entries yet. Each is added by the task that introduces the re-implementation.

## Vendored components

Third-party source copied into this repository with its licence file alongside
it. Each entry names the directory holding the licence text.

No entries yet. Each is added by the task that vendors the component.
