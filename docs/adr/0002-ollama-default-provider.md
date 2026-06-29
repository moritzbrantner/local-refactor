# Ollama Default Provider

Ollama is the default local model provider, accessed through a provider boundary so other local or OpenAI-compatible runtimes can be added later. V1 can run deterministic rules without a model, but model planning should enter through this adapter rather than writing files directly.

