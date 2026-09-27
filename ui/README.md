# ui

The web interface. Apache-2.0; see `LICENSE` at the repository root.

Served from embedded assets by the local binary and by the server: one bundle for
both. Nothing is fetched from the network at runtime — no fonts, icons, scripts or
tiles from outside — so it renders fully offline.

## Development

```
pnpm install
pnpm dev
pnpm lint      # eslint + tsc --noEmit
pnpm test
```

Node 22 and pnpm 10, both pinned. The bundle has a hard budget of 2.5 MB gzipped,
enforced by `make bundle-budget`.
