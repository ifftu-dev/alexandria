# alexandria-ai

Provider-independent model access shared by Studio, the lesson tutor, and global chat.

- `endpoint`: where a request may go. A connection declares `device`, `private_network`, or `remote`; the URL and every resolved address must agree, and requests are pinned to the resolved addresses. Redirects are never followed.
- `model`: canonical requests, streamed events, completions, and request bounds.
- `compatible`: the OpenAI-compatible chat-completions protocol, streamed or not, plus model discovery.
- `anthropic`: the Anthropic Messages protocol at API version `2026-10-21`. System messages become the top-level `system` field, same-role turns merge, and a trailing assistant turn is refused before sending. Sampling parameters are never sent. History is replayed as text without thinking blocks. A fixed thinking allowance is added to `max_tokens` because current models think by default.
- `Adapter` and `Protocol`: one adapter per connection, chosen by the protocol the connection declares.

The crate knows nothing about profiles, courses, or learner records. The host assembles an immutable `ChatRequest`, reads the key, and hands both to an adapter. Every `Error` displays a message safe to show a learner: no keys, request bodies, or provider response bodies. `Error::may_have_been_accepted` tells callers when a failed request may still have been billed, so they never retry those automatically.

```sh
cargo test -p alexandria-ai
```
