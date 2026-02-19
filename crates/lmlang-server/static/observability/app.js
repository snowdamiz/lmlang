(() => {
  const programId = document.body.dataset.programId;

  const state = {
    preset: "all",
    graph: null,
    filteredNodes: [],
    filteredEdges: [],
    layoutByNodeId: new Map(),
    selectedNodeId: null,
    queryResponse: null,
    activeTab: "summary",
    viewport: {
      x: 0,
      y: 0,
      scale: 1,
    },
    drag: {
      active: false,
      startX: 0,
      startY: 0,
      originX: 0,
      originY: 0,
    },
  };

  const el = {
    graphCanvas: document.getElementById("graphCanvas"),
    viewport: document.getElementById("viewport"),
    edgeLayer: document.getElementById("edgeLayer"),
    groupLayer: document.getElementById("groupLayer"),
    nodeLayer: document.getElementById("nodeLayer"),
    nodeCountBadge: document.getElementById("nodeCountBadge"),
    edgeCountBadge: document.getElementById("edgeCountBadge"),
    selectionBadge: document.getElementById("selectionBadge"),
    detailsContent: document.getElementById("detailsContent"),
    tabBody: document.getElementById("tabBody"),
    resultList: document.getElementById("resultList"),
    queryForm: document.getElementById("queryForm"),
    queryInput: document.getElementById("queryInput"),
    queryStatus: document.getElementById("queryStatus"),
    chipRow: document.getElementById("chipRow"),
    interpretationList: document.getElementById("interpretationList"),
    ambiguityPrompt: document.getElementById("ambiguityPrompt"),
    toggleSemantic: document.getElementById("toggleSemantic"),
    toggleCompute: document.getElementById("toggleCompute"),
    toggleCross: document.getElementById("toggleCross"),
    fitViewBtn: document.getElementById("fitViewBtn"),
    refreshBtn: document.getElementById("refreshBtn"),
    presetButtons: Array.from(document.querySelectorAll(".preset-btn")),
    tabButtons: Array.from(document.querySelectorAll(".tab-btn")),
  };

  const NS = "http://www.w3.org/2000/svg";

  function createSvg(name, attrs = {}) {
    const node = document.createElementNS(NS, name);
    Object.entries(attrs).forEach(([key, value]) => {
      node.setAttribute(key, String(value));
    });
    return node;
  }

  function sanitize(text) {
    return String(text ?? "").replace(/[<>]/g, "");
  }

  function applyViewportTransform() {
    el.viewport.setAttribute(
      "transform",
      `translate(${state.viewport.x} ${state.viewport.y}) scale(${state.viewport.scale})`
    );
  }

  function resetViewport() {
    state.viewport.x = 0;
    state.viewport.y = 0;
    state.viewport.scale = 1;
    applyViewportTransform();
  }

  function bindPanAndZoom() {
    el.graphCanvas.addEventListener("wheel", (event) => {
      event.preventDefault();
      const direction = event.deltaY < 0 ? 1 : -1;
      const factor = direction > 0 ? 1.12 : 0.9;
      const nextScale = Math.min(2.5, Math.max(0.35, state.viewport.scale * factor));
      state.viewport.scale = nextScale;
      applyViewportTransform();
    });

    el.graphCanvas.addEventListener("pointerdown", (event) => {
      if (event.target.closest(".graph-node")) {
        return;
      }
      state.drag.active = true;
      state.drag.startX = event.clientX;
      state.drag.startY = event.clientY;
      state.drag.originX = state.viewport.x;
      state.drag.originY = state.viewport.y;
      el.graphCanvas.setPointerCapture(event.pointerId);
    });

    el.graphCanvas.addEventListener("pointermove", (event) => {
      if (!state.drag.active) {
        return;
      }
      const dx = event.clientX - state.drag.startX;
      const dy = event.clientY - state.drag.startY;
      state.viewport.x = state.drag.originX + dx;
      state.viewport.y = state.drag.originY + dy;
      applyViewportTransform();
    });

    el.graphCanvas.addEventListener("pointerup", (event) => {
      state.drag.active = false;
      try {
        el.graphCanvas.releasePointerCapture(event.pointerId);
      } catch (_err) {
        // Ignore capture errors when pointer already released.
      }
    });
  }

  function setPreset(preset) {
    state.preset = preset;
    el.presetButtons.forEach((btn) => {
      btn.classList.toggle("active", btn.dataset.preset === preset);
    });
    loadGraph();
  }

  function selectedLayers() {
    return {
      semantic: el.toggleSemantic.checked,
      compute: el.toggleCompute.checked,
      cross: el.toggleCross.checked,
    };
  }

  function updateGraphBadges() {
    el.nodeCountBadge.textContent = `${state.filteredNodes.length} nodes`;
    el.edgeCountBadge.textContent = `${state.filteredEdges.length} edges`;
  }

  function filterGraphByLayer() {
    if (!state.graph) {
      return;
    }
    const toggles = selectedLayers();

    const allowedNodes = state.graph.nodes.filter((node) => {
      if (node.layer === "semantic" && !toggles.semantic) {
        return false;
      }
      if (node.layer === "compute" && !toggles.compute) {
        return false;
      }
      return true;
    });
    const allowedSet = new Set(allowedNodes.map((node) => node.id));

    const allowedEdges = state.graph.edges.filter((edge) => {
      if (!allowedSet.has(edge.from) || !allowedSet.has(edge.to)) {
        return false;
      }
      if (edge.cross_layer && !toggles.cross) {
        return false;
      }
      if (!edge.cross_layer && edge.from_layer === "semantic" && !toggles.semantic) {
        return false;
      }
      if (!edge.cross_layer && edge.from_layer === "compute" && !toggles.compute) {
        return false;
      }
      return true;
    });

    state.filteredNodes = allowedNodes;
    state.filteredEdges = allowedEdges;
  }

  function graphUrl() {
    const params = new URLSearchParams({
      preset: state.preset,
      include_cross_layer: "true",
    });
    return `/programs/${programId}/observability/graph?${params.toString()}`;
  }

  async function loadGraph() {
    try {
      el.queryStatus.textContent = "Loading graph...";
      const response = await fetch(graphUrl());
      if (!response.ok) {
        throw new Error(`graph request failed (${response.status})`);
      }
      state.graph = await response.json();
      filterGraphByLayer();
      computeLayout();
      renderGraph();
      updateGraphBadges();
      renderDetailsForSelection();
      renderResultList();
      el.queryStatus.textContent = "Graph updated.";
    } catch (error) {
      el.queryStatus.textContent = `Graph load failed: ${error.message}`;
    }
  }

  function computeLayout() {
    state.layoutByNodeId.clear();
    if (!state.graph) {
      return;
    }

    const groupMeta = new Map();
    state.graph.groups.forEach((group, index) => {
      const yTop = 80 + index * 190;
      groupMeta.set(group.id, {
        x: 730,
        y: yTop,
        width: 620,
        height: 160,
        centerY: yTop + 80,
      });

      const cols = 3;
      group.compute_node_ids.forEach((nodeId, nodeIndex) => {
        const col = nodeIndex % cols;
        const row = Math.floor(nodeIndex / cols);
        const x = 760 + col * 190;
        const y = yTop + 42 + row * 42;
        state.layoutByNodeId.set(nodeId, { x, y });
      });
    });

    const semanticNodes = state.filteredNodes.filter((node) => node.layer === "semantic");
    let semanticCursor = 90;
    semanticNodes.forEach((node) => {
      if (node.group_id && groupMeta.has(node.group_id)) {
        const group = groupMeta.get(node.group_id);
        state.layoutByNodeId.set(node.id, { x: 260, y: group.centerY });
      } else {
        state.layoutByNodeId.set(node.id, { x: 260, y: semanticCursor });
        semanticCursor += 72;
      }
    });

    state.filteredNodes.forEach((node) => {
      if (!state.layoutByNodeId.has(node.id)) {
        state.layoutByNodeId.set(node.id, { x: 260, y: semanticCursor });
        semanticCursor += 60;
      }
    });
  }

  function nodeById(nodeId) {
    return state.graph?.nodes.find((node) => node.id === nodeId) ?? null;
  }

  function drawGroups() {
    el.groupLayer.textContent = "";
    if (!state.graph) {
      return;
    }

    state.graph.groups.forEach((group, index) => {
      const y = 80 + index * 190;
      const box = createSvg("rect", {
        x: 730,
        y,
        width: 620,
        height: 160,
        rx: 14,
        class: "function-boundary",
      });
      el.groupLayer.appendChild(box);

      const label = createSvg("text", {
        x: 748,
        y: y + 24,
        class: "group-label",
      });
      label.textContent = `${sanitize(group.function_name)} (fn ${group.function_id})`;
      el.groupLayer.appendChild(label);
    });
  }

  function edgeClass(edge) {
    if (edge.cross_layer) {
      return "graph-edge edge-cross";
    }
    if (edge.edge_kind === "data") {
      return "graph-edge edge-data";
    }
    if (edge.edge_kind === "control") {
      return "graph-edge edge-control";
    }
    return "graph-edge edge-semantic";
  }

  function drawEdges() {
    el.edgeLayer.textContent = "";
    state.filteredEdges.forEach((edge) => {
      const from = state.layoutByNodeId.get(edge.from);
      const to = state.layoutByNodeId.get(edge.to);
      if (!from || !to) {
        return;
      }
      const line = createSvg("line", {
        x1: from.x,
        y1: from.y,
        x2: to.x,
        y2: to.y,
        class: edgeClass(edge),
        "data-edge-id": edge.id,
        "marker-end": "url(#arrowhead)",
      });
      if (state.selectedNodeId && (edge.from === state.selectedNodeId || edge.to === state.selectedNodeId)) {
        line.classList.add("active");
      }
      el.edgeLayer.appendChild(line);
    });
  }

  function drawNodes() {
    el.nodeLayer.textContent = "";

    state.filteredNodes.forEach((node) => {
      const point = state.layoutByNodeId.get(node.id);
      if (!point) {
        return;
      }

      const group = createSvg("g", {
        class: `graph-node layer-${node.layer}`,
        transform: `translate(${point.x} ${point.y})`,
        "data-node-id": node.id,
      });

      if (node.layer === "semantic") {
        const circle = createSvg("circle", {
          r: 20,
          class: "node-shape semantic-shape",
        });
        group.appendChild(circle);
      } else {
        const rect = createSvg("rect", {
          x: -32,
          y: -16,
          width: 64,
          height: 32,
          rx: 8,
          class: "node-shape compute-shape",
        });
        group.appendChild(rect);
      }

      const text = createSvg("text", {
        x: 0,
        y: node.layer === "semantic" ? 36 : 30,
        class: "node-label",
      });
      text.textContent = sanitize(node.short_label || node.label);
      group.appendChild(text);

      if (node.id === state.selectedNodeId) {
        group.classList.add("selected");
      }

      group.addEventListener("click", () => {
        selectNode(node.id, "graph");
      });

      el.nodeLayer.appendChild(group);
    });
  }

  function renderGraph() {
    if (!state.graph) {
      return;
    }
    drawGroups();
    drawEdges();
    drawNodes();
  }

  function formatDetailList(node) {
    const parts = [];
    parts.push(`<div class="detail-row"><strong>Node:</strong> ${sanitize(node.id)}</div>`);
    parts.push(`<div class="detail-row"><strong>Layer:</strong> ${sanitize(node.layer)}</div>`);
    parts.push(`<div class="detail-row"><strong>Kind:</strong> ${sanitize(node.kind)}</div>`);
    if (node.function_name) {
      parts.push(`<div class="detail-row"><strong>Function:</strong> ${sanitize(node.function_name)}</div>`);
    }
    if (node.summary) {
      parts.push(`<div class="detail-row"><strong>Summary:</strong> ${sanitize(node.summary)}</div>`);
    }
    return parts.join("");
  }

  function renderDetailsForSelection() {
    const node = state.selectedNodeId ? nodeById(state.selectedNodeId) : null;
    if (!node) {
      el.detailsContent.classList.add("empty");
      el.detailsContent.textContent =
        "Select a node to inspect summary, relationships, and contracts.";
      el.selectionBadge.textContent = "No selection";
      renderTabBody(null);
      return;
    }

    el.detailsContent.classList.remove("empty");
    el.detailsContent.innerHTML = formatDetailList(node);
    el.selectionBadge.textContent = node.id;

    const selectedResult =
      state.queryResponse?.results?.find((result) => result.node_id === node.id) ?? null;
    renderTabBody(selectedResult);
  }

  function renderSummaryTab(result) {
    if (!result) {
      return "<p class='muted'>Run a query or select a query result to populate context tabs.</p>";
    }
    const summary = result.summary;
    return `
      <article>
        <h4>${sanitize(summary.title)}</h4>
        <p>${sanitize(summary.body)}</p>
        <p class="muted">Function: ${sanitize(summary.function_name || "n/a")}</p>
      </article>
    `;
  }

  function renderRelationshipsTab(result) {
    if (!result) {
      return "<p class='muted'>No relationship context selected.</p>";
    }
    const items = result.relationships.items
      .map((item) => {
        return `<li><strong>${sanitize(item.direction)}</strong> ${sanitize(item.edge_kind)} -> ${sanitize(item.label)}</li>`;
      })
      .join("");
    return `
      <article>
        <p class="muted">Mini-graph nodes: ${result.relationships.mini_graph_node_ids.length}</p>
        <ul>${items || "<li>No relationships</li>"}</ul>
      </article>
    `;
  }

  function renderContractsTab(result) {
    if (!result) {
      return "<p class='muted'>No contract context selected.</p>";
    }
    if (!result.contracts.has_contracts) {
      return "<p class='muted'>No contracts attached to this result.</p>";
    }
    const rows = result.contracts.entries
      .map((entry) => {
        return `
          <li>
            <strong>${sanitize(entry.contract_kind)}</strong>
            <span>${sanitize(entry.message)}</span>
          </li>
        `;
      })
      .join("");
    return `<ul class="contract-list">${rows}</ul>`;
  }

  function renderTabBody(result) {
    if (state.activeTab === "summary") {
      el.tabBody.innerHTML = renderSummaryTab(result);
      return;
    }
    if (state.activeTab === "relationships") {
      el.tabBody.innerHTML = renderRelationshipsTab(result);
      return;
    }
    el.tabBody.innerHTML = renderContractsTab(result);
  }

  function selectNode(nodeId, source) {
    state.selectedNodeId = nodeId;
    renderGraph();
    renderDetailsForSelection();

    if (source === "result") {
      focusNode(nodeId);
    }
    syncResultSelection();
  }

  function syncResultSelection() {
    const cards = Array.from(el.resultList.querySelectorAll(".result-card"));
    cards.forEach((card) => {
      card.classList.toggle("active", card.dataset.nodeId === state.selectedNodeId);
    });
  }

  function focusNode(nodeId) {
    const point = state.layoutByNodeId.get(nodeId);
    if (!point) {
      return;
    }
    const viewBox = el.graphCanvas.viewBox.baseVal;
    const targetX = viewBox.width * 0.5 - point.x * state.viewport.scale;
    const targetY = viewBox.height * 0.45 - point.y * state.viewport.scale;
    state.viewport.x = targetX;
    state.viewport.y = targetY;
    applyViewportTransform();
  }

  function fitView() {
    if (state.filteredNodes.length === 0) {
      resetViewport();
      return;
    }

    let minX = Number.POSITIVE_INFINITY;
    let maxX = Number.NEGATIVE_INFINITY;
    let minY = Number.POSITIVE_INFINITY;
    let maxY = Number.NEGATIVE_INFINITY;
    state.filteredNodes.forEach((node) => {
      const point = state.layoutByNodeId.get(node.id);
      if (!point) {
        return;
      }
      minX = Math.min(minX, point.x);
      maxX = Math.max(maxX, point.x);
      minY = Math.min(minY, point.y);
      maxY = Math.max(maxY, point.y);
    });

    const width = Math.max(200, maxX - minX + 200);
    const height = Math.max(200, maxY - minY + 200);
    const viewBox = el.graphCanvas.viewBox.baseVal;
    const scale = Math.min(viewBox.width / width, viewBox.height / height, 1.2);

    state.viewport.scale = Math.max(0.4, Math.min(1.5, scale));
    state.viewport.x = viewBox.width * 0.5 - ((minX + maxX) * 0.5) * state.viewport.scale;
    state.viewport.y = viewBox.height * 0.5 - ((minY + maxY) * 0.5) * state.viewport.scale;
    applyViewportTransform();
  }

  function setQueryStatus(message) {
    el.queryStatus.textContent = message;
  }

  function renderPromptChips(chips) {
    const fallback = [
      { id: "f1", label: "Program Overview", query: "show the high level structure" },
      { id: "f2", label: "Contracts", query: "which functions enforce contracts" },
      { id: "f3", label: "Data Flow", query: "trace data flow in main" },
    ];
    const source = chips && chips.length > 0 ? chips : fallback;

    el.chipRow.textContent = "";
    source.forEach((chip) => {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "chip";
      btn.textContent = chip.label;
      btn.addEventListener("click", () => {
        el.queryInput.value = chip.query;
        runQuery(chip.query, null);
      });
      el.chipRow.appendChild(btn);
    });
  }

  function renderInterpretations(response) {
    el.interpretationList.textContent = "";
    if (!response || !response.ambiguous) {
      el.ambiguityPrompt.textContent = "No ambiguity pending.";
      return;
    }

    el.ambiguityPrompt.textContent = response.ambiguity_prompt || "Select an interpretation.";
    response.interpretations.forEach((candidate) => {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "interpretation-btn";
      btn.textContent = `${candidate.label} (${candidate.score.toFixed(2)})`;
      btn.addEventListener("click", () => {
        runQuery(response.query, candidate.candidate_id);
      });
      el.interpretationList.appendChild(btn);
    });
  }

  function renderResultList() {
    el.resultList.textContent = "";
    const response = state.queryResponse;
    if (!response || !response.results || response.results.length === 0) {
      const empty = document.createElement("p");
      empty.className = "muted";
      empty.textContent = "Query results appear here.";
      el.resultList.appendChild(empty);
      return;
    }

    response.results.forEach((result) => {
      const card = document.createElement("button");
      card.type = "button";
      card.className = "result-card";
      card.dataset.nodeId = result.node_id;
      card.innerHTML = `
        <span class="result-rank">#${result.rank}</span>
        <span class="result-label">${sanitize(result.label)}</span>
        <span class="result-score">score ${result.score.toFixed(2)}</span>
      `;
      card.addEventListener("click", () => {
        selectNode(result.node_id, "result");
      });
      el.resultList.appendChild(card);
    });

    syncResultSelection();
  }

  async function runQuery(query, selectedCandidateId) {
    const trimmed = query.trim();
    if (!trimmed) {
      setQueryStatus("Enter a question before querying.");
      return;
    }

    const payload = {
      query: trimmed,
      max_results: 5,
    };
    if (selectedCandidateId) {
      payload.selected_candidate_id = selectedCandidateId;
    }

    try {
      setQueryStatus("Running semantic query...");
      const response = await fetch(`/programs/${programId}/observability/query`, {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify(payload),
      });
      if (!response.ok) {
        throw new Error(`query failed (${response.status})`);
      }

      state.queryResponse = await response.json();
      renderPromptChips(state.queryResponse.suggested_prompts || []);
      renderInterpretations(state.queryResponse);
      renderResultList();

      const confidence = state.queryResponse.confidence ?? 0;
      const low = state.queryResponse.low_confidence ? " low confidence" : "";
      const fallback = state.queryResponse.fallback_reason
        ? ` | ${state.queryResponse.fallback_reason}`
        : "";
      setQueryStatus(`Result confidence ${confidence.toFixed(2)}${low}${fallback}`);

      if (state.queryResponse.selected_graph_node_id) {
        selectNode(state.queryResponse.selected_graph_node_id, "result");
      }
    } catch (error) {
      setQueryStatus(`Query failed: ${error.message}`);
    }
  }

  function bindEvents() {
    el.toggleSemantic.addEventListener("change", () => {
      filterGraphByLayer();
      computeLayout();
      renderGraph();
      updateGraphBadges();
      renderDetailsForSelection();
    });
    el.toggleCompute.addEventListener("change", () => {
      filterGraphByLayer();
      computeLayout();
      renderGraph();
      updateGraphBadges();
      renderDetailsForSelection();
    });
    el.toggleCross.addEventListener("change", () => {
      filterGraphByLayer();
      renderGraph();
      updateGraphBadges();
    });

    el.presetButtons.forEach((btn) => {
      btn.addEventListener("click", () => {
        setPreset(btn.dataset.preset);
      });
    });

    el.tabButtons.forEach((btn) => {
      btn.addEventListener("click", () => {
        state.activeTab = btn.dataset.tab;
        el.tabButtons.forEach((other) => {
          other.classList.toggle("active", other.dataset.tab === state.activeTab);
        });
        renderDetailsForSelection();
      });
    });

    el.fitViewBtn.addEventListener("click", () => {
      fitView();
    });

    el.refreshBtn.addEventListener("click", () => {
      loadGraph();
    });

    el.queryForm.addEventListener("submit", (event) => {
      event.preventDefault();
      runQuery(el.queryInput.value, null);
    });
  }

  function initialize() {
    bindEvents();
    bindPanAndZoom();
    renderPromptChips([]);
    resetViewport();
    loadGraph();
  }

  initialize();
})();
