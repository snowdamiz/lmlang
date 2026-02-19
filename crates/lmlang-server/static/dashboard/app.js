(() => {
  const programId = Number(document.body.dataset.programId);

  const ENDPOINTS = {
    REGISTER_AGENT: "/agents/register",
    LIST_AGENTS: "/agents",
    DEREGISTER_AGENT: (agentId) => `/agents/${agentId}`,
    LOCKS: (pid) => `/programs/${pid}/locks`,
    LOCK_ACQUIRE: (pid) => `/programs/${pid}/locks/acquire`,
    LOCK_RELEASE: (pid) => `/programs/${pid}/locks/release`,
    MUTATIONS: (pid) => `/programs/${pid}/mutations`,
    VERIFY: (pid) => `/programs/${pid}/verify`,
    SIMULATE: (pid) => `/programs/${pid}/simulate`,
    COMPILE: (pid) => `/programs/${pid}/compile`,
    HISTORY: (pid) => `/programs/${pid}/history`,
    OBSERVE: (pid) => `/programs/${pid}/observability`,
  };

  const state = {
    activeTab: "operate",
    selectedAgentId: null,
    runSetup: {
      program: String(programId),
      template: "execute-phase",
      prompt: "",
    },
    agents: [],
    agentStatuses: {},
    timeline: [],
    lastOutput: null,
    inFlightCount: 0,
  };

  const el = {
    tabButtons: Array.from(document.querySelectorAll(".tab-btn")),
    panels: Array.from(document.querySelectorAll(".panel")),
    sharedStatusPanel: document.getElementById("sharedStatusPanel"),
    statusBadge: document.getElementById("statusBadge"),
    agentBadge: document.getElementById("agentBadge"),
    templateBadge: document.getElementById("templateBadge"),
    contextProgramId: document.getElementById("contextProgramId"),
    observeMount: document.getElementById("observeMount"),
    openObserveLink: document.getElementById("openObserveLink"),
    operateAgentListMount: document.getElementById("operateAgentListMount"),
    operateRunSetupMount: document.getElementById("operateRunSetupMount"),
    operateActionsMount: document.getElementById("operateActionsMount"),
    operateTimelineMount: document.getElementById("operateTimelineMount"),
    operateOutputMount: document.getElementById("operateOutputMount"),
  };

  function escapeHtml(value) {
    return String(value ?? "")
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;")
      .replaceAll('"', "&quot;")
      .replaceAll("'", "&#039;");
  }

  function statusToneFromError(statusCode, errorText) {
    const lower = String(errorText || "").toLowerCase();
    if (statusCode === 409 || statusCode === 423 || lower.includes("lock") || lower.includes("conflict")) {
      return "blocked";
    }
    return "error";
  }

  function getAgentStatus(agentId) {
    return state.agentStatuses[agentId] || "idle";
  }

  function setAgentStatus(agentId, status) {
    if (!agentId) {
      return;
    }
    state.agentStatuses[agentId] = status;
    renderAgentsPanel();
  }

  function setGlobalStatus(message, tone = "idle") {
    el.sharedStatusPanel.dataset.state = tone;
    el.sharedStatusPanel.textContent = message;
    el.statusBadge.textContent = `Status: ${tone}`;
  }

  function pushTimeline(entry) {
    const timestamp = new Date().toISOString();
    state.timeline.unshift({ timestamp, ...entry });
    state.timeline = state.timeline.slice(0, 20);
    renderTimelinePanel();
  }

  function setSelectedAgent(agentId) {
    state.selectedAgentId = agentId;
    el.agentBadge.textContent = agentId ? `Agent: ${agentId}` : "No agent selected";
    renderAgentsPanel();
  }

  function updateRunSetup(next) {
    state.runSetup = { ...state.runSetup, ...next };
    el.templateBadge.textContent = `Template: ${state.runSetup.template}`;
  }

  function safeJsonParse(raw, fieldName) {
    try {
      return { ok: true, value: JSON.parse(raw) };
    } catch (_err) {
      return { ok: false, error: `${fieldName} must be valid JSON` };
    }
  }

  function parseCsvU32(raw) {
    if (!raw.trim()) {
      return [];
    }
    return raw
      .split(",")
      .map((item) => Number(item.trim()))
      .filter((value) => Number.isInteger(value) && value >= 0);
  }

  function writeOutput(title, payload) {
    state.lastOutput = { title, payload };

    const requestBlock = escapeHtml(JSON.stringify(payload.request, null, 2));
    const responseBlock = escapeHtml(JSON.stringify(payload.response, null, 2));

    el.operateOutputMount.innerHTML = `
      <section class="output-block">
        <h3>${escapeHtml(title)}</h3>
        <div class="output-grid">
          <div>
            <h4>Request</h4>
            <pre>${requestBlock}</pre>
          </div>
          <div>
            <h4>Response</h4>
            <pre>${responseBlock}</pre>
          </div>
        </div>
        <p class="output-hint">Use "Open in Observe" to inspect graph/query state after actions.</p>
      </section>
    `;
  }

  async function apiRequest({ method, url, body, agentId }) {
    const headers = {
      "content-type": "application/json",
    };

    if (agentId) {
      headers["X-Agent-Id"] = agentId;
    }

    const response = await fetch(url, {
      method,
      headers,
      body: body ? JSON.stringify(body) : undefined,
    });

    const text = await response.text();
    let data;
    try {
      data = text ? JSON.parse(text) : null;
    } catch (_err) {
      data = { raw: text };
    }

    if (!response.ok) {
      const message = data?.error || data?.message || text || `request failed (${response.status})`;
      const error = new Error(message);
      error.status = response.status;
      error.payload = data;
      throw error;
    }

    return data;
  }

  async function runAction({ title, endpoint, requestBody, selectedAgentRequired = false, includeAgentHeader = false, handler }) {
    const activeAgentId = state.selectedAgentId;

    if (selectedAgentRequired && !activeAgentId) {
      setGlobalStatus("Action blocked: select an agent first.", "blocked");
      pushTimeline({ title, endpoint, status: "blocked", detail: "missing selected agent" });
      return;
    }

    state.inFlightCount += 1;
    if (activeAgentId) {
      setAgentStatus(activeAgentId, "running");
    }
    setGlobalStatus(`Running ${title}...`, "running");

    try {
      const response = await handler({
        agentId: includeAgentHeader ? activeAgentId : null,
      });

      if (activeAgentId) {
        setAgentStatus(activeAgentId, "idle");
      }
      setGlobalStatus(`${title} completed`, "idle");

      pushTimeline({
        title,
        endpoint,
        status: "ok",
        detail: includeAgentHeader && activeAgentId ? `agent ${activeAgentId}` : "",
      });

      writeOutput(title, {
        request: requestBody,
        response,
      });
    } catch (error) {
      const tone = statusToneFromError(error.status, error.message);
      if (activeAgentId) {
        setAgentStatus(activeAgentId, tone);
      }

      setGlobalStatus(`${title} failed: ${error.message}`, tone);
      pushTimeline({
        title,
        endpoint,
        status: tone,
        detail: error.message,
      });

      writeOutput(`${title} (failed)`, {
        request: requestBody,
        response: {
          status: error.status || 500,
          error: error.message,
          payload: error.payload || null,
        },
      });
    } finally {
      state.inFlightCount = Math.max(0, state.inFlightCount - 1);
      renderTimelinePanel();
    }
  }

  function renderAgentsPanel() {
    const hasAgents = state.agents.length > 0;

    const cards = hasAgents
      ? state.agents
          .map((agent) => {
            const selected = state.selectedAgentId === agent.agent_id;
            const status = getAgentStatus(agent.agent_id);
            return `
              <li class="agent-card ${selected ? "selected" : ""}">
                <button type="button" data-agent-select="${agent.agent_id}" class="agent-select-btn">
                  <span class="agent-name">${escapeHtml(agent.name || "unnamed-agent")}</span>
                  <span class="agent-id">${escapeHtml(agent.agent_id)}</span>
                  <span class="agent-status status-${status}">${status}</span>
                </button>
              </li>
            `;
          })
          .join("")
      : `<li class="empty-note">No agents registered yet.</li>`;

    el.operateAgentListMount.innerHTML = `
      <div class="control-row">
        <input id="agentNameInput" placeholder="Optional agent name" />
        <button type="button" id="registerAgentBtn">Register Agent</button>
        <button type="button" id="refreshAgentsBtn">Refresh</button>
        <button type="button" id="deregisterAgentBtn" ${state.selectedAgentId ? "" : "disabled"}>Deregister Selected</button>
      </div>
      <ul class="agent-list">${cards}</ul>
    `;

    el.operateAgentListMount.querySelectorAll("[data-agent-select]").forEach((button) => {
      button.addEventListener("click", () => {
        setSelectedAgent(button.dataset.agentSelect);
      });
    });

    el.operateAgentListMount.querySelector("#registerAgentBtn")?.addEventListener("click", onRegisterAgent);
    el.operateAgentListMount.querySelector("#refreshAgentsBtn")?.addEventListener("click", refreshAgents);
    el.operateAgentListMount.querySelector("#deregisterAgentBtn")?.addEventListener("click", onDeregisterAgent);
  }

  function renderRunSetupPanel() {
    const template = state.runSetup.template;
    const prompt = state.runSetup.prompt;

    el.operateRunSetupMount.innerHTML = `
      <div class="field-grid">
        <label class="field">
          <span>Program</span>
          <input id="setupProgramInput" value="${escapeHtml(state.runSetup.program)}" readonly />
        </label>
        <label class="field">
          <span>Workflow Template</span>
          <select id="setupTemplateSelect">
            <option value="execute-phase" ${template === "execute-phase" ? "selected" : ""}>Execute Phase</option>
            <option value="plan-phase" ${template === "plan-phase" ? "selected" : ""}>Plan Phase</option>
            <option value="verify-work" ${template === "verify-work" ? "selected" : ""}>Verify Work</option>
          </select>
        </label>
      </div>
      <label class="field">
        <span>Task Prompt</span>
        <textarea id="setupPromptInput" rows="4" placeholder="Describe the run objective">${escapeHtml(prompt)}</textarea>
      </label>
      <button type="button" id="saveSetupBtn">Save Run Setup</button>
    `;

    el.operateRunSetupMount.querySelector("#saveSetupBtn")?.addEventListener("click", () => {
      const nextTemplate = el.operateRunSetupMount.querySelector("#setupTemplateSelect")?.value || "execute-phase";
      const nextPrompt = el.operateRunSetupMount.querySelector("#setupPromptInput")?.value || "";
      updateRunSetup({ template: nextTemplate, prompt: nextPrompt });
      setGlobalStatus("Run setup saved.", "idle");
      pushTimeline({ title: "Run setup updated", endpoint: "local", status: "ok", detail: nextTemplate });
    });
  }

  function renderActionsPanel() {
    el.operateActionsMount.innerHTML = `
      <div class="action-grid">
        <section class="action-card">
          <h3>Locks</h3>
          <p>Uses <code>${ENDPOINTS.LOCK_ACQUIRE(programId)}</code> and <code>${ENDPOINTS.LOCK_RELEASE(programId)}</code>.</p>
          <label class="field"><span>Function IDs (comma-separated)</span><input id="lockFunctionIdsInput" value="1" /></label>
          <label class="field"><span>Mode</span><select id="lockModeSelect"><option value="write">write</option><option value="read">read</option></select></label>
          <label class="field"><span>Description</span><input id="lockDescriptionInput" placeholder="optional" /></label>
          <div class="control-row">
            <button type="button" id="acquireLocksBtn">Acquire</button>
            <button type="button" id="releaseLocksBtn">Release</button>
            <button type="button" id="listLocksBtn">List</button>
          </div>
        </section>

        <section class="action-card">
          <h3>Mutations</h3>
          <p>Uses <code>${ENDPOINTS.MUTATIONS(programId)}</code> with dry-run and commit support.</p>
          <label class="field"><span>Mutations JSON</span><textarea id="mutationsInput" rows="6">[{"type":"AddFunction","name":"from_dashboard","module":0,"params":[],"return_type":7,"visibility":"Public"}]</textarea></label>
          <div class="control-row">
            <button type="button" id="dryRunMutationBtn">Dry Run</button>
            <button type="button" id="commitMutationBtn">Commit</button>
          </div>
        </section>

        <section class="action-card">
          <h3>Verify / Simulate / Compile / History</h3>
          <label class="field"><span>Verify Scope</span><select id="verifyScopeSelect"><option value="local">local</option><option value="full">full</option></select></label>
          <label class="field"><span>Affected Node IDs (JSON array, optional)</span><input id="verifyAffectedInput" value="[]" /></label>
          <div class="control-row">
            <button type="button" id="verifyBtn">Verify</button>
            <button type="button" id="simulateBtn">Simulate</button>
            <button type="button" id="compileBtn">Compile</button>
            <button type="button" id="historyBtn">History</button>
          </div>
          <label class="field"><span>Simulate Function ID</span><input id="simulateFunctionIdInput" value="1" /></label>
          <label class="field"><span>Simulate Inputs (JSON array)</span><input id="simulateInputsInput" value="[]" /></label>
          <label class="field"><span>Compile Opt Level</span><select id="compileOptLevelSelect"><option>O0</option><option>O1</option><option>O2</option><option>O3</option></select></label>
        </section>
      </div>
      <div class="control-row">
        <button type="button" id="openObserveFromOperateBtn">Open in Observe</button>
      </div>
    `;

    el.operateActionsMount.querySelector("#acquireLocksBtn")?.addEventListener("click", onAcquireLocks);
    el.operateActionsMount.querySelector("#releaseLocksBtn")?.addEventListener("click", onReleaseLocks);
    el.operateActionsMount.querySelector("#listLocksBtn")?.addEventListener("click", onListLocks);
    el.operateActionsMount.querySelector("#dryRunMutationBtn")?.addEventListener("click", () => onMutations(true));
    el.operateActionsMount.querySelector("#commitMutationBtn")?.addEventListener("click", () => onMutations(false));
    el.operateActionsMount.querySelector("#verifyBtn")?.addEventListener("click", onVerify);
    el.operateActionsMount.querySelector("#simulateBtn")?.addEventListener("click", onSimulate);
    el.operateActionsMount.querySelector("#compileBtn")?.addEventListener("click", onCompile);
    el.operateActionsMount.querySelector("#historyBtn")?.addEventListener("click", onHistory);

    el.operateActionsMount.querySelector("#openObserveFromOperateBtn")?.addEventListener("click", () => {
      activateTab("observe");
    });
  }

  function renderTimelinePanel() {
    if (!state.timeline.length) {
      el.operateTimelineMount.innerHTML = `<p class="empty-note">No operations yet.</p>`;
      return;
    }

    const rows = state.timeline
      .map((entry) => {
        return `
          <li class="timeline-row">
            <span class="timeline-status status-${escapeHtml(entry.status)}">${escapeHtml(entry.status)}</span>
            <div class="timeline-body">
              <p class="timeline-title">${escapeHtml(entry.title)}</p>
              <p class="timeline-meta">${escapeHtml(entry.endpoint)} • ${escapeHtml(entry.timestamp)}</p>
              <p class="timeline-detail">${escapeHtml(entry.detail || "")}</p>
            </div>
          </li>
        `;
      })
      .join("");

    el.operateTimelineMount.innerHTML = `<ul class="timeline-list">${rows}</ul>`;
  }

  function ensureObserveEmbedded() {
    if (el.observeMount.querySelector("iframe")) {
      return;
    }

    const iframe = document.createElement("iframe");
    iframe.title = "Observe panel";
    iframe.loading = "lazy";
    iframe.src = ENDPOINTS.OBSERVE(programId);
    iframe.className = "observe-frame";

    iframe.addEventListener("load", () => {
      setGlobalStatus("Observe module ready.", "idle");
    });

    iframe.addEventListener("error", () => {
      setGlobalStatus("Observe failed to load. Open it in a new tab.", "error");
    });

    el.observeMount.appendChild(iframe);
  }

  function activateTab(tab) {
    state.activeTab = tab;

    el.tabButtons.forEach((btn) => {
      btn.classList.toggle("is-active", btn.dataset.tab === tab);
    });

    el.panels.forEach((panel) => {
      panel.classList.toggle("is-active", panel.dataset.panel === tab);
    });

    if (tab === "observe") {
      ensureObserveEmbedded();
      setGlobalStatus("Observe tab active. Existing observability route reused.", "idle");
      return;
    }

    setGlobalStatus("Operate tab active.", "idle");
  }

  async function refreshAgents() {
    await runAction({
      title: "List agents",
      endpoint: ENDPOINTS.LIST_AGENTS,
      requestBody: null,
      handler: async () => {
        const response = await apiRequest({
          method: "GET",
          url: ENDPOINTS.LIST_AGENTS,
        });

        state.agents = response.agents || [];
        state.agents.forEach((agent) => {
          if (!state.agentStatuses[agent.agent_id]) {
            state.agentStatuses[agent.agent_id] = "idle";
          }
        });

        if (state.selectedAgentId && !state.agents.some((agent) => agent.agent_id === state.selectedAgentId)) {
          setSelectedAgent(null);
        }

        renderAgentsPanel();
        return response;
      },
    });
  }

  async function onRegisterAgent() {
    const nameInput = el.operateAgentListMount.querySelector("#agentNameInput");
    const name = nameInput?.value?.trim() || null;

    await runAction({
      title: "Register agent",
      endpoint: ENDPOINTS.REGISTER_AGENT,
      requestBody: { name },
      handler: async () => {
        const response = await apiRequest({
          method: "POST",
          url: ENDPOINTS.REGISTER_AGENT,
          body: { name },
        });

        if (response?.agent_id) {
          state.agentStatuses[response.agent_id] = "idle";
          setSelectedAgent(response.agent_id);
        }

        await refreshAgents();
        return response;
      },
    });
  }

  async function onDeregisterAgent() {
    const agentId = state.selectedAgentId;
    if (!agentId) {
      setGlobalStatus("Select an agent before deregistering.", "blocked");
      return;
    }

    await runAction({
      title: "Deregister agent",
      endpoint: ENDPOINTS.DEREGISTER_AGENT(agentId),
      requestBody: null,
      selectedAgentRequired: true,
      handler: async () => {
        const response = await apiRequest({
          method: "DELETE",
          url: ENDPOINTS.DEREGISTER_AGENT(agentId),
        });

        delete state.agentStatuses[agentId];
        setSelectedAgent(null);
        await refreshAgents();
        return response;
      },
    });
  }

  async function onAcquireLocks() {
    const functionIdsRaw = el.operateActionsMount.querySelector("#lockFunctionIdsInput")?.value || "";
    const mode = el.operateActionsMount.querySelector("#lockModeSelect")?.value || "write";
    const description = el.operateActionsMount.querySelector("#lockDescriptionInput")?.value?.trim() || null;
    const function_ids = parseCsvU32(functionIdsRaw);

    await runAction({
      title: "Acquire locks",
      endpoint: ENDPOINTS.LOCK_ACQUIRE(programId),
      requestBody: { function_ids, mode, description },
      selectedAgentRequired: true,
      includeAgentHeader: true,
      handler: async ({ agentId }) => {
        return apiRequest({
          method: "POST",
          url: ENDPOINTS.LOCK_ACQUIRE(programId),
          body: { function_ids, mode, description },
          agentId,
        });
      },
    });
  }

  async function onReleaseLocks() {
    const functionIdsRaw = el.operateActionsMount.querySelector("#lockFunctionIdsInput")?.value || "";
    const function_ids = parseCsvU32(functionIdsRaw);

    await runAction({
      title: "Release locks",
      endpoint: ENDPOINTS.LOCK_RELEASE(programId),
      requestBody: { function_ids },
      selectedAgentRequired: true,
      includeAgentHeader: true,
      handler: async ({ agentId }) => {
        return apiRequest({
          method: "POST",
          url: ENDPOINTS.LOCK_RELEASE(programId),
          body: { function_ids },
          agentId,
        });
      },
    });
  }

  async function onListLocks() {
    await runAction({
      title: "List locks",
      endpoint: ENDPOINTS.LOCKS(programId),
      requestBody: null,
      handler: async () => {
        return apiRequest({
          method: "GET",
          url: ENDPOINTS.LOCKS(programId),
        });
      },
    });
  }

  async function onMutations(dryRun) {
    const raw = el.operateActionsMount.querySelector("#mutationsInput")?.value || "[]";
    const parsed = safeJsonParse(raw, "mutations");
    if (!parsed.ok || !Array.isArray(parsed.value)) {
      setGlobalStatus(parsed.error || "mutations must be a JSON array", "error");
      return;
    }

    const body = {
      mutations: parsed.value,
      dry_run: dryRun,
    };

    await runAction({
      title: dryRun ? "Mutation dry-run" : "Mutation commit",
      endpoint: ENDPOINTS.MUTATIONS(programId),
      requestBody: body,
      includeAgentHeader: Boolean(state.selectedAgentId),
      handler: async ({ agentId }) => {
        return apiRequest({
          method: "POST",
          url: ENDPOINTS.MUTATIONS(programId),
          body,
          agentId,
        });
      },
    });
  }

  async function onVerify() {
    const scope = el.operateActionsMount.querySelector("#verifyScopeSelect")?.value || "local";
    const raw = el.operateActionsMount.querySelector("#verifyAffectedInput")?.value || "[]";
    const parsed = safeJsonParse(raw, "affected_nodes");
    if (!parsed.ok || !Array.isArray(parsed.value)) {
      setGlobalStatus(parsed.error || "affected_nodes must be a JSON array", "error");
      return;
    }

    const affectedNodes = parsed.value.filter((value) => Number.isInteger(value));
    const body = {
      scope,
      affected_nodes: affectedNodes,
    };

    await runAction({
      title: "Verify",
      endpoint: ENDPOINTS.VERIFY(programId),
      requestBody: body,
      handler: async () => {
        return apiRequest({
          method: "POST",
          url: ENDPOINTS.VERIFY(programId),
          body,
        });
      },
    });
  }

  async function onSimulate() {
    const functionId = Number(el.operateActionsMount.querySelector("#simulateFunctionIdInput")?.value || "1");
    const raw = el.operateActionsMount.querySelector("#simulateInputsInput")?.value || "[]";
    const parsed = safeJsonParse(raw, "inputs");
    if (!parsed.ok || !Array.isArray(parsed.value)) {
      setGlobalStatus(parsed.error || "inputs must be a JSON array", "error");
      return;
    }

    const body = {
      function_id: functionId,
      inputs: parsed.value,
      trace_enabled: true,
    };

    await runAction({
      title: "Simulate",
      endpoint: ENDPOINTS.SIMULATE(programId),
      requestBody: body,
      handler: async () => {
        return apiRequest({
          method: "POST",
          url: ENDPOINTS.SIMULATE(programId),
          body,
        });
      },
    });
  }

  async function onCompile() {
    const optLevel = el.operateActionsMount.querySelector("#compileOptLevelSelect")?.value || "O0";
    const body = {
      opt_level: optLevel,
      debug_symbols: false,
    };

    await runAction({
      title: "Compile",
      endpoint: ENDPOINTS.COMPILE(programId),
      requestBody: body,
      handler: async () => {
        return apiRequest({
          method: "POST",
          url: ENDPOINTS.COMPILE(programId),
          body,
        });
      },
    });
  }

  async function onHistory() {
    await runAction({
      title: "History",
      endpoint: ENDPOINTS.HISTORY(programId),
      requestBody: null,
      handler: async () => {
        return apiRequest({
          method: "GET",
          url: ENDPOINTS.HISTORY(programId),
        });
      },
    });
  }

  function bindTabNavigation() {
    el.tabButtons.forEach((btn) => {
      btn.addEventListener("click", () => activateTab(btn.dataset.tab));
    });
  }

  function bootstrapOutputPanel() {
    writeOutput("Dashboard bootstrap", {
      request: {
        program_id: programId,
        template: state.runSetup.template,
      },
      response: {
        observe_path: ENDPOINTS.OBSERVE(programId),
        endpoint_hooks: {
          agents: [ENDPOINTS.REGISTER_AGENT, ENDPOINTS.LIST_AGENTS, ENDPOINTS.DEREGISTER_AGENT("{agent_id}")],
          locks: [ENDPOINTS.LOCKS(programId), ENDPOINTS.LOCK_ACQUIRE(programId), ENDPOINTS.LOCK_RELEASE(programId)],
          mutations: [ENDPOINTS.MUTATIONS(programId)],
          verify: [ENDPOINTS.VERIFY(programId)],
          simulate: [ENDPOINTS.SIMULATE(programId)],
          compile: [ENDPOINTS.COMPILE(programId)],
          history: [ENDPOINTS.HISTORY(programId)],
        },
      },
    });
  }

  async function init() {
    bindTabNavigation();

    el.contextProgramId.textContent = String(programId);
    el.openObserveLink.href = ENDPOINTS.OBSERVE(programId);

    renderRunSetupPanel();
    renderActionsPanel();
    renderTimelinePanel();
    renderAgentsPanel();
    bootstrapOutputPanel();

    activateTab("operate");
    setGlobalStatus("Dashboard initialized. Loading agents...", "running");
    await refreshAgents();
    setGlobalStatus("Dashboard ready.", "idle");
  }

  init();
})();
