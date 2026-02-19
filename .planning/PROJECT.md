# lmlang

## What This Is

An AI-native programming system where programs are persistent dual-layer graphs (semantic + executable) manipulated by AI agents through a structured tool API and compiled to native binaries via LLVM.

## Core Value

AI agents can build, modify, and verify programs of arbitrary size with perfect local and global awareness.

## Current Milestone: v1.1 milestone-2

**Goal:** Define and execute the next scoped increment after baseline v1.0 completion.

**Target features:**
- Finalize v1.1 requirements and scope boundaries
- Create a new phased roadmap from those requirements
- Execute next increment with full traceability

## Requirements

### Validated

- v1.0 baseline platform completed and archived in `.planning/archive/milestones/v1.0-milestone/`

### Active

- v1.1 requirements are being defined in `.planning/REQUIREMENTS.md`

### Out of Scope

- Scope not explicitly selected into v1.1 requirements

## Constraints

- Language: Rust
- Compilation target: LLVM IR -> native binary
- Storage: Embedded backend with swappable storage boundary
- AI interface: external, model-agnostic structured API

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Archive v1.0 planning artifacts before starting v1.1 | Keep completed milestone immutable and auditable | Completed |
| Start v1.1 with requirement-first workflow | Preserve GSD sequencing (requirements -> roadmap -> execution) | In progress |

---
*Last updated: 2026-02-19 after v1.1 initialization*
