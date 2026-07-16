# CrowQuant integration provenance

CrowClaw Alpha 2 integrates CrowQuant as its local memory-vector compression
and retrieval substrate.

## Source binding

- Source repository: `https://github.com/CrowLoki/crowquant`
- Audited source commit: `b4f3f640e99289f6b40591f49bd5941c726361ce`
- Reference implementation: `crowquant/core.py` and `crowquant/search.py`
- Reference gate run before integration: 41 tests passed

The upstream package is Python/NumPy and its current package backend and CLI
are not deployable as a standalone Windows dependency. CrowClaw therefore
contains a focused native Rust port of the verified fixed-profile pipeline:

1. randomized Walsh-Hadamard rotation using the reference seed 42 signs;
2. Gaussian Lloyd-Max scalar quantization at 4 bits;
3. MSB-first packed indices;
4. the reference little-endian `<ddBIIII` block format; and
5. cosine ranking directly over compressed centroid values.

A golden test binds the native quantizer to the reference implementation's
scale, zero point, packed indices, metadata layout, and round-trip behavior.

## Host-owned text vectorization

CrowQuant compresses vectors; it does not create embeddings. Alpha 2 uses an
explicitly labelled deterministic local lexical vectorizer owned by CrowClaw.
It hashes words, adjacent word pairs, and character trigrams into a normalized
256-dimensional vector before CrowQuant compression. This provides useful
offline lexical recall without Python, a model API, a network call, or a hidden
account dependency. It is not represented as a neural semantic embedding.

## Durable boundary

CrowClaw owns the SQLite database and stores each memory's text, serialized
CrowQuant block, exact algorithm metadata, dimensions, bit width, seed, source
size, and creation time. Recall validates every stored block before ranking.
The migration, retention removal, and export paths include these records.

## Agent-facing approval boundary

The conversational runtime exposes two explicit tools over this same native
service and SQLite database:

- `remember_memory` proposes the exact text to compress and persist;
- `search_memory` proposes the exact lexical query and result limit.

Both tools always stop at CrowClaw's existing single-use approval boundary.
Proposal parsing performs no CrowQuant read or write. A denied or pre-execution
cancelled call never reaches the memory service. Once approved, the real tool
result is bound to its provider call ID, persisted in the approved-action
audit, and returned to the connected model. Search approval therefore states
plainly that matching stored text will be returned to both the model and audit.
Batched tool proposals retain independent decisions and exact result
binding.

This integration does not edit or replace the existing CrowQuant repository.
