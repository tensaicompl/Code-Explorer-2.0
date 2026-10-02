# Corpus guide

A short guide with *emphasis*, **strong**, `code` and ~~strikethrough~~ — and accents: café.

## Lists

1. First
   - nested bullet
     - deeper
       1. numbered again
2. Second

- [x] done task
- [ ] open task

## Code

```python
def greet(name: str) -> str:
    return f"Bonjour, {name}"
```

```c
#define TWICE(x) ((x) * 2)
```

    indented code block

## Table

| Language | Files | Notes |
|---|---:|:---:|
| C | 3 | macros |
| Lua | 2 | "long strings" |

> A quote
> > nested quote

<details>
<summary>HTML block</summary>

Inline <span class="x">html</span> and a [reference link][ref].

</details>

[ref]: ./README.md "Title"

Footnote reference.[^1]

[^1]: The footnote.
