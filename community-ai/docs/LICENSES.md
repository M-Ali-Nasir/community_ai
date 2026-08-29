# Model licence audit

## Why this file exists

This project copies model weights onto other people's computers. That changes
the licence question from "may I use this model?" to "may I put this model on my
friend's machine, and does my friend become a licensee?"

Two rules follow, and the default catalogue in
`packages/protocol/src/models.ts` enforces both:

1. **Apache-2.0 or MIT only.** Both permit redistribution without conditions
   that propagate to every volunteer.
2. **Ungated on Hugging Face.** A gated repository requires each person to
   accept terms individually before downloading. A worker cannot silently accept
   terms on its owner's behalf, so gated weights cannot be a default.

Community licences such as Llama's and Gemma's are not "not allowed" — they are
just a poor fit for automated fan-out to volunteer machines, and they are not
worth the friction when Apache-2.0 alternatives of the same quality exist.

## Included in the default catalogue

| Model | Params | Licence | Gated | Notes |
| --- | --- | --- | --- | --- |
| SmolLM2 360M Instruct | 0.36B | Apache-2.0 | No | Only q8_0 published upstream, so the GGUF is ~386 MB |
| Qwen2.5 0.5B Instruct | 0.49B | Apache-2.0 | No | Default. Runs on CPU-only machines |
| Qwen2.5 1.5B Instruct | 1.54B | Apache-2.0 | No | Comfortable on any discrete GPU |
| SmolLM2 1.7B Instruct | 1.7B | Apache-2.0 | No | Apache-2.0 alternative in the same class |
| Phi-3.5 Mini Instruct | 3.8B | MIT | No | Strongest small model here |
| Mistral 7B Instruct v0.3 | 7.25B | Apache-2.0 | No | Ungated Apache-2.0 7B |
| Qwen2.5 7B Instruct | 7.61B | Apache-2.0 | No | Target once friends with 8 GB+ GPUs join |

Both Apache-2.0 and MIT require the licence text and copyright notice to travel
with redistributed weights. Hugging Face repositories carry these, and pointing
`resolveModelFile` at the upstream repository rather than re-hosting keeps that
intact. **Do not re-host these weights on your own server without also copying
the LICENSE and NOTICE files.**

## Excluded, and why

### Qwen2.5 3B and 72B

The Qwen2.5 collection is Apache-2.0 **with two exceptions**. The 3B model is
released under `qwen-research`, and 72B under the Qwen License; both are marked
`other` on Hugging Face. Research-only terms do not obviously cover distributing
shards to a group of volunteers, so both are excluded. Every other size in the
family is Apache-2.0, which is why 0.5B, 1.5B and 7B are in the catalogue and
3B is conspicuously absent.

### Llama 3.1 8B Instruct

The Llama 3.1 Community License does permit this use. A hobby testnet is many
orders of magnitude below the 700M monthly-active-user threshold, and the
conditions are satisfiable:

- display "Built with Llama"
- include a copy of the licence
- ship the `Notice` text with distributed copies
- pass through the Acceptable Use Policy
- do not use the outputs to train a non-Llama model

It is excluded from the *defaults* purely because the repository is gated: every
volunteer would have to accept Meta's terms on Hugging Face before their worker
could download anything. If you want it, add it to the catalogue yourself and
tell your friends what they are agreeing to.

### Gemma 2

Same shape as Llama. The Gemma Terms of Use are usable at this scale, the
repository is gated, and the prohibited-use policy must be passed through to
every node. Not a default for the same reason.

### DeepSeek R1 distills

Licensing varies per distill depending on the base model. The Qwen-based ones
inherit Qwen's terms, the Llama-based ones inherit Llama's. Excluded until each
variant is checked individually rather than assumed.

## Browser models

The PWA uses WebLLM's prebuilt MLC conversions. These are format conversions of
the same upstream weights and carry the same licences. WebLLM's model ids drift
between releases, so the catalogue stores a *match prefix* and resolves it
against `prebuiltAppConfig.model_list` at runtime — an upstream rename shows up
as "unavailable in browser" rather than silently loading something else.

## If you add a model

1. Check the `license` field on the Hugging Face model page, not the paper or
   the blog post.
2. Check whether the repository is gated. Open it in a logged-out browser.
3. If it is not Apache-2.0 or MIT, write down what obligations propagate to
   people running workers, and tell them before they join.
4. Add it to `MODEL_CATALOG`, and add anything you rejected to
   `EXCLUDED_MODELS` with the reason. That map exists so the decision stays
   auditable instead of becoming folklore.

*Licences change. This audit reflects what the upstream repositories stated when
the catalogue was assembled; re-check before relying on it.*
