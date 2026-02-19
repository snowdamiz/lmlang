(() => {
  const programId = document.body.dataset.programId;

  const state = {
    activeTab: "operate",
    selectedAgentId: null,
    status: "idle",
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
  };

  function setStatus(message, tone = "idle") {
    state.status = tone;
    el.sharedStatusPanel.textContent = message;
    el.sharedStatusPanel.dataset.state = tone;
    el.statusBadge.textContent = `Status: ${tone}`;
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
        <p>Run setup controls mount here.</p>
        <ul>
          <li>Program selector: <code>${programId}</code></li>
          <li>Workflow template picker</li>
          <li>Task prompt input</li>
        </ul>
      </div>
    `;

    el.operateActionsMount.innerHTML = `
      <div class="placeholder-block">
        <p>Action controls will call existing endpoints:</p>
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
        <p>Timeline preview will show action results and history snippets.</p>
      </div>
    `;
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
    } else {
      setStatus("Operate tab active.", "idle");
    }
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
    el.agentBadge.textContent = "No agent selected";

    activateTab("operate");
    setStatus("Dashboard shell initialized.", "idle");
  }

  init();
})();
