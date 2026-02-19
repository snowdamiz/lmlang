# Phase 9: Human Observability - Context

**Gathered:** 2026-02-19
**Status:** Ready for planning

## Phase Boundary

This phase delivers human-facing observability over the program graph: an interactive DAG view with clear layer differentiation (semantic vs computational), plus natural-language querying that returns relevant subgraphs with contextual explanation.

## Implementation Decisions

### Graph View Design
- Use a cleaner DAG by default with minimal labels; reveal details on demand.
- Node interaction uses click-to-open right-side details panel.

### Layer Differentiation
- Two layers should be shown in separated regions on the same canvas.
- Cross-layer edges should be visually distinct (dashed/annotated).
- Provide layer visibility toggles and preset filters (for example: semantic-only, computational-only, interop edges).

### NL Query Behavior
- Query UI should combine free-text input with suggested prompt chips.
- For ambiguous queries, ask a clarifying follow-up and support surfacing top interpretations when useful.
- If no strong match exists, return nearest related nodes instead of a hard empty result.

### Result Context Format
- Use tabbed context sections: Summary, Relationships, Contracts.
- Contracts should show high-level description plus key clauses.
- Relationships should include a mini local graph plus text list.
- Graph and context should be bidirectionally synced.

### Claude's Discretion
- Exact function boundary rendering in graph view.
- Default viewport behavior when opening the graph.
- Final result breadth strategy for query responses (single, ranked, or hybrid) as long as it aligns with ambiguity handling decisions.
- Specific visual encoding for layer differentiation (exact color/shape system).

## Specific Ideas

- Keep the visual graph clean by default while supporting deeper inspection through the side panel.
- Ambiguity handling should favor disambiguation over silent guessing, with optional interpretation visibility.
- Context experience should stay tightly linked to graph exploration via bidirectional highlighting/selection.

## Deferred Ideas

None - discussion stayed within phase scope.

---

*Phase: 09-human-observability*
*Context gathered: 2026-02-19*
