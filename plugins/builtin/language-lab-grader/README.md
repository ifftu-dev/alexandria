# Language Lab Grader

The deterministic WASM grader for the built-in
[Language Lab](../language-lab/README.md) plugin. It runs inside the host's
[`plugins/wasm_runtime`](../../../src-tauri/src/plugins/wasm_runtime.rs)
sandbox, under the same ABI v1 as the [MCQ grader](../mcq-grader/README.md):

```
(export "alex_alloc"   (func (param i32) (result i32)))
(export "alex_dealloc" (func (param i32 i32)))
(export "alex_grade"   (func (param i32 i32) (result i64)))
(export "memory"       (memory))
```

Input is `{"version":"1","content":{...},"submission":{...}}`, where
`content` is the element's full `content_inline` including the top-level
`grader_private` block (the host strips that block before the plugin UI sees
the content). Output is `{"version":"1","score":0..1,"details":{...}}`. Bad
input gives a score of 0 and `details.error`, never a trap.

## Activities

`content.activity` picks the scorer. Text is compared after normalization:
lowercase, punctuation removed (apostrophes inside words kept), spaces
collapsed.

| Activity | Content (grader_private) | Submission | Score |
|---|---|---|---|
| `dictation` | `items:[{text, hint_hi?}]` | `answers:[string]` | Per item: word-level LCS ÷ max(expected words, typed words). Overall: mean. Details hold a per-word `ok`/`missing`/`extra` diff. |
| `reading` | `passage`, `glossary`, `questions:[{prompt, options}]` (`answers:[index]`) | `selected:[index or null]` | Correct ÷ total. |
| `writing` | `items:[{prompt_hi, prompt_en?, hint_hi?}]` (`items:[{accept?, require?, min_words?, checks?}]`) | `answers:[string]` | Per item: 1.0 if it equals an `accept` answer; otherwise the fraction of checks passed. Overall: mean. |
| `builder` | `items:[{tiles, hint_hi?}]` (`items:[{answer, also?}]`) | `orders:[[tile index]]` | An item is right when the joined tiles equal `answer` or one of `also`. Correct ÷ total. |
| `read_aloud` | — | — | Not graded: instructors review recordings. Returns 0 with an error. |

Writing checks are `capital` (starts with a capital letter),
`end_punctuation` (ends with `.`, `?` or `!`) and `articles` (a/an agrees with
the next word's spelling, with exceptions such as *an hour* and *a
university*). All three run unless the item lists its own `checks`. Each
`require` phrase and `min_words` add one check each. `details.items[i].failed`
names the checks that failed.

## Build

```bash
cargo test
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/language_lab_grader.wasm dist/
```

Then put the BLAKE3 of `dist/language_lab_grader.wasm` into `grader.cid` and
`grader.blake3` in `../language-lab/manifest.json`. Installing the plugin
fails if they differ. `dist/` is checked in, so rebuild and commit it after
any source change.

## Determinism

No clock, RNG, filesystem or network (no imports are linked). Item scores
are integers in basis points (0 to 10,000); the only float operation is the
final division of two integers.
