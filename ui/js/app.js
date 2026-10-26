/* IdeaCraft – desktop shell: boot, tabs, navigation, sidebar, search, shortcuts */
import {
  S, $, $$, el, esc, invoke, hasTauri, pageIcon, pageTitle, pageAncestors,
  clamp, debounce, timeAgo, highlight, modKey,
} from './api.js';
import {
  toast, showMenu, showPopover, openModal, promptDialog, emojiPicker,
  spinner, hasLayers, closeAllMenus,
} from './ui.js';
import {
  renderHome, renderPage, renderLibrary, renderTemplates, renderTrash,
  openSettings, openAIDrawer, showSidebarPageMenu,
} from './views.js';
import { flushEditor, isEditorDirty, destroyEditor } from './editor.js';

const DEDICATED = new Set(['home', 'library', 'templates', 'trash']);
const TREE_KEY = 'ideacraft.treeExpanded';
const INVITE_KEY = 'ideacraft.inviteHidden';

const DEFAULT_SETTINGS = {
  theme: 'system',
  sidebarWidth: 275,
  sidebarCollapsed: false,
  font: 'default',
  smallText: false,
  fullWidth: false,
  keepInBackground: false,
  userName: 'You',
  userEmail: '',
  userAvatar: '',
  aiEnabled: false,
  aiBaseUrl: 'http://127.0.0.1:11434',
  aiModel: 'llama3.2',
  syncFolder: '',
  syncAuto: false,
  onboardingDone: false,
  tabsState: null,
  lastSync: '',
};

const appEl = $('#app');
const viewEl = $('#view');
const tabsEl = $('#tabs');
const pageTreeEl = $('#pageTree');
const favoritesEl = $('#sidebarFavorites');

let tabs = [];
let activeTab = 0;
let renderSeq = 0;
let currentRouteKey = '';
let lastAppearance = '';
let booted = false;
let autoSyncTimer = null;
let lastAutoSync = 0;
let syncBusy = false;
let dragPageId = null;
let searchState = null;
let appearanceTimer = null;
let chromeTimer = null;
let pendingTrayPage = false;
let pendingTraySettings = false;

const dirtyPages = new Set();
const expanded = loadExpanded();
const rowPages = new Map();
const closedTabs = [];

/* ---------------- small helpers ---------------- */
function errText(e) {
  return typeof e === 'string' ? e : e?.message || String(e ?? 'Something went wrong');
}

function ico(name) {
  return `<svg class="ic"><use href="#i-${name}"/></svg>`;
}

function initials(name) {
  const s = String(name || 'You').trim();
  return (s[0] || 'Y').toUpperCase();
}

function avatarMarkup(value, name) {
  const v = String(value || '');
  if (v.startsWith('data:') || /^https?:/i.test(v)) {
    return `<img src="${esc(v)}" alt="" />`;
  }
  return esc(v && v.length <= 8 ? v : initials(name));
}

function sidebarToggleTitle() {
  return S.sidebarCollapsed ? 'Show sidebar  Ctrl + \\' : 'Hide sidebar  Ctrl + \\';
}

function loadExpanded() {
  try {
    const raw = localStorage.getItem(TREE_KEY);
    const list = raw ? JSON.parse(raw) : [];
    return new Set(Array.isArray(list) ? list.filter(x => typeof x === 'string') : []);
  } catch {
    return new Set();
  }
}

function saveExpanded() {
  try { localStorage.setItem(TREE_KEY, JSON.stringify([...expanded])); } catch { /* ignore */ }
}

function inviteHidden() {
  try { return localStorage.getItem(INVITE_KEY) === '1'; } catch { return false; }
}

function setInviteHidden() {
  try { localStorage.setItem(INVITE_KEY, '1'); } catch { /* ignore */ }
}

function routesEqual(a, b) {
  if (!a || !b) return false;
  return a.type === b.type && (a.type !== 'page' || a.pageId === b.pageId);
}

function normalizeRoute(route) {
  if (!route || typeof route !== 'object') return null;
  if (route.type === 'page' && typeof route.pageId === 'string' && route.pageId) {
    return { type: 'page', pageId: route.pageId };
  }
  if (DEDICATED.has(route.type)) return { type: route.type };
  return null;
}

function routeKey(route) {
  if (!route) return '';
  return route.type === 'page' ? `page:${route.pageId}` : route.type;
}

function routeSummary(route) {
  if (!route || route.type !== 'page') return null;
  return S.pageMap.get(route.pageId) || rowPages.get(route.pageId) || null;
}

function routeLabel(route) {
  if (!route) return 'IdeaCraft';
  switch (route.type) {
    case 'page': return pageTitle(routeSummary(route));
    case 'home': return 'Home';
    case 'library': return 'Library';
    case 'templates': return 'Templates';
    case 'trash': return 'Trash';
    default: return 'IdeaCraft';
  }
}

function routeIcon(route) {
  if (!route) return ico('file');
  if (route.type === 'page') return pageIcon(routeSummary(route));
  const icons = { home: 'home', library: 'library', templates: 'grid', trash: 'trash' };
  return ico(icons[route.type] || 'file');
}

function pagePath(p) {
  const parents = pageAncestors(p.id).map(pageTitle);
  return parents.length ? parents.join(' / ') : 'Private';
}

function currentTab() {
  return tabs[activeTab] || null;
}

function currentRoute() {
  const tab = currentTab();
  return tab ? tab.history[tab.index] : null;
}

function activePageId() {
  const route = currentRoute();
  return route?.type === 'page' ? route.pageId : null;
}

function makeTab(route) {
  return { history: [route], index: 0 };
}

function defaultTab() {
  return makeTab({ type: 'home' });
}

/* ---------------- themes & chrome ---------------- */
const darkMedia = window.matchMedia ? window.matchMedia('(prefers-color-scheme: dark)') : null;

function resolvedTheme() {
  const pref = S.settings.theme || 'system';
  if (pref === 'dark' || pref === 'light') return pref;
  return darkMedia?.matches ? 'dark' : 'light';
}

function applyTheme() {
  const resolved = resolvedTheme();
  document.documentElement.dataset.theme = resolved;
  document.documentElement.style.colorScheme = resolved;
  appEl.dataset.theme = resolved;
}

function applySettings(initial = false) {
  S.sidebarWidth = clamp(Number(S.settings.sidebarWidth) || 275, 220, 460);
  S.sidebarCollapsed = !!S.settings.sidebarCollapsed;
  document.documentElement.style.setProperty('--sidebar-width', `${S.sidebarWidth}px`);
  appEl.classList.toggle('sidebar-collapsed', S.sidebarCollapsed);
  appEl.dataset.font = S.settings.font || 'default';
  appEl.dataset.smallText = S.settings.smallText ? 'true' : 'false';
  appEl.dataset.fullWidth = S.settings.fullWidth ? 'true' : 'false';
  applyTheme();

  const toggle = $('#sidebarToggle');
  if (toggle) {
    toggle.title = sidebarToggleTitle();
    toggle.setAttribute('aria-label', S.sidebarCollapsed ? 'Show sidebar' : 'Hide sidebar');
  }

  const appearance = `${S.settings.font || 'default'}|${!!S.settings.smallText}|${!!S.settings.fullWidth}`;
  if (initial || !lastAppearance) {
    lastAppearance = appearance;
  } else if (appearance !== lastAppearance) {
    lastAppearance = appearance;
    if (booted) {
      clearTimeout(appearanceTimer);
      appearanceTimer = setTimeout(() => {
        const route = currentRoute();
        if (booted && route?.type === 'page') renderActive({ keepDrawers: true });
      }, 100);
    }
  }

  if (S.settings.aiEnabled === false && S.aiOpen) closeAIDrawer();
}

function applyBranding() {
  const ws = S.boot?.workspace || { name: 'IdeaCraft', icon: 'I' };
  const name = ws.name || 'IdeaCraft';
  const icon = ws.icon && String(ws.icon).trim() ? ws.icon : 'I';

  const wsNameEl = $('#wsName');
  const wsAvatarEl = $('#wsAvatar');
  if (wsNameEl) wsNameEl.textContent = name;
  if (wsAvatarEl) {
    wsAvatarEl.innerHTML = avatarMarkup(icon, name);
    wsAvatarEl.title = name;
  }

  const userName = S.settings.userName || 'You';
  const accountName = $('#accountName');
  const accountAvatar = $('#accountAvatar');
  if (accountName) accountName.textContent = userName;
  if (accountAvatar) {
    accountAvatar.innerHTML = avatarMarkup(S.settings.userAvatar, userName);
    accountAvatar.title = userName;
  }

  const invite = $('#inviteCard');
  if (invite) invite.classList.toggle('hidden', inviteHidden());

  document.documentElement.dataset.platform = S.app?.platform || 'windows';
  updateDocumentTitle();
}

function updateDocumentTitle() {
  const route = currentRoute();
  const label = route ? routeLabel(route) : '';
  const ws = S.boot?.workspace?.name || 'IdeaCraft';
  document.title = label ? `${label} – ${ws}` : ws;
}

function updateNavButtons() {
  const tab = currentTab();
  const back = $('#navBack');
  const forward = $('#navForward');
  if (back) back.disabled = !tab || tab.index <= 0;
  if (forward) forward.disabled = !tab || tab.index >= tab.history.length - 1;
}

/* ---------------- data and tree ---------------- */
function rebuildPageMap() {
  S.pageMap = new Map((S.pages || []).map(p => [p.id, p]));
}

function insertSummary(page) {
  if (!page || !page.id) return;
  if (page.type === 'row') {
    rowPages.set(page.id, page);
    return;
  }
  if (page.isTemplate || page.trashedAt) return;
  let summary = S.pageMap.get(page.id);
  if (!summary) {
    summary = {
      id: page.id,
      parentId: page.parentId || null,
      title: page.title || '',
      icon: page.icon || null,
      cover: page.cover || null,
      coverType: page.coverType || 'none',
      type: page.type === 'database' ? 'database' : 'page',
      databaseId: page.databaseId || null,
      position: page.position || 0,
      isFavorite: !!page.isFavorite,
      isTemplate: false,
      updatedAt: page.updatedAt || new Date().toISOString(),
      trashedAt: null,
    };
    S.pages.push(summary);
    S.pageMap.set(summary.id, summary);
  } else {
    Object.assign(summary, {
      parentId: page.parentId ?? summary.parentId,
      title: page.title ?? summary.title,
      icon: page.icon ?? null,
      cover: page.cover ?? null,
      coverType: page.coverType || summary.coverType || 'none',
      databaseId: page.databaseId ?? summary.databaseId,
      position: page.position ?? summary.position,
      isFavorite: !!page.isFavorite,
      updatedAt: page.updatedAt || summary.updatedAt,
    });
    if (page.type && page.type !== 'row') summary.type = page.type;
  }
}

function scheduleChrome() {
  clearTimeout(chromeTimer);
  chromeTimer = setTimeout(() => {
    if (!S.boot) return;
    renderSidebar();
    paintTabs();
    updateDocumentTitle();
  }, 160);
}

function syncPageSummary(page) {
  if (!page || !page.id) return;
  insertSummary(page);
  scheduleChrome();
}

async function refreshTree(opts = {}) {
  try {
    const data = await invoke('get_sidebar');
    if (data) {
      if (data.workspace) {
        S.boot = S.boot || {};
        S.boot.workspace = data.workspace;
      }
      S.pages = data.pages || [];
      rebuildPageMap();
      applyBranding();
      renderSidebar();
      paintTabs();
      updateDocumentTitle();
    }
    return data;
  } catch (e) {
    if (!opts.silent) console.warn('Could not refresh the sidebar:', e);
    return null;
  }
}

async function reloadWorkspace() {
  const data = await invoke('bootstrap');
  S.boot = data;
  S.app = { ...(S.app || {}), ...(data.app || {}) };
  S.pages = data.pages || [];
  rebuildPageMap();
  S.settings = { ...DEFAULT_SETTINGS, ...(data.settings || {}) };
  applySettings(true);
  applyBranding();
  renderSidebar();
  paintTabs();
  await renderActive({ keepDrawers: true });
  schedulePersistTabs();
  return data;
}

/* ---------------- tabs ---------------- */
function setActiveTab(index, render = true) {
  if (index < 0 || index >= tabs.length) return;
  if (index === activeTab && render) {
    paintTabs();
    return;
  }
  activeTab = index;
  S.activeTab = index;
  paintTabs();
  updateNavButtons();
  paintNavActive();
  if (render) renderActive();
  schedulePersistTabs();
}

function addTab(route, activate = true) {
  tabs.push(makeTab(route));
  S.tabs = tabs;
  if (activate) {
    activeTab = tabs.length - 1;
    S.activeTab = activeTab;
  }
  paintTabs();
  updateNavButtons();
  if (activate) {
    paintNavActive();
    renderActive();
  }
  schedulePersistTabs();
}

function navigateCurrent(route, opts = {}) {
  if (!tabs.length) tabs.push(defaultTab());
  const tab = tabs[activeTab] || tabs[0];
  if (!tab) return;
  const previous = tab.history[tab.index];
  if (!opts.replace && routesEqual(previous, route)) {
    if (!opts.force) return;
  }
  if (opts.replace) {
    tab.history[tab.index] = route;
    tab.history = tab.history.slice(0, tab.index + 1);
  } else {
    tab.history = tab.history.slice(0, tab.index + 1);
    tab.history.push(route);
    tab.index = tab.history.length - 1;
    if (tab.history.length > 40) {
      const removed = tab.history.length - 40;
      tab.history = tab.history.slice(removed);
      tab.index = Math.max(0, tab.index - removed);
    }
  }
  paintTabs();
  updateNavButtons();
  paintNavActive();
  renderActive();
  schedulePersistTabs();
}

function openPage(pageId, opts = {}) {
  if (!pageId) return;
  expandPagePath(pageId);
  const route = { type: 'page', pageId };
  if (opts.newTab) addTab(route, true);
  else navigateCurrent(route, { replace: !!opts.replace });
}

function openRoute(route, opts = {}) {
  const normalized = normalizeRoute(route);
  if (!normalized) return;
  if (DEDICATED.has(normalized.type) && !opts.inPlace) {
    const existing = tabs.findIndex(t => routesEqual(t.history[t.index], normalized));
    if (existing >= 0) {
      setActiveTab(existing);
      return;
    }
    addTab(normalized, true);
    return;
  }
  navigateCurrent(normalized, { replace: !!opts.replace });
}

function goBack() {
  const tab = currentTab();
  if (!tab || tab.index <= 0) return;
  tab.index--;
  if (tab.history[tab.index]?.type === 'page') expandPagePath(tab.history[tab.index].pageId);
  paintTabs();
  updateNavButtons();
  renderActive();
  schedulePersistTabs();
}

function goForward() {
  const tab = currentTab();
  if (!tab || tab.index >= tab.history.length - 1) return;
  tab.index++;
  if (tab.history[tab.index]?.type === 'page') expandPagePath(tab.history[tab.index].pageId);
  paintTabs();
  updateNavButtons();
  renderActive();
  schedulePersistTabs();
}

function pushClosed(tab) {
  if (!tab) return;
  closedTabs.push({
    history: tab.history.map(r => ({ ...r })),
    index: tab.index,
  });
  if (closedTabs.length > 12) closedTabs.shift();
}

function closeTab(index, opts = {}) {
  if (index < 0 || index >= tabs.length) return;
  const wasActive = index === activeTab;
  if (opts.record !== false) pushClosed(tabs[index]);
  tabs.splice(index, 1);
  if (!tabs.length) tabs.push(defaultTab());
  if (wasActive) activeTab = clamp(index, 0, tabs.length - 1);
  else if (index < activeTab) activeTab--;
  activeTab = clamp(activeTab, 0, tabs.length - 1);
  S.tabs = tabs;
  S.activeTab = activeTab;
  paintTabs();
  updateNavButtons();
  paintNavActive();
  if (wasActive) renderActive();
  schedulePersistTabs();
}

function closeOtherTabs(index) {
  if (index < 0 || index >= tabs.length) return;
  const keep = tabs[index];
  tabs = [keep];
  activeTab = 0;
  S.tabs = tabs;
  S.activeTab = 0;
  paintTabs();
  updateNavButtons();
  paintNavActive();
  renderActive();
  schedulePersistTabs();
}

function duplicateTab(index) {
  const tab = tabs[index];
  if (!tab) return;
  addTab({ ...tab.history[tab.index] }, true);
}

function reopenClosedTab() {
  const state = closedTabs.pop();
  if (!state) return;
  const history = (state.history || []).map(normalizeRoute).filter(Boolean);
  if (!history.length) return;
  const tab = { history, index: clamp(Number(state.index) || 0, 0, history.length - 1) };
  tabs.push(tab);
  activeTab = tabs.length - 1;
  S.tabs = tabs;
  S.activeTab = activeTab;
  if (tab.history[tab.index]?.type === 'page') expandPagePath(tab.history[tab.index].pageId);
  paintTabs();
  updateNavButtons();
  renderActive();
  schedulePersistTabs();
}

function closePageTabs(ids) {
  const set = new Set(ids || []);
  if (!set.size) return;
  const activeObj = tabs[activeTab];
  const remaining = tabs.filter(t => {
    const route = t.history[t.index];
    return !(route?.type === 'page' && set.has(route.pageId));
  });
  if (remaining.length === tabs.length) return;
  tabs = remaining.length ? remaining : [defaultTab()];
  if (!tabs.includes(activeObj)) activeTab = clamp(activeTab, 0, tabs.length - 1);
  else activeTab = tabs.indexOf(activeObj);
  S.tabs = tabs;
  S.activeTab = activeTab;
  paintTabs();
  updateNavButtons();
  paintNavActive();
  renderActive();
  schedulePersistTabs();
}

function cycleTabs(delta) {
  if (tabs.length < 2) return;
  const next = (activeTab + delta + tabs.length) % tabs.length;
  setActiveTab(next);
}

function paintTabs() {
  if (!tabsEl) return;
  S.tabs = tabs;
  S.activeTab = activeTab;
  tabsEl.innerHTML = '';
  tabs.forEach((tab, index) => {
    const route = tab.history[tab.index];
    const active = index === activeTab;
    const dirty = route?.type === 'page' && dirtyPages.has(route.pageId);
    const title = routeLabel(route);
    const node = el('div', {
      class: `tab ${active ? 'active' : ''} ${dirty ? 'dirty' : ''}`,
      role: 'tab',
      tabindex: '0',
      'aria-selected': active ? 'true' : 'false',
      title,
      dataset: { index: String(index) },
    },
      el('span', { class: 'tab-icon', html: routeIcon(route) }),
      el('span', { class: 'tab-title', text: title }),
      el('button', {
        class: 'tab-close',
        tabindex: '-1',
        'aria-label': 'Close tab',
        html: '<svg class="ic"><use href="#i-close"/></svg>',
        onclick: (e) => { e.stopPropagation(); closeTab(index); },
      })
    );
    node.addEventListener('click', () => setActiveTab(index));
    node.addEventListener('auxclick', (e) => {
      if (e.button === 1) { e.preventDefault(); closeTab(index); }
    });
    node.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); setActiveTab(index); }
      if (e.key === 'Delete') { e.preventDefault(); closeTab(index); }
    });
    node.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      showTabMenu(index, e.currentTarget, { x: e.clientX, y: e.clientY });
    });
    tabsEl.appendChild(node);
  });
  updateDocumentTitle();
}

function showTabMenu(index, anchor, at = null) {
  if (index < 0 || index >= tabs.length) return;
  const opts = { align: 'left', width: 230 };
  if (at) { opts.x = at.x; opts.y = at.y; } else opts.anchor = anchor;
  showMenu({
    ...opts,
    items: [
      { icon: 'close', label: 'Close tab', hint: `${modKey()} W`, action: () => closeTab(index) },
      { label: 'Close other tabs', disabled: tabs.length <= 1, action: () => closeOtherTabs(index) },
      { type: 'sep' },
      { icon: 'duplicate', label: 'Duplicate tab', action: () => duplicateTab(index) },
      { type: 'sep' },
      { icon: 'history', label: 'Reopen closed tab', hint: `${modKey()} Shift T`, disabled: !closedTabs.length, action: () => reopenClosedTab() },
    ],
  });
}

function tabTitle(pageId, title, icon) {
  if (pageId) {
    const summary = S.pageMap.get(pageId) || rowPages.get(pageId);
    if (summary) {
      summary.title = title || '';
      if (icon !== undefined) summary.icon = icon || null;
    }
  }
  tabs.forEach((tab, index) => {
    const route = tab.history[tab.index];
    if (route?.type !== 'page' || route.pageId !== pageId) return;
    const node = tabsEl?.querySelector(`.tab[data-index="${index}"]`);
    if (!node) return;
    const titleEl = node.querySelector('.tab-title');
    if (titleEl) titleEl.textContent = pageTitle({ title });
    const iconEl = node.querySelector('.tab-icon');
    if (iconEl) iconEl.innerHTML = routeIcon(route);
    node.title = pageTitle({ title });
  });
  updateDocumentTitle();
}

function setTabDirty(dirty, pageId = null) {
  const id = pageId || activePageId();
  if (!id) return;
  if (dirty) dirtyPages.add(id);
  else dirtyPages.delete(id);
  S.dirtyCount = dirtyPages.size;
  tabs.forEach((tab, index) => {
    const route = tab.history[tab.index];
    if (route?.type !== 'page' || route.pageId !== id) return;
    const node = tabsEl?.querySelector(`.tab[data-index="${index}"]`);
    if (node) node.classList.toggle('dirty', !!dirty);
  });
}

/* ---------------- persistence ---------------- */
function tabsSnapshot() {
  return {
    tabs: tabs.slice(0, 20).map(t => ({
      history: t.history.map(route => {
        const normalized = normalizeRoute(route);
        return normalized || { type: 'home' };
      }),
      index: clamp(t.index, 0, Math.max(0, t.history.length - 1)),
    })),
    active: clamp(activeTab, 0, Math.max(0, tabs.length - 1)),
  };
}

const persistTabs = debounce(async () => {
  if (!booted) return;
  const value = tabsSnapshot();
  S.settings.tabsState = value;
  try { await invoke('set_setting', { key: 'tabsState', value }); } catch { /* not fatal */ }
}, 450);

function persistTabsNow() {
  persistTabs.cancel();
  if (!booted) return;
  const value = tabsSnapshot();
  S.settings.tabsState = value;
  invoke('set_setting', { key: 'tabsState', value }).catch(() => {});
}

function schedulePersistTabs() {
  if (booted) persistTabs();
}

function restoreTabs(validIds = null) {
  let state = S.settings.tabsState;
  if (typeof state === 'string') {
    try { state = JSON.parse(state); } catch { state = null; }
  }
  const isKnownPage = (id) => !validIds || validIds.has(id) || S.pageMap.has(id) || rowPages.has(id);
  const isValidRoute = (route) => !!route && (route.type !== 'page' || isKnownPage(route.pageId));
  const restored = [];
  let activeObj = null;
  const rawTabs = state && Array.isArray(state.tabs) ? state.tabs.slice(0, 20) : [];
  const activeRaw = clamp(Number(state?.active) || 0, 0, Math.max(0, rawTabs.length - 1));
  rawTabs.forEach((raw, rawTabIndex) => {
    const original = (raw?.history || []).map(normalizeRoute).filter(Boolean);
    if (!original.length) return;
    const start = clamp(Number(raw.index) || 0, 0, original.length - 1);
    let current = original[start];
    if (!isValidRoute(current)) {
      current = null;
      for (let d = 1; d < original.length; d++) {
        if (isValidRoute(original[start - d])) { current = original[start - d]; break; }
        if (isValidRoute(original[start + d])) { current = original[start + d]; break; }
      }
    }
    const history = original.filter(isValidRoute);
    if (!history.length) return;
    const compact = [];
    for (const route of history) {
      if (!compact.length || !routesEqual(compact[compact.length - 1], route)) compact.push(route);
    }
    let index = current ? compact.findIndex(route => routesEqual(route, current)) : -1;
    if (index < 0) index = clamp(start, 0, compact.length - 1);
    const tab = { history: compact, index };
    if (rawTabIndex === activeRaw) activeObj = tab;
    restored.push(tab);
  });
  if (!restored.length) restored.push(defaultTab());
  tabs = restored;
  activeTab = activeObj ? restored.indexOf(activeObj) : clamp(activeRaw, 0, tabs.length - 1);
  S.tabs = tabs;
  S.activeTab = activeTab;
  const route = tabs[activeTab]?.history[tabs[activeTab].index];
  if (route?.type === 'page') expandPagePath(route.pageId);
}

/* ---------------- sidebar ---------------- */
function sortPages(list) {
  return list.slice().sort((a, b) => {
    const pa = Number(a.position) || 0;
    const pb = Number(b.position) || 0;
    if (pa !== pb) return pa - pb;
    return pageTitle(a).localeCompare(pageTitle(b), undefined, { sensitivity: 'base' });
  });
}

function pageChildrenMap() {
  const map = new Map();
  for (const page of S.pages || []) {
    if (!page || page.isTemplate || page.type === 'row' || page.trashedAt) continue;
    const key = page.parentId || '__root__';
    if (!map.has(key)) map.set(key, []);
    map.get(key).push(page);
  }
  for (const [key, list] of map) map.set(key, sortPages(list));
  return map;
}

function expandPagePath(pageId) {
  let page = S.pageMap.get(pageId);
  const seen = new Set();
  let changed = false;
  while (page?.parentId && !seen.has(page.parentId)) {
    seen.add(page.parentId);
    if (!expanded.has(page.parentId)) { expanded.add(page.parentId); changed = true; }
    page = S.pageMap.get(page.parentId);
  }
  if (changed) saveExpanded();
  return changed;
}

function toggleExpanded(pageId) {
  if (expanded.has(pageId)) expanded.delete(pageId);
  else expanded.add(pageId);
  saveExpanded();
  renderSidebar();
}

function expandAllTree() {
  const children = pageChildrenMap();
  for (const [key, list] of children) {
    if (key !== '__root__' && list.length) expanded.add(key);
  }
  saveExpanded();
  renderSidebar();
}

function collapseAllTree() {
  const active = activePageId();
  expanded.clear();
  if (active) expandPagePath(active);
  saveExpanded();
  renderSidebar();
}

function buildTreeActions(page, favorite = false) {
  const actions = el('span', { class: 'tree-actions' });
  if (!favorite) {
    const add = el('button', {
      class: 'icon-btn tiny', title: 'Add a page inside', 'aria-label': 'Add a page inside', html: ico('plus'),
    });
    add.addEventListener('click', (e) => {
      e.stopPropagation();
      createPage(page.id, 'page');
    });
    actions.appendChild(add);
  }
  const more = el('button', {
    class: 'icon-btn tiny', title: 'More', 'aria-label': 'More', html: ico('more'),
  });
  more.addEventListener('click', (e) => {
    e.stopPropagation();
    showSidebarPageMenu(more, page);
  });
  actions.appendChild(more);
  return actions;
}

function buildPageRow(page, opts = {}) {
  const depth = opts.depth || 0;
  const favorite = !!opts.favorite;
  const active = activePageId() === page.id;
  const children = opts.childrenMap?.get(page.id) || [];
  const row = el('div', {
    class: `tree-row ${favorite ? 'fav-row' : ''} ${active ? 'active' : ''}`,
    draggable: favorite ? 'false' : 'true',
    role: 'button',
    tabindex: '0',
    dataset: { pageId: page.id },
    title: pageTitle(page),
  });
  if (!favorite) row.appendChild(el('span', { class: 'tree-indent', style: { width: `${depth * 12}px` } }));

  if (favorite) {
    row.appendChild(el('span', { class: 'tree-chevron fav-star', html: ico('star') }));
  } else if (children.length) {
    const chev = el('button', {
      class: `tree-chevron ${expanded.has(page.id) ? 'open' : ''}`,
      title: expanded.has(page.id) ? 'Collapse' : 'Expand',
      'aria-label': expanded.has(page.id) ? 'Collapse' : 'Expand',
      html: ico('chevron-right'),
    });
    chev.addEventListener('click', (e) => {
      e.stopPropagation();
      toggleExpanded(page.id);
    });
    row.appendChild(chev);
  } else {
    row.appendChild(el('span', { class: 'tree-chevron' }));
  }

  row.appendChild(el('span', { class: 'tree-icon', html: pageIcon(page) }));
  row.appendChild(el('span', { class: 'tree-title', text: pageTitle(page) }));
  row.appendChild(buildTreeActions(page, favorite));

  row.addEventListener('click', () => openPage(page.id));
  row.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); openPage(page.id); }
    if (e.key === 'ArrowRight' && children.length && !expanded.has(page.id)) {
      e.preventDefault(); toggleExpanded(page.id);
    }
    if (e.key === 'ArrowLeft' && children.length && expanded.has(page.id)) {
      e.preventDefault(); toggleExpanded(page.id);
    }
  });
  row.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    showSidebarPageMenu(null, page, { x: e.clientX, y: e.clientY });
  });
  if (!favorite) attachTreeDrag(row, page);
  return row;
}

function renderFavorites() {
  if (!favoritesEl) return;
  favoritesEl.innerHTML = '';
  const favorites = sortPages((S.pages || []).filter(p =>
    p.isFavorite && !p.isTemplate && p.type !== 'row' && !p.trashedAt
  ));
  if (!favorites.length) return;
  const section = el('div', { class: 'side-section favorites-section' },
    el('div', { class: 'side-section-head' }, el('span', { text: 'Favorites' }))
  );
  const tree = el('div', { class: 'page-tree' });
  for (const page of favorites) tree.appendChild(buildPageRow(page, { favorite: true }));
  section.appendChild(tree);
  favoritesEl.appendChild(section);
}

function renderTree() {
  if (!pageTreeEl) return;
  pageTreeEl.innerHTML = '';
  if (!S.pages?.length) {
    pageTreeEl.appendChild(el('div', { class: 'sidebar-empty' },
      el('div', { text: 'No pages yet' }),
      el('button', { class: 'sidebar-empty-btn', text: 'Create a page', onclick: () => createPage(null, 'page') })
    ));
    return;
  }
  const childrenMap = pageChildrenMap();
  const roots = childrenMap.get('__root__') || [];
  const seen = new Set();
  const renderNodes = (parent, depth) => {
    const list = childrenMap.get(parent || '__root__') || [];
    for (const page of list) {
      if (seen.has(page.id) || depth > 18) continue;
      seen.add(page.id);
      pageTreeEl.appendChild(buildPageRow(page, { depth, childrenMap }));
      if (expanded.has(page.id)) renderNodes(page.id, depth + 1);
    }
  };
  renderNodes(null, 0);
  if (!roots.length) {
    pageTreeEl.appendChild(el('div', { class: 'sidebar-empty', text: 'No pages yet' }));
  }
}

function renderSidebar() {
  renderFavorites();
  renderTree();
  paintNavActive();
}

function paintNavActive() {
  const route = currentRoute();
  const pageId = route?.type === 'page' ? route.pageId : null;
  const map = {
    home: route?.type === 'home',
    library: route?.type === 'library',
    templates: route?.type === 'templates',
    trash: route?.type === 'trash',
    ai: !!S.aiOpen,
  };
  $$('.side-item[data-nav]').forEach(btn => {
    const key = btn.dataset.nav;
    const active = !!map[key];
    btn.classList.toggle('active', active);
    if (active) btn.setAttribute('aria-current', 'page');
    else btn.removeAttribute('aria-current');
  });
  $$('.tree-row[data-page-id]').forEach(row => {
    const active = row.dataset.pageId === pageId;
    row.classList.toggle('active', active);
    if (active) row.setAttribute('aria-current', 'page');
    else row.removeAttribute('aria-current');
  });
}

/* ---------------- drag & drop ---------------- */
function isDescendantOf(candidateId, ancestorId) {
  let page = S.pageMap.get(candidateId);
  const seen = new Set();
  while (page?.parentId) {
    if (page.parentId === ancestorId) return true;
    if (seen.has(page.parentId)) return false;
    seen.add(page.parentId);
    page = S.pageMap.get(page.parentId);
  }
  return false;
}

function clearDragMarks() {
  $$('.tree-row').forEach(row => row.classList.remove('drag-over', 'drop-before', 'drop-after', 'dragging'));
  pageTreeEl?.classList.remove('drag-root');
}

function attachTreeDrag(row, page) {
  row.addEventListener('dragstart', (e) => {
    if (e.target.closest('.tree-actions')) { e.preventDefault(); return; }
    dragPageId = page.id;
    e.dataTransfer.effectAllowed = 'move';
    e.dataTransfer.setData('text/plain', page.id);
    requestAnimationFrame(() => row.classList.add('dragging'));
  });
  row.addEventListener('dragend', () => {
    dragPageId = null;
    clearDragMarks();
  });
  row.addEventListener('dragover', (e) => {
    if (!dragPageId || dragPageId === page.id || isDescendantOf(page.id, dragPageId)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    const rect = row.getBoundingClientRect();
    const ratio = (e.clientY - rect.top) / Math.max(1, rect.height);
    const mode = ratio < 0.28 ? 'before' : ratio > 0.72 ? 'after' : 'inside';
    row.dataset.dropMode = mode;
    row.classList.toggle('drop-before', mode === 'before');
    row.classList.toggle('drop-after', mode === 'after');
    row.classList.toggle('drag-over', mode === 'inside');
  });
  row.addEventListener('dragleave', (e) => {
    if (row.contains(e.relatedTarget)) return;
    row.classList.remove('drag-over', 'drop-before', 'drop-after');
  });
  row.addEventListener('drop', async (e) => {
    if (!dragPageId) return;
    e.preventDefault();
    e.stopPropagation();
    const id = dragPageId;
    dragPageId = null;
    const mode = row.dataset.dropMode || 'inside';
    row.classList.remove('drop-before', 'drop-after', 'drag-over');
    if (id === page.id || isDescendantOf(page.id, id)) return;
    if (mode === 'inside') {
      expanded.add(page.id);
      saveExpanded();
      await movePageTo(id, page.id, null);
    } else {
      const position = (Number(page.position) || 0) + (mode === 'before' ? -0.25 : 0.25);
      await movePageTo(id, page.parentId || null, position);
    }
  });
}

async function movePageTo(id, parentId, position) {
  if (!id) return;
  if (parentId && (parentId === id || isDescendantOf(parentId, id))) {
    toast('A page cannot be moved inside itself', 'error');
    return;
  }
  try {
    await invoke('move_page', { id, parentId, position });
    if (parentId) {
      expanded.add(parentId);
      saveExpanded();
    }
    await refreshTree();
    toast('Page moved', 'success', 1300);
  } catch (e) {
    toast(`Could not move page: ${errText(e)}`, 'error');
  } finally {
    clearDragMarks();
  }
}

function initTreeDropZone() {
  if (pageTreeEl) {
    pageTreeEl.addEventListener('dragover', (e) => {
      if (!dragPageId || e.target.closest('.tree-row')) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = 'move';
      pageTreeEl.classList.add('drag-root');
    });
    pageTreeEl.addEventListener('dragleave', (e) => {
      if (pageTreeEl.contains(e.relatedTarget)) return;
      pageTreeEl.classList.remove('drag-root');
    });
    pageTreeEl.addEventListener('drop', async (e) => {
      if (!dragPageId || e.target.closest('.tree-row')) return;
      e.preventDefault();
      const id = dragPageId;
      dragPageId = null;
      await movePageTo(id, null, null);
    });
  }
  const privateHead = $('#sidebarPrivate .side-section-head');
  if (privateHead) {
    privateHead.addEventListener('dragover', (e) => {
      if (!dragPageId) return;
      e.preventDefault();
      privateHead.classList.add('drag-over');
    });
    privateHead.addEventListener('dragleave', () => privateHead.classList.remove('drag-over'));
    privateHead.addEventListener('drop', async (e) => {
      if (!dragPageId) return;
      e.preventDefault();
      e.stopPropagation();
      privateHead.classList.remove('drag-over');
      const id = dragPageId;
      dragPageId = null;
      await movePageTo(id, null, null);
    });
  }
}

/* ---------------- page creation & imports ---------------- */
async function createPage(parentId = null, pageType = 'page', opts = {}) {
  try {
    if (parentId) {
      expanded.add(parentId);
      saveExpanded();
    }
    const detail = await invoke('create_page', { parentId, title: '', pageType });
    await refreshTree({ silent: true });
    if (detail?.page?.id) openPage(detail.page.id, { newTab: !!opts.newTab });
    return detail;
  } catch (e) {
    toast(`Could not create page: ${errText(e)}`, 'error');
    return null;
  }
}

async function importMarkdown(parentId = null) {
  try {
    const detail = await invoke('import_markdown', { parentId });
    if (!detail) return null;
    await refreshTree({ silent: true });
    if (detail?.page?.id) openPage(detail.page.id);
    toast('Page imported', 'success');
    return detail;
  } catch (e) {
    toast(`Import failed: ${errText(e)}`, 'error');
    return null;
  }
}

/* ---------------- rendering ---------------- */
function showSplash(text = 'Loading your workspace…') {
  if (!viewEl) return;
  viewEl.innerHTML = '';
  viewEl.appendChild(el('div', { class: 'boot-loading' },
    el('div', { class: 'boot-logo', text: 'I' }),
    el('div', { class: 'boot-title', text: 'IdeaCraft' }),
    el('div', { class: 'boot-status' },
      el('span', { class: 'boot-spin', html: spinner(15) }),
      el('span', { text })
    )
  ));
}

function showBootError(e) {
  if (!viewEl) return;
  viewEl.innerHTML = '';
  const message = hasTauri
    ? 'The workspace database could not be loaded. Try again, or make sure the app has permission to use its data folder.'
    : 'The native backend is not available in this window. Run IdeaCraft through IdeaCraft.exe.';
  viewEl.appendChild(el('div', { class: 'boot-error' },
    el('div', { class: 'be-mark', text: 'I' }),
    el('h1', { text: 'IdeaCraft could not start' }),
    el('p', { text: message }),
    el('pre', { class: 'be-detail', text: errText(e) }),
    el('div', { class: 'be-actions' },
      el('button', { class: 'btn primary', text: 'Try again', onclick: () => location.reload() }),
      hasTauri ? el('button', {
        class: 'btn', text: 'Open data folder',
        onclick: () => invoke('open_data_folder').catch(() => toast('Could not open the data folder', 'error')),
      }) : null
    )
  ));
}

async function renderActive(opts = {}) {
  if (!viewEl) return;
  if (!tabs.length) tabs.push(defaultTab());
  activeTab = clamp(activeTab, 0, tabs.length - 1);
  const tab = tabs[activeTab];
  if (!tab) return;
  const route = tab.history[tab.index];
  if (!route) return;

  const key = routeKey(route);
  const changed = key !== currentRouteKey;
  currentRouteKey = key;
  if (changed && !opts.keepDrawers) closeAllDrawers();

  S.activeTab = activeTab;
  S.tabs = tabs;
  S.view = route;
  S.currentPageId = route.type === 'page' ? route.pageId : null;
  if (route.type === 'page') expandPagePath(route.pageId);

  const seq = ++renderSeq;
  if (isEditorDirty()) {
    try { await flushEditor(true); } catch { /* keep navigating */ }
  }
  if (seq !== renderSeq) return;
  destroyEditor();

  paintTabs();
  updateNavButtons();
  paintNavActive();
  updateDocumentTitle();

  try {
    viewEl.innerHTML = '';
    if (route.type === 'page') {
      await renderPage(viewEl, route.pageId);
    } else if (route.type === 'home') {
      await renderHome(viewEl);
    } else if (route.type === 'library') {
      renderLibrary(viewEl);
    } else if (route.type === 'templates') {
      await renderTemplates(viewEl);
    } else if (route.type === 'trash') {
      await renderTrash(viewEl);
    }
  } catch (e) {
    if (seq !== renderSeq) return;
    console.error('Could not render view:', e);
    viewEl.innerHTML = '';
    viewEl.appendChild(el('div', { class: 'view-error' },
      el('div', { class: 've-icon', text: '⚠️' }),
      el('h2', { text: 'This view could not be shown' }),
      el('p', { text: errText(e) }),
      el('button', { class: 'btn', text: 'Try again', onclick: () => renderActive({ keepDrawers: true }) })
    ));
  }

  if (seq !== renderSeq) return;
  paintTabs();
  paintNavActive();
  updateDocumentTitle();
  schedulePersistTabs();
}

function onPageShown(detail) {
  const page = detail?.page;
  if (!page) return;
  S.currentPageId = page.id;
  insertSummary(page);
  if (page.type === 'row') rowPages.set(page.id, page);
  scheduleChrome();
  paintTabs();
  updateDocumentTitle();
  invoke('visit_page', { id: page.id }).catch(() => {});
}

function reloadPage(pageId) {
  const route = currentRoute();
  if (route?.type === 'page' && route.pageId === pageId) {
    renderActive({ keepDrawers: true });
  }
}

function closeAIDrawer() {
  const root = $('.ai-drawer');
  if (!root) { S.aiOpen = false; paintNavActive(); return; }
  const btn = root.querySelector('.drawer-head > .icon-btn.tiny:last-child');
  if (btn) btn.click();
  else { root.remove(); S.aiOpen = false; paintNavActive(); }
}

function closeCommentsDrawer() {
  const root = $('.comments-drawer');
  if (!root) { S.commentsOpen = false; return; }
  const btn = root.querySelector('.drawer-head > .icon-btn.tiny:last-child');
  if (btn) btn.click();
  else { root.remove(); S.commentsOpen = false; }
}

function closeAllDrawers() {
  if (S.aiOpen) closeAIDrawer();
  if (S.commentsOpen) closeCommentsDrawer();
}

function toggleAI() {
  if (S.aiOpen) closeAIDrawer();
  else openAIDrawer();
}

/* ---------------- settings helpers ---------------- */
async function setSetting(key, value, opts = {}) {
  const previous = S.settings[key];
  S.settings[key] = value;
  applySettings();
  try {
    await invoke('set_setting', { key, value });
    return true;
  } catch (e) {
    S.settings[key] = previous;
    applySettings();
    if (!opts.silent) toast(`Could not save setting: ${errText(e)}`, 'error');
    return false;
  }
}

function toggleSidebar() {
  S.sidebarCollapsed = !appEl.classList.contains('sidebar-collapsed');
  appEl.classList.toggle('sidebar-collapsed', S.sidebarCollapsed);
  applySettings();
  setSetting('sidebarCollapsed', S.sidebarCollapsed, { silent: true });
}

/* ---------------- search palette ---------------- */
function openSearch() {
  if (searchState) {
    searchState.input?.focus();
    searchState.input?.select();
    return;
  }
  if (!S.boot) return;
  closeAllMenus();

  const overlay = el('div', { class: 'search-overlay' });
  const panel = el('div', { class: 'search-panel', role: 'dialog', 'aria-modal': 'true', 'aria-label': 'Search' });
  const input = el('input', {
    type: 'text', placeholder: 'Search pages, databases, and content…',
    spellcheck: 'false', 'aria-label': 'Search your workspace', autocomplete: 'off',
  });
  const inputRow = el('div', { class: 'search-input-row' },
    el('span', { html: ico('search') }),
    input,
    el('kbd', { text: 'Esc' })
  );
  const results = el('div', { class: 'search-results' });
  const foot = el('div', { class: 'search-foot' },
    el('span', {}, el('kbd', { text: '↑↓' }), ' navigate'),
    el('span', {}, el('kbd', { text: '↵' }), ' open'),
    el('span', {}, el('kbd', { text: `${modKey()} K` }), ' close')
  );
  panel.append(inputRow, results, foot);
  overlay.appendChild(panel);
  document.body.appendChild(overlay);

  let items = [];
  let activeIndex = 0;
  let requestSeq = 0;
  let searchTimer = null;
  let pageResults = [];
  let error = null;
  let loading = false;

  const state = {
    overlay, panel, results, input,
    close,
    get items() { return items; },
  };
  searchState = state;
  S.searchOpen = true;

  const actionList = () => [
    { label: 'New page', icon: 'file', hint: `${modKey()} N`, run: () => createPage(null, 'page', { newTab: true }) },
    { label: 'New database', icon: 'database', run: () => createPage(null, 'database', { newTab: true }) },
    { label: 'Open Settings', icon: 'settings', hint: `${modKey()} ,`, run: () => openSettings('account') },
    { label: 'Browse Templates', icon: 'grid', run: () => openRoute({ type: 'templates' }) },
    { label: 'Open Trash', icon: 'trash', run: () => openRoute({ type: 'trash' }) },
    { label: 'Open IdeaCraft AI', icon: 'sparkles', hint: `${modKey()} J`, run: () => openAIDrawer() },
    { label: 'Go Home', icon: 'home', run: () => openRoute({ type: 'home' }) },
    { label: 'Toggle sidebar', icon: 'sidebar', hint: `${modKey()} \\`, run: () => toggleSidebar() },
  ];

  function matchingActions(q) {
    const list = actionList();
    if (!q) return list.slice(0, 5);
    return list.filter(a => a.label.toLowerCase().includes(q.toLowerCase()));
  }

  function setActive(index) {
    if (!items.length) { activeIndex = -1; return; }
    activeIndex = (index + items.length) % items.length;
    items.forEach((item, i) => item.node.classList.toggle('active', i === activeIndex));
    items[activeIndex]?.node.scrollIntoView({ block: 'nearest' });
  }

  function makeResultRow(opts) {
    const node = el('div', { class: 'search-result', role: 'button' },
      el('span', { class: 'sr-icon', html: opts.iconHtml }),
      el('span', { class: 'sr-main' },
        el('span', { class: 'sr-title', html: opts.titleHtml }),
        opts.sub ? el('span', { class: 'sr-sub', html: opts.subHtml || esc(opts.sub) }) : null
      ),
      opts.type ? el('span', { class: 'sr-type', text: opts.type }) : null
    );
    node.addEventListener('mousedown', e => e.preventDefault());
    node.addEventListener('click', () => runItem({ run: opts.run }));
    node.addEventListener('dblclick', () => runItem({ run: opts.run }));
    return node;
  }

  function runItem(item) {
    if (!item) return;
    close();
    try { item.run(); } catch (e) { toast(errText(e), 'error'); }
  }

  function render() {
    results.innerHTML = '';
    items = [];
    activeIndex = 0;
    const q = input.value.trim();

    if (pageResults.length) {
      results.appendChild(el('div', { class: 'search-group-label', text: q ? 'Pages' : 'Recent pages' }));
      for (const result of pageResults) {
        const page = result.page;
        const path = pagePath(page);
        const snippet = result.snippet && result.matched === 'content'
          ? highlight(result.snippet, q)
          : highlight(path, q);
        const node = makeResultRow({
          iconHtml: pageIcon(page),
          titleHtml: highlight(pageTitle(page), q),
          sub: snippet,
          subHtml: snippet,
          type: page.type === 'database' ? 'Database' : 'Page',
          run: () => openPage(page.id),
        });
        results.appendChild(node);
        items.push({ node, run: () => openPage(page.id) });
      }
    }

    const actions = matchingActions(q);
    if (actions.length) {
      results.appendChild(el('div', { class: 'search-group-label', text: 'Actions' }));
      for (const action of actions) {
        const node = makeResultRow({
          iconHtml: ico(action.icon || 'plus'),
          titleHtml: highlight(action.label, q),
          sub: action.hint || '',
          type: '',
          run: action.run,
        });
        results.appendChild(node);
        items.push({ node, run: action.run });
      }
    }

    if (loading) {
      const loadingNode = el('div', { class: 'search-loading' }, el('span', { html: spinner(14) }), el('span', { text: 'Searching…' }));
      results.prepend(loadingNode);
    } else if (error) {
      results.prepend(el('div', { class: 'search-empty' }, el('div', { text: 'Search is unavailable right now.' }), el('div', { class: 'search-error', text: errText(error) })));
    } else if (!pageResults.length && !actions.length) {
      results.appendChild(el('div', { class: 'search-empty' },
        el('div', { class: 'search-empty-icon', text: '🔎' }),
        el('div', { text: q ? `No results for “${q}”` : 'Nothing here yet' }),
        el('div', { class: 'search-empty-sub', text: q ? 'Try a different search or create a new page.' : 'Create a page to start searching.' })
      ));
    }
    setActive(items.length ? 0 : -1);
  }

  async function doSearch() {
    const q = input.value.trim();
    const token = ++requestSeq;
    loading = true;
    error = null;
    render();
    try {
      const list = await invoke('search', { query: q });
      if (token !== requestSeq || searchState !== state) return;
      let pages = Array.isArray(list) ? list : [];
      if (!q && !pages.length) {
        pages = sortPages((S.pages || []).filter(p => !p.isTemplate && p.type !== 'row' && !p.trashedAt))
          .slice(0, 10)
          .map(p => ({ page: p, snippet: `Edited ${timeAgo(p.updatedAt)}`, matched: 'recent' }));
      }
      pageResults = pages;
    } catch (e) {
      if (token !== requestSeq || searchState !== state) return;
      pageResults = [];
      error = e;
    } finally {
      if (token === requestSeq && searchState === state) {
        loading = false;
        render();
      }
    }
  }

  function scheduleSearch() {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(doSearch, 110);
  }

  function close() {
    if (searchState !== state) return;
    clearTimeout(searchTimer);
    requestSeq++;
    document.removeEventListener('keydown', onDocKey, true);
    overlay.remove();
    searchState = null;
    S.searchOpen = false;
  }

  function onDocKey(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close();
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      e.stopPropagation();
      close();
    }
  }

  overlay.addEventListener('mousedown', (e) => {
    if (e.target === overlay) close();
  });
  input.addEventListener('input', scheduleSearch);
  input.addEventListener('keydown', (e) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); setActive(activeIndex + 1); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); setActive(activeIndex - 1); }
    else if (e.key === 'Enter') {
      e.preventDefault();
      runItem(items[activeIndex]);
    }
  });
  document.addEventListener('keydown', onDocKey, true);
  render();
  doSearch();
  setTimeout(() => input.focus(), 20);
}

/* ---------------- informational popovers ---------------- */
function showInbox(anchor) {
  const node = el('div', { class: 'inbox-pop' });
  const head = el('div', { class: 'inbox-head' },
    el('div', { class: 'inbox-head-title', text: 'Inbox' }),
    el('span', { class: 'badge-soft', text: 'Local' })
  );
  node.appendChild(head);
  node.appendChild(el('p', { class: 'inbox-sub', text: 'IdeaCraft does not send notifications. Here is what changed recently on this device.' }));
  const recent = sortPages((S.pages || []).filter(p => !p.isTemplate && p.type !== 'row' && !p.trashedAt))
    .sort((a, b) => new Date(b.updatedAt || 0) - new Date(a.updatedAt || 0))
    .slice(0, 6);
  if (recent.length) {
    const list = el('div', { class: 'inbox-list' });
    for (const page of recent) {
      const row = el('button', { class: 'inbox-item' },
        el('span', { class: 'inbox-icon', html: pageIcon(page) }),
        el('span', { class: 'inbox-main' },
          el('span', { class: 'inbox-title', text: pageTitle(page) }),
          el('span', { class: 'inbox-meta', text: `Edited ${timeAgo(page.updatedAt)}` })
        )
      );
      row.addEventListener('click', () => { pop.close(); openPage(page.id); });
      list.appendChild(row);
    }
    node.appendChild(list);
  } else {
    node.appendChild(el('div', { class: 'inbox-empty', text: 'No recent activity yet.' }));
  }
  const foot = el('button', { class: 'inbox-foot', text: 'Browse all pages' });
  foot.addEventListener('click', () => { pop.close(); openRoute({ type: 'library' }); });
  node.appendChild(foot);
  const pop = showPopover({ node, anchor, align: 'left', className: 'inbox-pop', width: 344 });
  return pop;
}

function openLocalShareInfo() {
  const body = el('div', { class: 'local-share' },
    el('div', { class: 'local-share-hero' },
      el('div', { class: 'local-share-icon', html: ico('user') }),
      el('h3', { text: 'A workspace of your own' }),
      el('p', { text: 'IdeaCraft is local-first, so there are no accounts to invite and no cloud invitations. Sharing means exporting a copy or syncing a folder you already trust.' })
    ),
    el('div', { class: 'local-share-options' },
      el('div', { class: 'local-share-option' },
        el('span', { class: 'lso-icon', html: ico('download') }),
        el('span', {}, el('b', { text: 'Export a page' }), el('span', { text: 'Open a page and use Share → Export to send a Markdown or HTML file.' }))
      ),
      el('div', { class: 'local-share-option' },
        el('span', { class: 'lso-icon', html: ico('cloud') }),
        el('span', {}, el('b', { text: 'Sync through a folder' }), el('span', { text: 'Choose OneDrive, Dropbox, iCloud Drive, or any folder you control in Settings → Sync.' }))
      ),
      el('div', { class: 'local-share-option' },
        el('span', { class: 'lso-icon', html: ico('database') }),
        el('span', {}, el('b', { text: 'Back up everything' }), el('span', { text: 'Settings → Data exports every page, block, comment, and database as one file.' }))
      )
    )
  );
  let modal;
  const syncBtn = el('button', { class: 'btn primary', html: `${ico('cloud')}<span>Sync settings</span>` });
  syncBtn.addEventListener('click', () => { modal.close(); openSettings('sync'); });
  const backupBtn = el('button', { class: 'btn', html: `${ico('download-cloud')}<span>Export backup</span>` });
  backupBtn.addEventListener('click', async () => {
    try {
      const path = await invoke('export_backup');
      if (path) toast(`Backup saved to ${path}`, 'success', 4200);
    } catch (e) { toast(`Export failed: ${errText(e)}`, 'error'); }
  });
  modal = openModal({
    title: 'Invite members',
    width: 540,
    body,
    footer: el('div', { style: { display: 'flex', gap: '8px', width: '100%', justifyContent: 'flex-end' } },
      el('button', { class: 'btn', text: 'Done', onclick: () => modal.close() }),
      backupBtn,
      syncBtn
    ),
  });
  return modal;
}

/* ---------------- menus ---------------- */
async function renameWorkspace() {
  const name = await promptDialog({
    title: 'Rename workspace',
    label: 'Workspace name',
    value: S.boot?.workspace?.name || 'IdeaCraft',
    confirmText: 'Rename',
  });
  if (!name) return;
  try {
    const ws = await invoke('save_workspace', { name, icon: S.boot?.workspace?.icon || null });
    S.boot.workspace = ws;
    applyBranding();
    renderSidebar();
    toast('Workspace renamed', 'success');
  } catch (e) {
    toast(`Could not rename workspace: ${errText(e)}`, 'error');
  }
}

function chooseWorkspaceIcon(anchor) {
  emojiPicker(anchor, async (emoji) => {
    if (!emoji) return;
    try {
      const ws = await invoke('save_workspace', { name: S.boot?.workspace?.name || 'IdeaCraft', icon: emoji });
      S.boot.workspace = ws;
      applyBranding();
      renderSidebar();
    } catch (e) {
      toast(`Could not update workspace icon: ${errText(e)}`, 'error');
    }
  }, { allowRemove: false });
}

function showWorkspaceMenu(anchor) {
  showMenu({
    anchor, align: 'left', width: 276, header: S.boot?.workspace?.name || 'IdeaCraft',
    items: [
      { icon: 'edit', label: 'New page', action: () => createPage(null, 'page') },
      { icon: 'database', label: 'New database', action: () => createPage(null, 'database') },
      { type: 'sep' },
      { icon: 'settings', label: 'Settings', action: () => openSettings('account') },
      { icon: 'grid', label: 'Templates', action: () => openRoute({ type: 'templates' }) },
      { icon: 'trash', label: 'Trash', action: () => openRoute({ type: 'trash' }) },
      { type: 'sep' },
      { icon: 'edit', label: 'Rename workspace…', action: () => renameWorkspace() },
      { icon: 'smile', label: 'Change workspace icon…', action: () => chooseWorkspaceIcon(anchor) },
      { icon: 'user', label: 'Invite members…', action: () => openLocalShareInfo() },
      { type: 'sep' },
      { icon: 'info', label: 'About IdeaCraft', action: () => openSettings('about') },
    ],
  });
}

function showAccountMenu(anchor) {
  const identity = el('div', { class: 'account-menu-head' },
    el('span', { class: 'account-menu-avatar', html: avatarMarkup(S.settings.userAvatar, S.settings.userName) }),
    el('span', { class: 'account-menu-text' },
      el('span', { class: 'account-menu-name', text: S.settings.userName || 'You' }),
      el('span', { class: 'account-menu-mail', text: S.settings.userEmail || 'Local account' })
    )
  );
  const theme = S.settings.theme || 'system';
  showMenu({
    anchor, align: 'left', vAlign: 'above', width: 280,
    items: [
      { type: 'custom', node: identity },
      { type: 'sep' },
      { icon: 'settings', label: 'Settings', action: () => openSettings('account') },
      { icon: 'bell', label: 'Notifications', action: () => openSettings('notifications') },
      { icon: 'keyboard', label: 'Keyboard shortcuts', action: () => openSettings('shortcuts') },
      { type: 'sep' },
      { type: 'label', label: 'Theme' },
      { checked: theme === 'light', label: 'Light', action: () => setSetting('theme', 'light') },
      { checked: theme === 'dark', label: 'Dark', action: () => setSetting('theme', 'dark') },
      { checked: theme === 'system', label: 'Use system setting', action: () => setSetting('theme', 'system') },
      { type: 'sep' },
      { icon: 'cloud', label: 'Sync now', disabled: !S.settings.syncFolder, action: () => runSync(true) },
      { icon: 'info', label: 'About IdeaCraft', action: () => openSettings('about') },
    ],
  });
}

function showPrivateMore(anchor) {
  showMenu({
    anchor, align: 'right', width: 240,
    items: [
      { icon: 'edit', label: 'New page', action: () => createPage(null, 'page') },
      { icon: 'database', label: 'New database', action: () => createPage(null, 'database') },
      { type: 'sep' },
      { icon: 'upload', label: 'Import Markdown…', action: () => importMarkdown(null) },
      { icon: 'grid', label: 'Templates', action: () => openRoute({ type: 'templates' }) },
      { type: 'sep' },
      { icon: 'chevron-down', label: 'Expand all', action: () => expandAllTree() },
      { icon: 'chevron-right', label: 'Collapse all', action: () => collapseAllTree() },
      { type: 'sep' },
      { icon: 'trash', label: 'Open Trash', action: () => openRoute({ type: 'trash' }) },
    ],
  });
}

/* ---------------- sync ---------------- */
async function runSync(manual = true, quiet = false) {
  if (syncBusy) return;
  if (!S.settings.syncFolder) {
    if (!quiet) toast('Choose a sync folder in Settings → Sync', 'error');
    return;
  }
  syncBusy = true;
  try {
    const res = await invoke('sync_now');
    S.settings.lastSync = res?.time || new Date().toISOString();
    if (res?.imported) await refreshTree({ silent: true });
    if (manual) toast(`Sync complete${res?.imported ? ` · ${res.imported} merged` : ''}`, 'success', 3200);
    else if (res?.imported && !quiet) toast('Synced changes from your folder', 'success', 2600);
  } catch (e) {
    if (manual) toast(`Sync failed: ${errText(e)}`, 'error', 4200);
    else console.warn('Background sync failed:', e);
  } finally {
    syncBusy = false;
  }
}

function startAutoSync() {
  if (autoSyncTimer) clearInterval(autoSyncTimer);
  autoSyncTimer = setInterval(() => {
    if (!booted || !S.settings.syncAuto || !S.settings.syncFolder || document.hidden) return;
    if (Date.now() - lastAutoSync < 4 * 60 * 1000) return;
    lastAutoSync = Date.now();
    runSync(false, true);
  }, 60 * 1000);
}

/* ---------------- window controls ---------------- */
async function windowAction(action) {
  if (!hasTauri) {
    toast('Window controls are available in the desktop app', 'info');
    return;
  }
  try {
    await invoke('window_action', { action });
    if (action === 'maximize' || action === 'fullscreen') setTimeout(syncWindowState, 140);
  } catch (e) {
    toast(`Window action failed: ${errText(e)}`, 'error');
  }
}

async function syncWindowState() {
  if (!hasTauri) return;
  try {
    const state = await invoke('window_state');
    const btn = $('#winMax');
    if (!btn) return;
    if (state.fullscreen) {
      btn.title = 'Exit full screen';
      btn.innerHTML = ico('collapse');
    } else if (state.maximized) {
      btn.title = 'Restore';
      btn.innerHTML = ico('restore');
    } else {
      btn.title = 'Maximize';
      btn.innerHTML = ico('maximize');
    }
  } catch { /* window may be closing */ }
}

async function closeWindow() {
  try {
    if (isEditorDirty()) await flushEditor(true);
  } catch { /* continue closing */ }
  persistTabsNow();
  if (!hasTauri) return;
  try { await invoke('window_action', { action: 'close' }); }
  catch (e) { toast(`Could not close the window: ${errText(e)}`, 'error'); }
}

/* ---------------- global keyboard ---------------- */
function isComposing(e) {
  return e.isComposing || e.keyCode === 229;
}

function handleGlobalKey(e) {
  if (isComposing(e)) return;
  const mod = e.ctrlKey || e.metaKey;
  const key = String(e.key || '').toLowerCase();

  if (searchState) {
    // The search palette owns Escape and Ctrl+K while it is open.
    return;
  }

  if (mod && key === 'k' && !e.altKey) {
    e.preventDefault();
    openSearch();
    return;
  }
  if (hasLayers()) return;

  if (mod && !e.altKey) {
    if (key === 'n') { e.preventDefault(); createPage(null, 'page', { newTab: true }); return; }
    if (key === 't') {
      e.preventDefault();
      if (e.shiftKey) reopenClosedTab();
      else addTab({ type: 'home' }, true);
      return;
    }
    if (key === 'w') { e.preventDefault(); closeTab(activeTab); return; }
    if (key === '\\') { e.preventDefault(); toggleSidebar(); return; }
    if (key === ',') { e.preventDefault(); openSettings('account'); return; }
    if (key === 'j') { e.preventDefault(); toggleAI(); return; }
    if (key === 's') { e.preventDefault(); if (isEditorDirty()) flushEditor(true); return; }
    if (key === 'tab') { e.preventDefault(); cycleTabs(e.shiftKey ? -1 : 1); return; }
    if (/^[1-9]$/.test(key)) {
      const idx = Number(key) - 1;
      if (idx < tabs.length) { e.preventDefault(); setActiveTab(idx); }
      return;
    }
  }

  if (e.altKey && !mod) {
    if (key === 'arrowleft') { e.preventDefault(); goBack(); return; }
    if (key === 'arrowright') { e.preventDefault(); goForward(); return; }
  }
  if (key === 'f11') {
    e.preventDefault();
    windowAction('fullscreen');
  }
}

/* ---------------- UI bindings ---------------- */
let uiBound = false;

function bindUI() {
  if (uiBound) return;
  uiBound = true;

  $('#sidebarToggle')?.addEventListener('click', toggleSidebar);
  $('#navBack')?.addEventListener('click', goBack);
  $('#navForward')?.addEventListener('click', goForward);
  $('#newTab')?.addEventListener('click', () => addTab({ type: 'home' }, true));
  $('#sideSearch')?.addEventListener('click', openSearch);
  $('#workspaceBtn')?.addEventListener('click', (e) => showWorkspaceMenu(e.currentTarget));
  $('#accountRow')?.addEventListener('click', (e) => showAccountMenu(e.currentTarget));
  $('#sideInbox')?.addEventListener('click', (e) => showInbox(e.currentTarget));
  $('#sideNewPage')?.addEventListener('click', () => createPage(null, 'page'));
  $('#newRootPage')?.addEventListener('click', () => createPage(null, 'page'));
  $('#privateMore')?.addEventListener('click', (e) => showPrivateMore(e.currentTarget));

  $('#winMin')?.addEventListener('click', () => windowAction('minimize'));
  $('#winMax')?.addEventListener('click', () => windowAction('maximize'));
  $('#winClose')?.addEventListener('click', closeWindow);

  const invite = $('#inviteCard');
  invite?.addEventListener('click', (e) => {
    if (e.target.closest('#inviteClose')) return;
    openLocalShareInfo();
  });
  $('#inviteClose')?.addEventListener('click', (e) => {
    e.stopPropagation();
    invite?.classList.add('hidden');
    setInviteHidden();
  });

  $$('.side-item[data-nav]').forEach(btn => {
    btn.addEventListener('click', () => {
      const nav = btn.dataset.nav;
      if (nav === 'home') openRoute({ type: 'home' });
      else if (nav === 'library') openRoute({ type: 'library' });
      else if (nav === 'templates') openRoute({ type: 'templates' });
      else if (nav === 'trash') openRoute({ type: 'trash' });
      else if (nav === 'ai') toggleAI();
      else if (nav === 'settings') openSettings('account');
    });
  });

  tabsEl?.addEventListener('dblclick', (e) => {
    if (e.target === tabsEl) addTab({ type: 'home' }, true);
  });

  const titlebar = $('#titlebar');
  titlebar?.addEventListener('dblclick', (e) => {
    if (e.target.closest('.tab, button, .tabs')) return;
    windowAction('maximize');
  });

  // Sidebar resizing
  const resizer = $('#sidebarResizer');
  const sidebar = $('#sidebar');
  if (resizer && sidebar) {
    let drag = null;
    resizer.addEventListener('pointerdown', (e) => {
      if (appEl.classList.contains('sidebar-collapsed')) return;
      e.preventDefault();
      resizer.setPointerCapture?.(e.pointerId);
      drag = { startX: e.clientX, startWidth: sidebar.getBoundingClientRect().width };
      document.body.classList.add('resizing');
    });
    resizer.addEventListener('pointermove', (e) => {
      if (!drag) return;
      const width = clamp(Math.round(drag.startWidth + (e.clientX - drag.startX)), 220, 460);
      S.sidebarWidth = width;
      S.settings.sidebarWidth = width;
      document.documentElement.style.setProperty('--sidebar-width', `${width}px`);
    });
    const endResize = () => {
      if (!drag) return;
      drag = null;
      document.body.classList.remove('resizing');
      setSetting('sidebarWidth', S.sidebarWidth, { silent: true });
    };
    resizer.addEventListener('pointerup', endResize);
    resizer.addEventListener('pointercancel', endResize);
    resizer.addEventListener('dblclick', () => {
      S.sidebarWidth = 275;
      S.settings.sidebarWidth = 275;
      document.documentElement.style.setProperty('--sidebar-width', '275px');
      setSetting('sidebarWidth', 275, { silent: true });
    });
  }

  window.addEventListener('resize', debounce(() => {
    if (hasTauri) syncWindowState();
  }, 250));

  document.addEventListener('keydown', handleGlobalKey);
  document.addEventListener('click', (e) => {
    const link = e.target.closest('a[href]');
    if (!link) return;
    const href = link.getAttribute('href') || '';
    if (/^ideacraft:\/\/page\//i.test(href)) {
      e.preventDefault();
      const id = href.split('/').pop();
      if (id) openPage(id);
      return;
    }
    if (/^(https?:|mailto:)/i.test(href)) {
      e.preventDefault();
      invoke('open_url', { url: href }).catch(() => toast('Could not open the link', 'error'));
    }
  });
  window.addEventListener('beforeunload', () => {
    persistTabsNow();
    if (isEditorDirty()) flushEditor(true);
  });

  initTreeDropZone();

  if (window.MutationObserver) {
    new MutationObserver(() => {
      const aiOpen = !!$('.ai-drawer');
      const commentsOpen = !!$('.comments-drawer');
      if (aiOpen !== !!S.aiOpen || commentsOpen !== !!S.commentsOpen) {
        S.aiOpen = aiOpen;
        S.commentsOpen = commentsOpen;
        paintNavActive();
      }
    }).observe(document.body, { childList: true });
  }

  if (darkMedia) {
    const onChange = () => {
      if ((S.settings.theme || 'system') === 'system') applyTheme();
    };
    if (darkMedia.addEventListener) darkMedia.addEventListener('change', onChange);
    else darkMedia.addListener?.(onChange);
  }
}

/* ---------------- Tauri events ---------------- */
async function setupTauriEvents() {
  const T = window.__TAURI__;
  if (!T?.event?.listen) return;
  try {
    await T.event.listen('tray-new-page', () => {
      if (!booted) { pendingTrayPage = true; return; }
      createPage(null, 'page', { newTab: true });
    });
    await T.event.listen('open-settings', () => {
      if (!booted) { pendingTraySettings = true; return; }
      openSettings('account');
    });
  } catch (e) {
    console.warn('Could not subscribe to tray events:', e);
  }
  if (pendingTrayPage && booted) { pendingTrayPage = false; createPage(null, 'page', { newTab: true }); }
  if (pendingTraySettings && booted) { pendingTraySettings = false; openSettings('account'); }
}

/* ---------------- bootstrap ---------------- */
async function boot() {
  applyTheme();
  appEl.classList.add('booting');
  bindUI();
  showSplash();

  try {
    const data = await invoke('bootstrap');
    S.boot = data;
    S.app = {
      version: data.app?.version || '1.0.0',
      platform: data.app?.platform || 'windows',
      dataPath: data.app?.dataPath || '',
      ...(data.app || {}),
    };
    S.pages = data.pages || [];
    rebuildPageMap();
    S.settings = { ...DEFAULT_SETTINGS, ...(data.settings || {}) };

    applySettings(true);
    applyBranding();
    const validPageIds = data.pageIds?.length
      ? new Set(data.pageIds)
      : new Set((data.pages || []).map(p => p.id));
    restoreTabs(validPageIds);
    renderSidebar();
    paintTabs();
    updateNavButtons();
    paintNavActive();
    applyTheme();

    S.host = {
      openPage,
      openRoute,
      refreshTree,
      reloadPage,
      tabTitle,
      setTabDirty,
      closePageTabs,
      onPageShown,
      openSearch,
      applySettings,
      applyBranding,
      reloadWorkspace,
      syncPage: syncPageSummary,
    };

    await renderActive();
    booted = true;
    S.booted = true;
    appEl.classList.remove('booting');
    persistTabsNow();
    setupTauriEvents();
    startAutoSync();
    setTimeout(syncWindowState, 220);

    invoke('app_info').then(info => {
      S.app = { ...(S.app || {}), ...(info || {}) };
      if (S.boot) S.boot.app = S.app;
    }).catch(() => {});
  } catch (e) {
    console.error('IdeaCraft boot failed:', e);
    appEl.classList.remove('booting');
    S.booted = false;
    showBootError(e);
  }
}

boot();
