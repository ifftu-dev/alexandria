# alexandria-ai

Provider-independent model access shared by Studio, the lesson tutor, and global chat.

- `endpoint`: where a request may go. A connection declares `device`, `private_network`, or `remote`; the URL and every resolved address must agree, and requests are pinned to the resolved addresses. Redirects are never followed.
- `model`: canonical requests, streamed events, completions, and request bounds.
- `compatible`: the OpenAI-compatible chat-completions protocol, streamed or not, plus model discovery.

The crate knows nothing about profiles, courses, or learner records. The host assembles an immutable `ChatRequest`, reads the key, and hands both to an adapter. Every `Error` displays a message safe to show a learner: no keys, request bodies, or provider response bodies. `Error::may_have_been_accepted` tells callers when a failed request may still have been billed, so they never retry those automatically.

```sh
cargo test -p alexandria-ai
```
