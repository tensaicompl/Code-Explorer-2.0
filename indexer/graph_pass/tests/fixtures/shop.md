---
title: Shop fixture
status: throwaway
---

# Shop

A deliberately tiny project used to exercise the indexer end to end without
paying for a real re-index. Every extractor in `graph_pass/` has something to
chew on here.

## Schema

The `db/schema.sql` file declares the tables and the foreign key between them.

### Tables

This heading is level 3, so it folds into "Schema" as metadata rather than
becoming a node of its own.

## API

Defined in `api/shop.proto`.

```bash
# This is a shell comment inside a fence, NOT a heading.
# If it becomes a document node, the fence tracking is broken.
grpcurl -plaintext localhost:50051 list
```

## Configuration

See `.env.example`. Values there are placeholders; the indexer redacts them
either way.
