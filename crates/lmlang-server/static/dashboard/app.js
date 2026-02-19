(() => {
  const initialProgramIdRaw = document.body.dataset.initialProgramId || "";
  const initialProgramId = Number(initialProgramIdRaw);
  const SETUP_STORAGE_KEY = "lmlang.dashboard.first_time_setup.v1";

  const state = {
    programs: [],
    agents: [],
    projectAgents: [],
    selectedProgramId: Number.isFinite(initialProgramId) && initialProgramId > 0 ? initialProgramId : null,
    selectedAgentId: null,
    selectedProjectAgentId: null,
    transcript: [],
  };

  const el = {
    projectNameInput: document.getElementById("projectNameInput"),
    createProjectBtn: document.getElementById("createProjectBtn"),
    refreshProjectsBtn: document.getElementById("refreshProjectsBtn"),
    createHelloWorldBtn: document.getElementById("createHelloWorldBtn"),
    projectList: document.getElementById("projectList"),
    observeLink: document.getElementById("observeLink"),

    agentNameInput: document.getElementById("agentNameInput"),
    registerAgentBtn: document.getElementById("registerAgentBtn"),
    refreshAgentsBtn: document.getElementById("refreshAgentsBtn"),
    agentProviderSelect: document.getElementById("agentProviderSelect"),
    agentModelInput: document.getElementById("agentModelInput"),
    agentBaseUrlInput: document.getElementById("agentBaseUrlInput"),
    agentApiKeyInput: document.getElementById("agentApiKeyInput"),
    agentSystemPromptInput: document.getElementById("agentSystemPromptInput"),
    saveAgentConfigBtn: document.getElementById("saveAgentConfigBtn"),
    agentAssignSelect: document.getElementById("agentAssignSelect"),
    assignAgentBtn: document.getElementById("assignAgentBtn"),
    projectAgentList: document.getElementById("projectAgentList"),

    setupWizard: document.getElementById("setupWizard"),
    setupAgentNameInput: document.getElementById("setupAgentNameInput"),
    setupProviderSelect: document.getElementById("setupProviderSelect"),
    setupModelInput: document.getElementById("setupModelInput"),
    setupApiKeyInput: document.getElementById("setupApiKeyInput"),
    setupBaseUrlInput: document.getElementById("setupBaseUrlInput"),
    setupCompleteBtn: document.getElementById("setupCompleteBtn"),
    setupSkipBtn: document.getElementById("setupSkipBtn"),

    goalInput: document.getElementById("goalInput"),
    startBuildBtn: document.getElementById("startBuildBtn"),
    stopBuildBtn: document.getElementById("stopBuildBtn"),
    chatLog: document.getElementById("chatLog"),
    chatInput: document.getElementById("chatInput"),
    sendChatBtn: document.getElementById("sendChatBtn"),

    activeProjectBadge: document.getElementById("activeProjectBadge"),
    activeAgentBadge: document.getElementById("activeAgentBadge"),
    runStatusBadge: document.getElementById("runStatusBadge"),
    statusBar: document.getElementById("statusBar"),
  };

  function setStatus(message, tone = "idle") {
    el.statusBar.textContent = message;
    el.statusBar.dataset.state = tone;
  }

  function safeStorageGet(key) {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  }

  function safeStorageSet(key, value) {
    try {
      localStorage.setItem(key, value);
    } catch {
      // no-op
    }
  }

  function isSetupComplete() {
    return safeStorageGet(SETUP_STORAGE_KEY) === "1";
  }

  function markSetupComplete() {
    safeStorageSet(SETUP_STORAGE_KEY, "1");
  }

  function hasConfiguredAgent() {
    return state.agents.some((agent) => {
      const llm = agent.llm || {};
      return Boolean(llm.provider && llm.model && llm.api_key_configured);
    });
  }

  function showSetupWizard() {
    if (el.setupWizard) {
      el.setupWizard.classList.remove("hidden");
    }
  }

  function hideSetupWizard() {
    if (el.setupWizard) {
      el.setupWizard.classList.add("hidden");
    }
  }

  function maybeShowSetupWizard() {
    if (hasConfiguredAgent()) {
      markSetupComplete();
      hideSetupWizard();
      return;
    }

    if (!isSetupComplete()) {
      showSetupWizard();
      return;
    }

    hideSetupWizard();
  }

  function updateBadges() {
    const selectedProject = state.programs.find((p) => p.id === state.selectedProgramId);
    const selectedProjectAgent = state.projectAgents.find(
      (a) => a.agent_id === state.selectedProjectAgentId
    );

    el.activeProjectBadge.textContent = selectedProject
      ? `Project: ${selectedProject.name} (#${selectedProject.id})`
      : "Project: none";

    el.activeAgentBadge.textContent = selectedProjectAgent
      ? `Agent: ${selectedProjectAgent.name || selectedProjectAgent.agent_id}`
      : "Agent: none";

    el.runStatusBadge.textContent = selectedProjectAgent
      ? `Run: ${selectedProjectAgent.run_status}`
      : "Run: idle";
  }

  async function api(method, path, body) {
    const response = await fetch(path, {
      method,
      headers: {
        "content-type": "application/json",
      },
      body: body ? JSON.stringify(body) : undefined,
    });

    const text = await response.text();
    const data = text ? JSON.parse(text) : null;

    if (!response.ok) {
      const message =
        data?.error?.message || data?.message || `request failed (${response.status})`;
      throw new Error(message);
    }

    return data;
  }

  function renderProjects() {
    if (!state.programs.length) {
      el.projectList.innerHTML = '<li class="list-item">No projects yet.</li>';
      return;
    }

    el.projectList.innerHTML = state.programs
      .map((program) => {
        const selected = program.id === state.selectedProgramId ? "selected" : "";
        return `
          <li class="list-item ${selected}">
            <p class="item-title">${program.name}</p>
            <p class="item-meta">program_id=${program.id}</p>
            <button type="button" data-project-id="${program.id}">Select Project</button>
          </li>
        `;
      })
      .join("");

    el.projectList.querySelectorAll("[data-project-id]").forEach((button) => {
      button.addEventListener("click", async () => {
        await selectProject(Number(button.dataset.projectId));
      });
    });
  }

  function renderAgentAssignOptions() {
    if (!state.agents.length) {
      el.agentAssignSelect.innerHTML = '<option value="">No registered agents</option>';
      renderSelectedAgentConfig();
      return;
    }

    el.agentAssignSelect.innerHTML = state.agents
      .map((agent) => {
        const label = `${agent.name || "unnamed-agent"} (${agent.agent_id})`;
        const selected = agent.agent_id === state.selectedAgentId ? "selected" : "";
        return `<option value="${agent.agent_id}" ${selected}>${label}</option>`;
      })
      .join("");

    if (!state.selectedAgentId) {
      state.selectedAgentId = state.agents[0].agent_id;
    }

    renderSelectedAgentConfig();
  }

  function selectedAgent() {
    return state.agents.find((agent) => agent.agent_id === state.selectedAgentId) || null;
  }

  function renderSelectedAgentConfig() {
    const agent = selectedAgent();
    if (!agent || !agent.llm) {
      el.agentProviderSelect.value = "";
      el.agentModelInput.value = "";
      el.agentBaseUrlInput.value = "";
      el.agentApiKeyInput.value = "";
      el.agentSystemPromptInput.value = "";
      return;
    }

    el.agentProviderSelect.value = agent.llm.provider || "";
    el.agentModelInput.value = agent.llm.model || "";
    el.agentBaseUrlInput.value = agent.llm.api_base_url || "";
    el.agentApiKeyInput.value = "";
    el.agentSystemPromptInput.value = agent.llm.system_prompt || "";
  }

  function buildConfigPayload(provider, model, baseUrl, apiKey, systemPrompt) {
    let resolvedProvider = provider || null;
    let resolvedBaseUrl = (baseUrl || "").trim() || null;

    if (resolvedProvider === "openrouter" && !resolvedBaseUrl) {
      resolvedBaseUrl = "https://openrouter.ai/api/v1";
    }

    if (!resolvedProvider) {
      resolvedProvider = null;
    }

    return {
      provider: resolvedProvider,
      model: (model || "").trim() || null,
      api_base_url: resolvedBaseUrl,
      api_key: (apiKey || "").trim() || null,
      system_prompt: (systemPrompt || "").trim() || null,
    };
  }

  function agentConfigPayload() {
    return buildConfigPayload(
      el.agentProviderSelect.value,
      el.agentModelInput.value,
      el.agentBaseUrlInput.value,
      el.agentApiKeyInput.value,
      el.agentSystemPromptInput.value
    );
  }

  function setupConfigPayload() {
    return buildConfigPayload(
      el.setupProviderSelect.value,
      el.setupModelInput.value,
      el.setupBaseUrlInput.value,
      el.setupApiKeyInput.value,
      null
    );
  }

  function renderProjectAgents() {
    if (!state.selectedProgramId) {
      el.projectAgentList.innerHTML = '<li class="list-item">Select a project first.</li>';
      return;
    }

    if (!state.projectAgents.length) {
      el.projectAgentList.innerHTML = '<li class="list-item">No assigned agents for this project.</li>';
      return;
    }

    el.projectAgentList.innerHTML = state.projectAgents
      .map((session) => {
        const selected = session.agent_id === state.selectedProjectAgentId ? "selected" : "";
        return `
          <li class="list-item ${selected}">
            <p class="item-title">${session.name || "unnamed-agent"}</p>
            <p class="item-meta">${session.agent_id}</p>
            <p class="item-meta">status=${session.run_status}${session.active_goal ? ` goal=${session.active_goal}` : ""}</p>
            <button type="button" data-project-agent-id="${session.agent_id}">Select Agent</button>
          </li>
        `;
      })
      .join("");

    el.projectAgentList
      .querySelectorAll("[data-project-agent-id]")
      .forEach((button) => {
        button.addEventListener("click", async () => {
          state.selectedProjectAgentId = button.dataset.projectAgentId;
          await refreshProjectAgentDetail();
          renderProjectAgents();
          updateBadges();
        });
      });
  }

  function renderChatLog() {
    if (!state.transcript.length) {
      el.chatLog.innerHTML = '<div class="chat-msg system"><p class="chat-role">system</p><p class="chat-content">No chat yet.</p></div>';
      return;
    }

    el.chatLog.innerHTML = state.transcript
      .map((entry) => {
        return `
          <div class="chat-msg ${entry.role}">
            <p class="chat-role">${entry.role} • ${entry.timestamp}</p>
            <p class="chat-content">${entry.content}</p>
          </div>
        `;
      })
      .join("");
    el.chatLog.scrollTop = el.chatLog.scrollHeight;
  }

  async function refreshPrograms() {
    const response = await api("GET", "/programs");
    state.programs = (response.programs || []).map((program) => ({
      id: Number(program.id),
      name: program.name,
    }));

    if (
      state.selectedProgramId &&
      !state.programs.some((program) => program.id === state.selectedProgramId)
    ) {
      state.selectedProgramId = null;
      state.projectAgents = [];
      state.selectedProjectAgentId = null;
      state.transcript = [];
    }

    renderProjects();
    updateObserveLink();
    updateBadges();
  }

  async function refreshAgents() {
    const response = await api("GET", "/agents");
    state.agents = response.agents || [];

    if (
      state.selectedAgentId &&
      !state.agents.some((agent) => agent.agent_id === state.selectedAgentId)
    ) {
      state.selectedAgentId = null;
    }

    renderAgentAssignOptions();
    renderSelectedAgentConfig();
    maybeShowSetupWizard();
  }

  async function refreshProjectAgents() {
    if (!state.selectedProgramId) {
      state.projectAgents = [];
      state.selectedProjectAgentId = null;
      state.transcript = [];
      renderProjectAgents();
      renderChatLog();
      updateBadges();
      return;
    }

    const response = await api("GET", `/programs/${state.selectedProgramId}/agents`);
    state.projectAgents = response.agents || [];

    if (
      state.selectedProjectAgentId &&
      !state.projectAgents.some((s) => s.agent_id === state.selectedProjectAgentId)
    ) {
      state.selectedProjectAgentId = null;
      state.transcript = [];
    }

    if (!state.selectedProjectAgentId && state.projectAgents.length > 0) {
      state.selectedProjectAgentId = state.projectAgents[0].agent_id;
      await refreshProjectAgentDetail();
    }

    renderProjectAgents();
    updateBadges();
  }

  async function refreshProjectAgentDetail() {
    if (!state.selectedProgramId || !state.selectedProjectAgentId) {
      state.transcript = [];
      renderChatLog();
      return;
    }

    const response = await api(
      "GET",
      `/programs/${state.selectedProgramId}/agents/${state.selectedProjectAgentId}`
    );
    state.transcript = response.transcript || [];

    const idx = state.projectAgents.findIndex(
      (session) => session.agent_id === state.selectedProjectAgentId
    );
    if (idx >= 0) {
      state.projectAgents[idx] = response.session;
    }

    renderChatLog();
    updateBadges();
  }

  async function selectProject(programId) {
    state.selectedProgramId = programId;
    state.selectedProjectAgentId = null;
    state.transcript = [];

    await api("POST", `/programs/${programId}/load`, {});
    await refreshProjectAgents();

    renderProjects();
    renderProjectAgents();
    renderChatLog();
    updateObserveLink();
    updateBadges();
    setStatus(`Selected project ${programId}.`, "idle");
  }

  function updateObserveLink() {
    if (!state.selectedProgramId) {
      el.observeLink.href = "#";
      el.observeLink.textContent = "Open selected project in Observe";
      return;
    }

    el.observeLink.href = `/programs/${state.selectedProgramId}/observability`;
    el.observeLink.textContent = `Open project ${state.selectedProgramId} in Observe`;
  }

  async function onCreateProject() {
    const name = el.projectNameInput.value.trim();
    if (!name) {
      setStatus("Project name is required.", "error");
      return;
    }

    try {
      const response = await api("POST", "/programs", { name });
      const programId = Number(response.id);
      el.projectNameInput.value = "";
      await refreshPrograms();
      await selectProject(programId);
      setStatus(`Created project '${name}' (${programId}).`, "idle");
    } catch (error) {
      setStatus(`Create project failed: ${error.message}`, "error");
    }
  }

  async function onRegisterAgent() {
    const name = el.agentNameInput.value.trim();

    try {
      const response = await api("POST", "/agents/register", {
        name: name || null,
        ...agentConfigPayload(),
      });
      el.agentNameInput.value = "";
      el.agentApiKeyInput.value = "";
      state.selectedAgentId = response.agent_id;
      await refreshAgents();
      if (response?.llm?.api_key_configured && response?.llm?.model && response?.llm?.provider) {
        markSetupComplete();
        hideSetupWizard();
      }
      setStatus(`Registered agent ${response.agent_id}.`, "idle");
    } catch (error) {
      setStatus(`Register agent failed: ${error.message}`, "error");
    }
  }

  async function onSaveAgentConfig() {
    const agentId = el.agentAssignSelect.value || state.selectedAgentId;
    if (!agentId) {
      setStatus("Select an agent first.", "error");
      return;
    }

    try {
      await api("POST", `/agents/${agentId}/config`, agentConfigPayload());
      el.agentApiKeyInput.value = "";
      state.selectedAgentId = agentId;
      await refreshAgents();
      if (hasConfiguredAgent()) {
        markSetupComplete();
        hideSetupWizard();
      }
      setStatus(`Saved config for agent ${agentId}.`, "idle");
    } catch (error) {
      setStatus(`Save config failed: ${error.message}`, "error");
    }
  }

  async function onCompleteSetupWizard() {
    const model = el.setupModelInput.value.trim();
    const apiKey = el.setupApiKeyInput.value.trim();

    if (!model || !apiKey) {
      setStatus("Setup requires model and api key.", "error");
      return;
    }

    try {
      const response = await api("POST", "/agents/register", {
        name: el.setupAgentNameInput.value.trim() || "builder",
        ...setupConfigPayload(),
      });
      el.setupApiKeyInput.value = "";
      state.selectedAgentId = response.agent_id;
      markSetupComplete();
      hideSetupWizard();
      await refreshAgents();
      setStatus("First-time AI setup complete.", "idle");
    } catch (error) {
      setStatus(`Setup failed: ${error.message}`, "error");
    }
  }

  function onSkipSetupWizard() {
    markSetupComplete();
    hideSetupWizard();
    setStatus("Setup skipped. You can configure provider settings in Agents.", "idle");
  }

  async function onCreateHelloWorldScaffold() {
    if (!state.selectedProgramId) {
      setStatus("Select a project first.", "error");
      return;
    }

    try {
      setStatus("Creating hello world scaffold...", "running");

      await api("POST", `/programs/${state.selectedProgramId}/load`, {});

      const createFunctionResponse = await api(
        "POST",
        `/programs/${state.selectedProgramId}/mutations`,
        {
          mutations: [
            {
              type: "AddFunction",
              name: "hello_world",
              module: 0,
              params: [],
              return_type: 7,
              visibility: "Public",
            },
          ],
          dry_run: false,
        }
      );

      let helloWorldFunctionId = null;
      if (Array.isArray(createFunctionResponse.created)) {
        const createdFunction = createFunctionResponse.created.find(
          (entry) => entry.type === "Function"
        );
        if (createdFunction && Number.isInteger(createdFunction.id)) {
          helloWorldFunctionId = createdFunction.id;
        }
      }

      if (helloWorldFunctionId !== null) {
        await api("POST", `/programs/${state.selectedProgramId}/mutations`, {
          mutations: [
            {
              type: "InsertNode",
              op: { Core: "Return" },
              owner: helloWorldFunctionId,
            },
          ],
          dry_run: false,
        });
      }

      await api("POST", `/programs/${state.selectedProgramId}/verify`, {
        scope: "full",
        affected_nodes: [],
      });

      const queryPreview = await api(
        "POST",
        `/programs/${state.selectedProgramId}/observability/query`,
        {
          query: "hello world",
          max_results: 5,
        }
      );
      const results = Array.isArray(queryPreview.results) ? queryPreview.results.length : 0;

      setStatus(
        `Hello world scaffold created. Open Observe and query 'hello world' (${results} preview results).`,
        "idle"
      );
    } catch (error) {
      setStatus(`Hello world scaffold failed: ${error.message}`, "error");
    }
  }

  async function onAssignAgent() {
    if (!state.selectedProgramId) {
      setStatus("Select a project first.", "error");
      return;
    }

    const agentId = el.agentAssignSelect.value;
    if (!agentId) {
      setStatus("Select an agent to assign.", "error");
      return;
    }

    try {
      await api(
        "POST",
        `/programs/${state.selectedProgramId}/agents/${agentId}/assign`,
        {}
      );
      state.selectedProjectAgentId = agentId;
      await refreshProjectAgents();
      await refreshProjectAgentDetail();
      setStatus(`Assigned agent ${agentId} to project ${state.selectedProgramId}.`, "idle");
    } catch (error) {
      setStatus(`Assign agent failed: ${error.message}`, "error");
    }
  }

  async function onStartBuild() {
    if (!state.selectedProgramId || !state.selectedProjectAgentId) {
      setStatus("Select a project and an assigned agent first.", "error");
      return;
    }

    const goal = el.goalInput.value.trim();
    if (!goal) {
      setStatus("Build goal is required.", "error");
      return;
    }

    try {
      await api(
        "POST",
        `/programs/${state.selectedProgramId}/agents/${state.selectedProjectAgentId}/start`,
        { goal }
      );
      await refreshProjectAgents();
      await refreshProjectAgentDetail();
      setStatus("Build started.", "running");
    } catch (error) {
      setStatus(`Start build failed: ${error.message}`, "error");
    }
  }

  async function onStopBuild() {
    if (!state.selectedProgramId || !state.selectedProjectAgentId) {
      setStatus("Select a project and an assigned agent first.", "error");
      return;
    }

    try {
      await api(
        "POST",
        `/programs/${state.selectedProgramId}/agents/${state.selectedProjectAgentId}/stop`,
        { reason: "Stopped from dashboard" }
      );
      await refreshProjectAgents();
      await refreshProjectAgentDetail();
      setStatus("Build stopped.", "stopped");
    } catch (error) {
      setStatus(`Stop build failed: ${error.message}`, "error");
    }
  }

  async function onSendChat() {
    const message = el.chatInput.value.trim();
    if (!message) {
      setStatus("Chat message is required.", "error");
      return;
    }

    try {
      const response = await api(
        "POST",
        "/dashboard/ai/chat",
        {
          message,
          selected_program_id: state.selectedProgramId,
          selected_agent_id: state.selectedAgentId,
          selected_project_agent_id: state.selectedProjectAgentId,
        }
      );

      el.chatInput.value = "";
      state.selectedProgramId = Number.isInteger(response.selected_program_id)
        ? response.selected_program_id
        : null;
      state.selectedAgentId = response.selected_agent_id || null;
      state.selectedProjectAgentId = response.selected_project_agent_id || null;

      await refreshPrograms();
      await refreshAgents();
      await refreshProjectAgents();

      if (Array.isArray(response.transcript)) {
        state.transcript = response.transcript;
        renderChatLog();
      }

      renderProjects();
      renderAgentAssignOptions();
      renderProjectAgents();
      updateBadges();
      updateObserveLink();
      setStatus(response.reply || "AI action completed.", "idle");
    } catch (error) {
      setStatus(`Chat failed: ${error.message}`, "error");
    }
  }

  function bindEvents() {
    el.createProjectBtn.addEventListener("click", onCreateProject);
    el.createHelloWorldBtn.addEventListener("click", onCreateHelloWorldScaffold);
    el.refreshProjectsBtn.addEventListener("click", async () => {
      try {
        await refreshPrograms();
        setStatus("Projects refreshed.", "idle");
      } catch (error) {
        setStatus(`Refresh projects failed: ${error.message}`, "error");
      }
    });

    el.registerAgentBtn.addEventListener("click", onRegisterAgent);
    el.saveAgentConfigBtn.addEventListener("click", onSaveAgentConfig);
    el.refreshAgentsBtn.addEventListener("click", async () => {
      try {
        await refreshAgents();
        setStatus("Agents refreshed.", "idle");
      } catch (error) {
        setStatus(`Refresh agents failed: ${error.message}`, "error");
      }
    });

    el.assignAgentBtn.addEventListener("click", onAssignAgent);
    el.startBuildBtn.addEventListener("click", onStartBuild);
    el.stopBuildBtn.addEventListener("click", onStopBuild);
    el.sendChatBtn.addEventListener("click", onSendChat);

    el.agentAssignSelect.addEventListener("change", () => {
      state.selectedAgentId = el.agentAssignSelect.value || null;
      renderSelectedAgentConfig();
    });

    el.agentProviderSelect.addEventListener("change", () => {
      if (
        el.agentProviderSelect.value === "openrouter" &&
        !el.agentBaseUrlInput.value.trim()
      ) {
        el.agentBaseUrlInput.value = "https://openrouter.ai/api/v1";
      }
    });

    el.setupCompleteBtn.addEventListener("click", onCompleteSetupWizard);
    el.setupSkipBtn.addEventListener("click", onSkipSetupWizard);
    el.setupProviderSelect.addEventListener("change", () => {
      if (
        el.setupProviderSelect.value === "openrouter" &&
        !el.setupBaseUrlInput.value.trim()
      ) {
        el.setupBaseUrlInput.value = "https://openrouter.ai/api/v1";
      }
    });
  }

  async function init() {
    bindEvents();

    try {
      setStatus("Loading projects and agents...", "running");
      await refreshPrograms();
      await refreshAgents();

      if (state.selectedProgramId) {
        await selectProject(state.selectedProgramId);
      }

      renderProjects();
      renderAgentAssignOptions();
      renderProjectAgents();
      renderChatLog();
      updateObserveLink();
      updateBadges();
      maybeShowSetupWizard();
      setStatus("Dashboard ready.", "idle");
    } catch (error) {
      setStatus(`Initialization failed: ${error.message}`, "error");
    }
  }

  init();
})();
