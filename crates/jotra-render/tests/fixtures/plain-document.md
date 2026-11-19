---
title: Plain document
tags: [a, b]
---

# Heading

A paragraph with `inline code`, *emphasis*, **strong**, ~~struck~~ text and a
[link](https://example.com).

A wikilink to [[Note]], one to [[Missing]], one to [[Both]], one with an alias
[[Note|the note]] and one with a heading [[Note#Some Heading]].

Inline math $x^2 + y^2$ and a display block:

$$
\int_0^1 x\,dx
$$

| Column | Other |
| --- | ---: |
| one | two |
| three | four |

```mermaid
graph TD
  A --> B
```

```rust
let x = 1;
```

![alt text](img.png)

![[picture.png]]

![[wide.png|300]]

<img src="raw.png" alt="raw">

- a list item
- another, with [[Note]] in it

> A quotation with no callout marker.

1. numbered
2. list

- [ ] a task
- [x] a done task

Footnote reference[^1].

[^1]: The footnote body.
