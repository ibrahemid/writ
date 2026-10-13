# Model-list fixtures

Read on 2026-10-13. Only OpenRouter's list answers without a key, so it is the one recorded
response; the others are built from each provider's own documentation, in the shape its list
endpoint answers.

| File | Source |
|---|---|
| `openrouter-models-2026-10-13.json` | `GET https://openrouter.ai/api/v1/models`, the ids in the order it answered. |
| `openai-models-2026-10-13.json` | Every model page under `developers.openai.com/api/docs/models`, with the snapshots each page lists, in the `GET /v1/models` shape. GPT-Rosalind (`gpt-rosalind-research`), which the catalogue offers to approved organizations without a model page, is not among them. |
| `openai-chat-completions-2026-10-13.json` | The `v1/chat/completions` row of each of those pages: `true` for Supported, `false` for Not supported. A snapshot takes its model's answer, except `gpt-3.5-turbo-instruct`: the GPT-3.5 Turbo page lists it among its snapshots, and as an instruct model it answers only the legacy completions endpoint. |
| `anthropic-models-2026-10-13.json` | The active and deprecated rows of `platform.claude.com/docs/en/about-claude/model-deprecations`, which is what `GET /v1/models` lists when `lifecycle` is omitted. |
| `gemini-models-2026-10-13.json` | The ids on `ai.google.dev/gemini-api/docs/models`. The generation methods follow each model's kind: text, image and speech models answer `generateContent`, live models `bidiGenerateContent`, embedding models `embedContent`. |
| `together-models-2026-10-13.json` | The chat, image and video tables of `docs.together.ai/docs/serverless-models`, in the bare-array shape `GET /v1/models` answers (`docs.together.ai/reference/models`), with each entry's `type`. |
