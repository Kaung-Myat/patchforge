(() => {
  "use strict";

  const tokenKey = "patchforge.publisherToken";
  const sidebarKey = "patchforge.sidebarCollapsed";
  const state = {
    token: sessionStorage.getItem(tokenKey) || "",
    system: null,
    apps: [],
    app: null,
    releases: [],
    release: null,
  };
  const $ = (id) => document.getElementById(id);
  const elements = {
    loginPanel: $("loginPanel"), dashboard: $("dashboard"), loginForm: $("loginForm"),
    publisherToken: $("publisherToken"), loginError: $("loginError"), dashboardError: $("dashboardError"),
    connectionBadge: $("connectionBadge"), logoutButton: $("logoutButton"), refreshButton: $("refreshButton"),
    sidebarToggle: $("sidebarToggle"), pageTitle: $("pageTitle"), serverVersion: $("serverVersion"),
    stats: $("stats"), activityChart: $("activityChart"), activityEmpty: $("activityEmpty"),
    appsList: $("appsList"), releasesList: $("releasesList"), patchesList: $("patchesList"),
    showCreateApp: $("showCreateApp"), createAppForm: $("createAppForm"), appName: $("appName"),
    showCreateRelease: $("showCreateRelease"), createReleaseForm: $("createReleaseForm"),
    releaseVersion: $("releaseVersion"), releaseChannel: $("releaseChannel"),
    mobileSetupTitle: $("mobileSetupTitle"), initCommand: $("initCommand"), copyInit: $("copyInit"),
    appIdValue: $("appIdValue"), keyIdValue: $("keyIdValue"), publicKeyValue: $("publicKeyValue"),
    releasesTitle: $("releasesTitle"), patchesTitle: $("patchesTitle"), infrastructureDetails: $("infrastructureDetails"),
  };

  function showError(element, message) {
    element.textContent = message;
    element.classList.toggle("hidden", !message);
  }

  async function api(path, options = {}) {
    const headers = new Headers(options.headers || {});
    headers.set("Authorization", `Bearer ${state.token}`);
    if (options.body) headers.set("Content-Type", "application/json");
    const response = await fetch(path, { ...options, headers });
    if (!response.ok) {
      const raw = await response.text();
      let message = raw || `${response.status} ${response.statusText}`;
      try { message = JSON.parse(raw).message || message; } catch (_) { /* plain text response */ }
      const error = new Error(message);
      error.status = response.status;
      throw error;
    }
    return response.status === 204 ? null : response.json();
  }

  function el(tag, className, text) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = String(text);
    return node;
  }

  function svgEl(tag, attributes = {}) {
    const node = document.createElementNS("http://www.w3.org/2000/svg", tag);
    Object.entries(attributes).forEach(([name, value]) => node.setAttribute(name, String(value)));
    return node;
  }

  function formatBytes(bytes) {
    const value = Number(bytes || 0);
    if (value < 1024) return `${value} B`;
    const units = ["KB", "MB", "GB"];
    let size = value / 1024;
    let unit = units[0];
    for (let i = 1; size >= 1024 && i < units.length; i += 1) { size /= 1024; unit = units[i]; }
    return `${size.toFixed(size < 10 ? 1 : 0)} ${unit}`;
  }

  function setConnected(connected) {
    elements.loginPanel.classList.toggle("hidden", connected);
    elements.dashboard.classList.toggle("hidden", !connected);
    elements.connectionBadge.textContent = connected ? "Connected" : "Disconnected";
  }

  function setSidebar(collapsed) {
    document.body.classList.toggle("sidebar-collapsed", collapsed);
    elements.sidebarToggle.setAttribute("aria-expanded", String(!collapsed));
    elements.sidebarToggle.setAttribute("aria-label", collapsed ? "Expand sidebar" : "Collapse sidebar");
    sessionStorage.setItem(sidebarKey, collapsed ? "1" : "0");
  }

  function renderStats() {
    const stats = state.system.stats;
    const items = [
      ["▦", "Applications", stats.apps],
      ["◇", "Releases", stats.releases],
      ["⑂", "Published patches", stats.patches],
      ["▤", "Artifact data", formatBytes(stats.artifact_bytes)],
    ];
    elements.stats.replaceChildren(...items.map(([icon, label, value]) => {
      const card = el("div", "stat");
      const heading = el("div", "stat-heading");
      heading.append(el("span", "stat-icon", icon), el("span", "", label));
      card.append(heading, el("strong", "", value));
      return card;
    }));
  }

  function renderActivityChart() {
    const data = state.system.activity || [];
    elements.activityChart.replaceChildren();
    elements.activityEmpty.classList.toggle("hidden", data.length > 0);
    elements.activityChart.classList.toggle("hidden", data.length === 0);
    if (!data.length) return;

    const width = 760;
    const height = 260;
    const margin = { top: 18, right: 18, bottom: 42, left: 42 };
    const plotWidth = width - margin.left - margin.right;
    const plotHeight = height - margin.top - margin.bottom;
    const maximum = Math.max(1, ...data.map((point) => Number(point.patches)));
    const x = (index) => margin.left + (data.length === 1 ? plotWidth / 2 : (index / (data.length - 1)) * plotWidth);
    const y = (value) => margin.top + plotHeight - (Number(value) / maximum) * plotHeight;

    for (let index = 0; index <= 4; index += 1) {
      const position = margin.top + (plotHeight / 4) * index;
      elements.activityChart.append(svgEl("line", { x1: margin.left, y1: position, x2: width - margin.right, y2: position, class: "chart-grid" }));
      const label = svgEl("text", { x: margin.left - 10, y: position + 4, "text-anchor": "end", class: "chart-axis-label" });
      label.textContent = String(Math.round(maximum - (maximum / 4) * index));
      elements.activityChart.append(label);
    }

    const path = svgEl("path", {
      d: data.map((point, index) => `${index ? "L" : "M"} ${x(index)} ${y(point.patches)}`).join(" "),
      class: "chart-line",
    });
    elements.activityChart.append(path);

    const labelStep = Math.max(1, Math.ceil(data.length / 5));
    data.forEach((point, index) => {
      const circle = svgEl("circle", { cx: x(index), cy: y(point.patches), r: 4, class: "chart-point" });
      const title = svgEl("title");
      title.textContent = `${point.day}: ${point.patches} patch${Number(point.patches) === 1 ? "" : "es"}`;
      circle.append(title);
      elements.activityChart.append(circle);
      if (index % labelStep === 0 || index === data.length - 1) {
        const label = svgEl("text", { x: x(index), y: height - 12, "text-anchor": "middle", class: "chart-axis-label" });
        label.textContent = new Date(`${point.day}T00:00:00`).toLocaleDateString(undefined, { month: "short", day: "numeric" });
        elements.activityChart.append(label);
      }
    });
  }

  function renderInfrastructure() {
    const details = [
      ["Server version", `v${state.system.server_version}`],
      ["Database", state.system.database_provider],
      ["Storage provider", state.system.storage.provider],
      ["Artifact limit", formatBytes(state.system.storage.max_artifact_size_bytes)],
    ];
    elements.serverVersion.textContent = `v${state.system.server_version}`;
    elements.infrastructureDetails.replaceChildren(...details.map(([term, value]) => {
      const row = el("div");
      row.append(el("dt", "", term), el("dd", "", value));
      return row;
    }));
  }

  function renderApps() {
    if (!state.apps.length) {
      elements.appsList.replaceChildren(el("p", "empty", "No applications yet."));
      return;
    }
    elements.appsList.replaceChildren(...state.apps.map((app) => {
      const button = el("button", `list-item${state.app?.id === app.id ? " active" : ""}`);
      button.type = "button";
      const content = el("span");
      content.append(el("strong", "", app.name), el("small", "", app.id));
      button.append(content, el("span", "pill", "Flutter"));
      button.addEventListener("click", () => selectApp(app));
      return button;
    }));
  }

  function renderMobileSetup() {
    const app = state.app;
    elements.mobileSetupTitle.textContent = app ? app.name : "No app selected";
    elements.appIdValue.textContent = app?.id || "—";
    elements.keyIdValue.textContent = state.system.signing.key_id;
    elements.publicKeyValue.textContent = state.system.signing.public_key_base64;
    elements.initCommand.textContent = app
      ? `patchforge init --url ${location.origin} --app-id ${app.id}`
      : `patchforge init --url ${location.origin}`;
  }

  function renderReleases() {
    elements.releasesTitle.textContent = state.app ? `Releases · ${state.app.name}` : "Releases";
    elements.showCreateRelease.classList.toggle("hidden", !state.app);
    if (!state.app) {
      elements.releasesList.replaceChildren(el("p", "empty", "Select an application to load releases."));
      return;
    }
    if (!state.releases.length) {
      elements.releasesList.replaceChildren(el("p", "empty", "No releases yet."));
      return;
    }
    elements.releasesList.replaceChildren(...state.releases.map((release) => {
      const button = el("button", `list-item${state.release?.id === release.id ? " active" : ""}`);
      button.type = "button";
      const content = el("span");
      content.append(el("strong", "", release.version), el("small", "", release.id));
      button.append(content, el("span", "pill", release.channel));
      button.addEventListener("click", () => selectRelease(release));
      return button;
    }));
  }

  function renderPatches(patches = []) {
    elements.patchesTitle.textContent = state.release ? `Patch chain · ${state.release.version}` : "Patch chain";
    if (!state.release) {
      elements.patchesList.replaceChildren(el("p", "empty", "Select a release to inspect its patches."));
      return;
    }
    if (!patches.length) {
      elements.patchesList.replaceChildren(el("p", "empty", "No patches published for this release."));
      return;
    }
    const table = el("table");
    const header = el("tr");
    ["#", "Architecture", "Format", "Size", "Created"].forEach((value) => header.append(el("th", "", value)));
    const head = el("thead");
    const body = el("tbody");
    head.append(header);
    patches.forEach((patch) => {
      const row = el("tr");
      [patch.patch_number, patch.architecture, patch.patch_format, formatBytes(patch.file_size), new Date(patch.created_at).toLocaleString()]
        .forEach((value) => row.append(el("td", "", value)));
      body.append(row);
    });
    table.append(head, body);
    elements.patchesList.replaceChildren(table);
  }

  async function selectApp(app) {
    state.app = app;
    state.release = null;
    state.releases = [];
    elements.createReleaseForm.classList.add("hidden");
    renderApps(); renderMobileSetup(); renderReleases(); renderPatches();
    try {
      state.releases = await api(`/api/v1/apps/${app.id}/releases`);
      renderReleases();
    } catch (error) { showError(elements.dashboardError, error.message); }
  }

  async function selectRelease(release) {
    state.release = release;
    renderReleases(); renderPatches();
    try { renderPatches(await api(`/api/v1/releases/${release.id}/patches`)); }
    catch (error) { showError(elements.dashboardError, error.message); }
  }

  async function loadDashboard() {
    showError(elements.dashboardError, "");
    const [system, apps] = await Promise.all([api("/api/v1/system"), api("/api/v1/apps")]);
    state.system = system;
    state.apps = apps;
    if (state.app) state.app = apps.find((app) => app.id === state.app.id) || null;
    renderStats(); renderActivityChart(); renderInfrastructure(); renderApps(); renderMobileSetup();
    if (state.app) await selectApp(state.app); else { renderReleases(); renderPatches(); }
    setConnected(true);
  }

  document.querySelectorAll(".nav-item").forEach((item) => {
    item.addEventListener("click", () => {
      document.querySelectorAll(".nav-item").forEach((link) => link.classList.remove("active"));
      item.classList.add("active");
      elements.pageTitle.textContent = item.dataset.title;
    });
  });
  elements.sidebarToggle.addEventListener("click", () => setSidebar(!document.body.classList.contains("sidebar-collapsed")));
  elements.loginForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    showError(elements.loginError, "");
    state.token = elements.publisherToken.value.trim();
    if (!state.token) return;
    try {
      await loadDashboard();
      sessionStorage.setItem(tokenKey, state.token);
      elements.publisherToken.value = "";
    } catch (error) {
      state.token = "";
      showError(elements.loginError, error.status === 401 ? "Publisher token is not valid." : error.message);
    }
  });
  elements.logoutButton.addEventListener("click", () => {
    sessionStorage.removeItem(tokenKey);
    state.token = ""; state.app = null; state.release = null;
    setConnected(false);
  });
  elements.refreshButton.addEventListener("click", () => loadDashboard().catch((error) => showError(elements.dashboardError, error.message)));
  elements.showCreateApp.addEventListener("click", () => { elements.createAppForm.classList.toggle("hidden"); elements.appName.focus(); });
  elements.showCreateRelease.addEventListener("click", () => { elements.createReleaseForm.classList.toggle("hidden"); elements.releaseVersion.focus(); });
  elements.copyInit.addEventListener("click", async () => {
    await navigator.clipboard.writeText(elements.initCommand.textContent);
    elements.copyInit.textContent = "✓";
    elements.copyInit.setAttribute("aria-label", "Initialization command copied");
    setTimeout(() => { elements.copyInit.textContent = "□"; elements.copyInit.setAttribute("aria-label", "Copy initialization command"); }, 1200);
  });
  elements.createAppForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    showError(elements.dashboardError, "");
    try {
      const app = await api("/api/v1/apps", { method: "POST", body: JSON.stringify({ name: elements.appName.value.trim() }) });
      elements.appName.value = "";
      elements.createAppForm.classList.add("hidden");
      state.apps.unshift(app);
      await selectApp(app);
      await loadDashboard();
    } catch (error) { showError(elements.dashboardError, error.message); }
  });
  elements.createReleaseForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (!state.app) return;
    showError(elements.dashboardError, "");
    try {
      const release = await api(`/api/v1/apps/${state.app.id}/releases`, {
        method: "POST",
        body: JSON.stringify({ version: elements.releaseVersion.value.trim(), platform: "android", channel: elements.releaseChannel.value }),
      });
      elements.releaseVersion.value = "";
      elements.createReleaseForm.classList.add("hidden");
      state.releases.unshift(release);
      await selectRelease(release);
      await loadDashboard();
    } catch (error) { showError(elements.dashboardError, error.message); }
  });

  setSidebar(sessionStorage.getItem(sidebarKey) === "1");
  setConnected(false);
  if (state.token) loadDashboard().catch(() => {
    sessionStorage.removeItem(tokenKey);
    state.token = "";
    setConnected(false);
  });
})();
