# Model-list fixtures

Read on 2026-10-13. Only OpenRouter's list answers without a key, so it is the one recorded
response; the others are built from each provider's own documentation, in the shape its list
endpoint answers.

| File | Source |
|---|---|
| `openrouter-models-2026-10-13.json` | `GET https://openrouter.ai/api/v1/models`, the ids in the order it answered. |
| `openai-models-2026-10-13.json` | Every model page under `developers.openai.com/api/docs/models`, with the snapshots each page lists, in the `GET /v1/models` shape. GPT-Rosalind (`gpt-rosalind-research`), which the catalogue offers to approved organizations without a model page, is not among them. |
| `openai-chat-completions-2026-10-13.json` | The `v1/chat/completions` row of each of those pages: `true` for Supported, `false` for Not supported. A snapshot takes its model's answer. `gpt-3.5-turbo-instruct`, which the GPT-3.5 Turbo page also lists among its snapshots, has a page of its own, `developers.openai.com/api/docs/models/gpt-3.5-turbo-instruct`, which says it is "Compatible with legacy Completions endpoint and not Chat Completions." The endpoint table on that page reads Supported for Chat Completions and Not supported for Completions (legacy), against its own description; the description is what the fixture records. |
| `anthropic-models-2026-10-13.json` | The active and deprecated rows of `platform.claude.com/docs/en/about-claude/model-deprecations`, which is what `GET /v1/models` lists when `lifecycle` is omitted. |
| `gemini-models-2026-10-13.json` | The ids on `ai.google.dev/gemini-api/docs/models`. The generation methods follow each model's kind: text, image and speech models answer `generateContent`, live models `bidiGenerateContent`, embedding models `embedContent`. |
| `together-models-2026-10-13.json` | The chat, image and video tables of `docs.together.ai/docs/serverless-models`, in the bare-array shape `GET /v1/models` answers (`docs.together.ai/reference/models`), with each entry's `type`. |

## The provider table's ids

Every hosted row of `writ_core::ai::providers::PROVIDERS` names its default model first and its
other suggestions after it. Each was read from the pages below on 2026-10-13.

| Row | Ids | Read from |
|---|---|---|
| OpenAI | `gpt-6-luna`, `gpt-6.1-sol`, `gpt-6-astra` | `https://developers.openai.com/api/docs/models`, which names GPT-6 Luna for cost-sensitive, high-volume workloads, and `https://developers.openai.com/api/docs/models/gpt-6-luna` |
| Anthropic | `claude-sonnet-5-5`, `claude-haiku-5-5`, `claude-opus-5-5` | `https://platform.claude.com/docs/en/models/overview` and `https://platform.claude.com/docs/en/models/sonnet-5-5/overview` |
| Google Gemini | `gemini-3.8-flash`, `gemini-3.5-flash-lite` | `https://ai.google.dev/gemini-api/docs/models` |
| OpenRouter | `meta-llama/llama-3.3-70b-instruct`, `openai/gpt-6-luna`, `google/gemma-4-26b-a4b-it:free` | `GET https://openrouter.ai/api/v1/models`, the fixture above |
| Groq | `openai/gpt-oss-120b`, `openai/gpt-oss-20b` | `https://console.groq.com/docs/models` and `https://console.groq.com/docs/deprecations`, which shut Llama 3.3 70B and Llama 3.1 8B down for the free and developer tiers on 2026-08-16 and name these two in their place |
| DeepSeek | `deepseek-flash`, `deepseek-v4-pro` | `https://api-docs.deepseek.com/quick_start/pricing` |
| Mistral | `mistral-small-latest`, `mistral-large-latest` | `https://docs.mistral.ai/models/mistral-small-4-0-26-03` (Mistral Small 4, `mistral-small-2603`) and `https://docs.mistral.ai/models/mistral-large-3-25-12` (Mistral Large 3, `mistral-large-2512`) |
| xAI | `grok-4.3`, `grok-4.7` | `https://docs.x.ai/docs/models` and `https://docs.x.ai/developers/migration/may-15-retirement`, which retired `grok-4-0709` on 2026-05-15 and answers it with `grok-4.3` |
| Together | `openai/gpt-oss-120b`, `Qwen/Qwen3.8-Flash`, `deepseek-ai/DeepSeek-V4.1-Flash` | `https://docs.together.ai/docs/serverless-models`; the host, `api.together.ai`, is the one `https://docs.together.ai/reference/models` and `https://docs.together.ai/docs/quickstart` name |
| Fireworks | `accounts/fireworks/models/glm-5p3-flash`, `accounts/fireworks/models/gpt-oss-120b` | `https://docs.fireworks.ai/serverless/pricing` and `https://docs.fireworks.ai/getting-started/quickstart`, whose model is GLM 5.3 Flash; `https://fireworks.ai/models/fireworks/llama-v3p3-70b-instruct` lists Serverless as not supported |
