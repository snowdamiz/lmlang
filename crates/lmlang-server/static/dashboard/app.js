(() => {
  const programId = document.body.dataset.programId;

  const state = {
    activeTab: "operate",
    selectedAgentId: null,
    status: "idle",
    lastOperation: null,
  };

  const el = {
    tabButtons: Array.from(document.querySelectorAll(".tab-btn")),
    panels: Array.from(document.querySelectorAll(".panel")),
    sharedStatusPanel: document.getElementById("sharedStatusPanel"),
    statusBadge: document.getElementById("statusBadge"),
    agentBadge: document.getElementById("agentBadge"),
    observeMount: document.getElementById("observeMount"),
    openObserveLink: document.getElementById("openObserveLink"),
    operateAgentListMount: document.getElementById("operateAgentListMount"),
    operateRunSetupMount: document.getElementById("operateRunSetupMount"),
    operateActionsMount: document.getElementById("operateActionsMount"),
    operateTimelineMount: document.getElementById("operateTimelineMount"),
    operateOutputMount: document.getElementById("operateOutputMount"),
  };

  function setStatus(message, tone = "idle") {
    state.status = tone;
    el.sharedStatusPanel.textContent = message;
    el.sharedStatusPanel.dataset.state = tone;
    el.statusBadge.textContent = `Status: ${tone}`;
  }

  function setSelectedAgent(agentId) {
    state.selectedAgentId = agentId;
    el.agentBadge.textContent = agentId ? `Agent: ${agentId}` : "No agent selected";
  }

  function writeOutput(title, data) {
    state.lastOperation = { title, data };
    const text = JSON.stringify(data, null, 2);
    el.operateOutputMount.innerHTML = `
      <section class="output-block">
        <h3>${title}</h3>
        <pre>${text}</pre>
      </section>
    `;
  }

  function renderPlaceholderPanels() {
    el.operateAgentListMount.innerHTML = `
      <div class="placeholder-block">
        <p>Agent list will load from <code>/agents</code>.</p>
        <p class="muted">Status categories: idle, running, blocked, error</p>
      </div>
    `;

    el.operateRunSetupMount.innerHTML = `
      <div class="placeholder-block">
        <label class="field">
          <span>Program</span>
          <input value="${programId}" readonly />
        </label>
        <label class="field">
          <span>Workflow Template</span>
          <select>
            <option value="plan">Plan Phase</option>
            <option value="execute">Execute Phase</option>
            <option value="verify">Verify Work</option>
          </select>
        </label>
        <label class="field">
          <span>Task Prompt</span>
          <textarea rows="3" placeholder="Describe run intent"></textarea>
        </label>
      </div>
    `;

    el.operateActionsMount.innerHTML = `
      <div class="placeholder-block">
        <p>Action controls call existing endpoints only:</p>
        <ul>
          <li><code>/agents/register</code>, <code>/agents</code>, <code>/agents/{agent_id}</code></li>
          <li><code>/programs/${programId}/locks</code></li>
          <li><code>/programs/${programId}/mutations</code></li>
          <li><code>/programs/${programId}/verify</code>, <code>/simulate</code>, <code>/compile</code></li>
          <li><code>/programs/${programId}/history</code></li>
        </ul>
      </div>
    `;

    el.operateTimelineMount.innerHTML = `
      <div class="placeholder-block">
        <p>Timeline preview surfaces outcomes from lock, mutation, and verify steps.</p>
      </div>
    `;

    writeOutput("Dashboard", {
      program_id: Number(programId),
      mode: "endpoint-first",
      observe_path: `/programs/${programId}/observability`,
    });
  }

  function ensureObserveEmbedded() {
    if (el.observeMount.querySelector("iframe")) {
      return;
    }

    const iframe = document.createElement("iframe");
    iframe.title = "Observe panel";
    iframe.loading = "lazy";
    iframe.src = el.observeMount.dataset.observeSrc;
    iframe.className = "observe-frame";

    iframe.addEventListener("load", () => {
      setStatus("Observe module ready.", "idle");
    });

    iframe.addEventListener("error", () => {
      setStatus("Observe failed to load. Open it in a new tab.", "error");
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
      setStatus("Observe tab active. Existing observability route reused.", "idle");
      return;
    }

    setStatus("Operate tab active.", "idle");
  }

  function bindTabNavigation() {
    el.tabButtons.forEach((btn) => {
      btn.addEventListener("click", () => activateTab(btn.dataset.tab));
    });
  }

  function init() {
    bindTabNavigation();
    renderPlaceholderPanels();

    el.openObserveLink.href = `/programs/${programId}/observability`;
    setSelectedAgent(null);

    activateTab("operate");
    setStatus("Dashboard shell initialized.", "idle");
  }

  init();
})();
