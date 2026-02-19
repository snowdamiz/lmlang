# Phase 2: Storage & Persistence - Context

**Gathered:** 2026-02-18
**Status:** Ready for planning

<domain>
## Phase Boundary

Programs persist across process restarts in SQLite with a swappable backend and content-addressable identity. A multi-program database stores graphs in normalized relational tables. Restore, versioning, and undo are separate phases.

</domain>

<decisions>
## Implementation Decisions

### Storage model
- Normalized relational schema: separate tables for nodes, edges, types, modules, functions
- Types in their own table with IDs; nodes and edges reference type IDs (enables type-based queries and deduplication)
- Multi-program database: single SQLite file holds multiple programs with a programs table
- Modules and functions as separate tables with parent-child hierarchy (enables querying by module path, listing functions in a module)

### Content hashing
- Merkle-tree style: node hashes compose upward including edge targets' hashes (structural integrity like git)
- Per-function root hashes: each function has a root hash; cross-function changes detected by comparing function roots
- Eager recomputation: hashes updated immediately on every mutation (always consistent, cost at write time)
- Hashes NOT stored in database: computed from stored content on load (DB is source of truth, hashes are derived)

### Save/load granularity
- Whole-program loading: load all nodes, edges, types, functions, modules in one operation
- Incremental saves: track dirty nodes/edges since last save, write only changed data
- Auto-persist: every mutation writes through to SQLite immediately (always durable)

### Trait API surface
- Two-layer API: low-level CRUD (insert_node, get_node, delete_node, insert_edge, etc.) as the trait foundation, with high-level convenience methods (save_program, load_program, save_function) built on top
- In-memory backend is first-class: production-quality, used for tests, ephemeral agent sessions, and anywhere persistence isn't needed
- Query methods included in trait: find_by_type, find_functions, search edges — backend-specific optimization (SQL WHERE vs in-memory filter)

### Claude's Discretion
- Sync vs async trait design (based on Rust async ergonomics and project backend evolution)
- SQLite transaction batching strategy for grouped mutations
- Exact SQL schema design and migration approach
- Serialization format for complex types (e.g., ConstValue variants)
- Error type design for storage operations

</decisions>

<specifics>
## Specific Ideas

- Merkle-tree hashing mirrors git's content-addressable model — structural integrity verification by comparing root hashes
- In-memory backend should be usable in the same way as SQLite (not a second-class citizen)
- Multi-program DB enables future cross-program operations without requiring separate files

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 02-storage-persistence*
*Context gathered: 2026-02-18*
