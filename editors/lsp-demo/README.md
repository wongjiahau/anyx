# anyx language server demo

Monaco editing a `.poentaro` score, with diagnostics, completion, formatting
and folding from poentaro's grammar alone. The language server is
[`anyx-lsp`](../../crates/anyx-lsp) compiled to WebAssembly, running in a Web
Worker, so the page is static: no server behind it.

```sh
npm install
npm run dev     # builds the WebAssembly, then serves the page with Vite
npm run build   # writes the static page to dist/, for any static host
```

Both need [`wasm-pack`](https://rustwasm.github.io/wasm-pack/).

## For your own grammar

1. A `cdylib` crate with the decoder anyx-generate wrote, like [`wasm/`](wasm):

   ```rust
   mod generated;
   anyx_lsp::language!(MyLanguage = generated);
   anyx_lsp::wasm!(MyLanguage);
   ```

   with `anyx-lsp = { …, features = ["wasm"] }`.
2. `wasm-pack build --target web`.
3. A worker that loads it and passes messages through, like
   [`src/worker.ts`](src/worker.ts), and an LSP client talking to that
   worker, like [`src/main.ts`](src/main.ts).

## CI and deployment

The [CI/CD workflow](../../.github/workflows/ci.yml) runs on every pull request
and push to `master`, and can also be started manually. It checks Rust formatting,
runs Clippy with warnings as errors, tests every workspace crate with all features,
and builds and type-checks this demo with Node.js 24 and wasm-pack 0.15.1.
Cargo and npm use their checked-in lockfiles.

After both jobs pass on `master`, the workflow deploys `dist/` to GitHub Pages.
Before the first deployment, select **Settings → Pages → Build and deployment →
Source → GitHub Actions** in the repository. The `github-pages` environment can
be configured to require deployment approval. Pull requests only run checks;
manual runs on other branches do not deploy. The deployment URL appears in the
workflow's deployment environment.
