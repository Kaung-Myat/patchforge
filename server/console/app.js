(() => {
  "use strict";

  const tokenKey = "patchforge.publisherToken";
  const state = { token: sessionStorage.getItem(tokenKey) || "", system: null, apps: [], app: null, releases: [], release: null };
  const $ = (id) => document.getElementById(id);

  const elements = {
    loginPanel: $("loginPanel"), dashboard: $("dashboard"), loginForm: $("loginForm"),
    publisherToken: $("publisherToken"), loginError: $("loginError"), dashboardError: $("dashboardError"),
    connectionBadge: $("connectionBadge"), logoutButton: $("logoutButton"), refreshButton: $("refreshButton"),
    stats: $("stats"), appsList: $("appsList"), releasesList: $("releasesList"), patchesList: $("patchesList"),
    showCreateApp: $("showCreateApp"), createAppForm: $("createAppForm"), appName: $("appName"),
    showCreateRelease: $("showCreateRelease"), createReleaseForm: $("createReleaseForm"),
    releaseVersion: $("releaseVersion"), releaseChannel: $("releaseChannel"),
    mobileSetupTitle: $("mobileSetupTitle"), initCommand: $("initCommand"), copyInit: $("copyInit"),
    appIdValue: $("appIdValue"), keyIdValue: $("keyIdValue"), publicKeyValue: $("publicKeyValue"),
    releasesTitle: $("releasesTitle"), patchesTitle: $("patchesTitle"), infrastructureDetails: $("infrastructureDetails")
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
      try { message = JSON.parse(raw).message || message; } catch (_) { /* plain-text server response */ }
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

  function formatBytes(bytes) {
    const value = Number(bytes || 0);
    if (value < 1024) return `${value} B`;
    const units = ["KB", "MB", "GB"];
    let size = value / 1024, unit = units[0];
    for (let i = 1; size >= 1024 && i < units.length; i += 1) { size /= 1024; unit = units[i]; }
    return `${size.toFixed(size < 10 ? 1 : 0)} ${unit}`;
  }

  function setConnected(connected) {
    elements.loginPanel.classList.toggle("hidden", connected);
    elements.dashboard.classList.toggle("hidden", !connected);
    elements.logoutButton.classList.toggle("hidden", !connected);
    elements.connectionBadge.className = `badge${connected ? "" : " muted"}`;
    elements.connectionBadge.textContent = connected ? "Server connected" : "Not connected";
  }

  function renderStats() {
    const stats = state.system.stats;
    elements.stats.replaceChildren(...[
      [stats.apps, "Applications"], [stats.releases, "Releases"], [stats.patches, "Published patches"],
      [formatBytes(stats.artifact_bytes), "Artifact data"]
    ].map(([value, label]) => {
      const box = el("div", "stat"); box.append(el("strong", "", value), el("span", "", label)); return box;
    }));
  }

  function renderInfrastructure() {
    const details = [
      ["Server", `v${state.system.server_version}`], ["Database", state.system.database_provider],
      ["Storage", state.system.storage.provider], ["Artifact limit", formatBytes(state.system.storage.max_artifact_size_bytes)]
    ];
    elements.infrastructureDetails.replaceChildren(...details.map(([term, value]) => {
      const row = el("div"); row.append(el("dt", "", term), el("dd", "", value)); return row;
    }));
  }

  function renderApps() {
    if (!state.apps.length) {
      elements.appsList.replaceChildren(el("p", "empty", "No applications yet. Create your first app.")); return;
    }
    elements.appsList.replaceChildren(...state.apps.map((app) => {
      const button = el("button", `list-item${state.app?.id === app.id ? " active" : ""}`);
      button.type = "button";
      const copy = el("span"); copy.append(el("strong", "", app.name), el("small", "", app.id));
      button.append(copy, el("span", "pill", "Flutter"));
      button.addEventListener("click", () => selectApp(app));
      return button;
    }));
  }

  function renderMobileSetup() {
    const app = state.app;
    elements.mobileSetupTitle.textContent = app ? app.name : "Select an app";
    elements.appIdValue.textContent = app?.id || "—";
    elements.keyIdValue.textContent = state.system.signing.key_id;
    elements.publicKeyValue.textContent = state.system.signing.public_key_base64;
    const command = app
      ? `patchforge init --url ${location.origin} --app-id ${app.id}`
      : `patchforge init --url ${location.origin}`;
    elements.initCommand.textContent = command;
  }

  function renderReleases() {
    elements.releasesTitle.textContent = state.app ? state.app.name : "Select an app";
    elements.showCreateRelease.classList.toggle("hidden", !state.app);
    if (!state.app) { elements.releasesList.replaceChildren(el("p", "empty", "No application selected.")); return; }
    if (!state.releases.length) { elements.releasesList.replaceChildren(el("p", "empty", "No releases yet.")); return; }
    elements.releasesList.replaceChildren(...state.releases.map((release) => {
      const button = el("button", `list-item${state.release?.id === release.id ? " active" : ""}`);
      button.type = "button";
      const copy = el("span"); copy.append(el("strong", "", release.version), el("small", "", release.id));
      button.append(copy, el("span", "pill", release.channel));
      button.addEventListener("click", () => selectRelease(release));
      return button;
    }));
  }

  function renderPatches(patches = []) {
    elements.patchesTitle.textContent = state.release ? `${state.release.version} · ${state.release.channel}` : "Select a release";
    if (!state.release) { elements.patchesList.replaceChildren(el("p", "empty", "No release selected.")); return; }
    if (!patches.length) { elements.patchesList.replaceChildren(el("p", "empty", "No patches published for this release.")); return; }
    const table = el("table");
    const header = el("tr"); ["#", "Architecture", "Format", "Size", "Created"].forEach((value) => header.append(el("th", "", value)));
    const head = el("thead"); head.append(header); const body = el("tbody");
    patches.forEach((patch) => {
      const row = el("tr");
      [patch.patch_number, patch.architecture, patch.patch_format, formatBytes(patch.file_size), new Date(patch.created_at).toLocaleString()]
        .forEach((value) => row.append(el("td", "", value)));
      body.append(row);
    });
    table.append(head, body); elements.patchesList.replaceChildren(table);
  }

  async function selectApp(app) {
    state.app = app; state.release = null; state.releases = [];
    elements.createReleaseForm.classList.add("hidden");
    renderApps(); renderMobileSetup(); renderReleases(); renderPatches();
    try { state.releases = await api(`/api/v1/apps/${app.id}/releases`); renderReleases(); }
    catch (error) { showError(elements.dashboardError, error.message); }
  }

  async function selectRelease(release) {
    state.release = release; renderReleases(); renderPatches();
    try { renderPatches(await api(`/api/v1/releases/${release.id}/patches`)); }
    catch (error) { showError(elements.dashboardError, error.message); }
  }

  async function loadDashboard() {
    showError(elements.dashboardError, "");
    const [system, apps] = await Promise.all([api("/api/v1/system"), api("/api/v1/apps")]);
    state.system = system; state.apps = apps;
    if (state.app) state.app = apps.find((app) => app.id === state.app.id) || null;
    renderStats(); renderInfrastructure(); renderApps(); renderMobileSetup();
    if (state.app) await selectApp(state.app); else { renderReleases(); renderPatches(); }
    setConnected(true);
  }

  elements.loginForm.addEventListener("submit", async (event) => {
    event.preventDefault(); showError(elements.loginError, "");
    state.token = elements.publisherToken.value.trim();
    if (!state.token) return;
    try { await loadDashboard(); sessionStorage.setItem(tokenKey, state.token); elements.publisherToken.value = ""; }
    catch (error) { state.token = ""; showError(elements.loginError, error.status === 401 ? "Publisher token is not valid." : error.message); }
  });

  elements.logoutButton.addEventListener("click", () => {
    sessionStorage.removeItem(tokenKey); state.token = ""; state.app = null; state.release = null; setConnected(false);
  });
  elements.refreshButton.addEventListener("click", () => loadDashboard().catch((error) => showError(elements.dashboardError, error.message)));
  elements.showCreateApp.addEventListener("click", () => { elements.createAppForm.classList.toggle("hidden"); elements.appName.focus(); });
  elements.showCreateRelease.addEventListener("click", () => { elements.createReleaseForm.classList.toggle("hidden"); elements.releaseVersion.focus(); });
  elements.copyInit.addEventListener("click", async () => {
    await navigator.clipboard.writeText(elements.initCommand.textContent);
    elements.copyInit.textContent = "Copied"; setTimeout(() => { elements.copyInit.textContent = "Copy"; }, 1200);
  });
  elements.createAppForm.addEventListener("submit", async (event) => {
    event.preventDefault(); showError(elements.dashboardError, "");
    try {
      const app = await api("/api/v1/apps", { method: "POST", body: JSON.stringify({ name: elements.appName.value.trim() }) });
      elements.appName.value = ""; elements.createAppForm.classList.add("hidden"); state.apps.unshift(app); renderApps(); await selectApp(app); await loadDashboard();
    } catch (error) { showError(elements.dashboardError, error.message); }
  });
  elements.createReleaseForm.addEventListener("submit", async (event) => {
    event.preventDefault(); if (!state.app) return; showError(elements.dashboardError, "");
    try {
      const release = await api(`/api/v1/apps/${state.app.id}/releases`, { method: "POST", body: JSON.stringify({ version: elements.releaseVersion.value.trim(), platform: "android", channel: elements.releaseChannel.value }) });
      elements.releaseVersion.value = ""; elements.createReleaseForm.classList.add("hidden"); state.releases.unshift(release); renderReleases(); await selectRelease(release); await loadDashboard();
    } catch (error) { showError(elements.dashboardError, error.message); }
  });

  setConnected(false);
  if (state.token) loadDashboard().catch(() => { sessionStorage.removeItem(tokenKey); state.token = ""; setConnected(false); });
})();
