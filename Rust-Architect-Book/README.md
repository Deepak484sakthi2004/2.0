# Source to Silicon: Rust for the Systems Architect

A book-length, architect-level Rust course for an experienced Java/backend engineer. It is written incrementally, one Part at a time.

| Path | What it is |
|---|---|
| `src/` | The book itself (mdBook layout). Start at `src/preface.md`, then follow `src/SUMMARY.md`. |
| `listings/` | Every Rust listing in the book as a standalone file with `// verify:` headers. |
| `tools/verify.ps1` | Compiles/runs listings on the Rust Playground and checks each header (expected output, panic, crash, test run, or exact compiler error code). |
| `tools/emit.ps1` | Fetches what rustc generates for a listing (assembly, LLVM IR, MIR, HIR, macro expansion) from the Playground. |
| `SPEC.md` | The authoring brief the book follows (scope, chapter template, teaching philosophy). |
| `PROGRESS.md` | Continuity ledger: what's written, concepts introduced, promises made to later Parts. |
| `CLAUDE.md` | Working rules for continuing the book in a later session. |

## Reading it

- **Plain Markdown:** open `src/` in VS Code (Markdown preview) or any Markdown viewer. Chapters are ordinary `.md` files.
- **As a website:** install [mdBook](https://rust-lang.github.io/mdBook/) (a single prebuilt binary, a few MB, from its GitHub releases page) and run `mdbook serve` in this folder.

## Verifying the code

No local Rust toolchain is required:

```powershell
powershell -ExecutionPolicy Bypass -File tools\verify.ps1 listings\part-01
```

The script sends each listing to the public Rust Playground (play.rust-lang.org), stable channel, edition 2024.
