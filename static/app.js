/* exported getCurrentFolderId, toggleChevron, toggleTheme, trapFocus, openNewFolderPrompt, initApp, formatLocalDates, buildBookmarklet */

function getCurrentFolderId() {
  const el = document.getElementById('current-folder-id');
  return el ? el.value : document.body.dataset.folderId || '';
}

function toggleChevron(el) {
  el.classList.toggle('open');
  const sibling = el.closest('.tree-item').nextElementSibling;
  if (sibling && sibling.classList.contains('tree-children')) {
    sibling.classList.toggle('open');
  }
}

function getSystemTheme() {
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

function applyTheme(preference) {
  if (preference === 'system') {
    document.documentElement.removeAttribute('data-theme');
  } else {
    document.documentElement.setAttribute('data-theme', preference);
  }
  const store = Alpine.store('app');
  if (store) store.effectiveTheme = (preference === 'system') ? getSystemTheme() : preference;
}

function toggleTheme() {
  const store = Alpine.store('app');
  const cycle = { system: 'light', light: 'dark', dark: 'system' };
  const next = cycle[store.themePreference] || 'system';
  store.themePreference = next;
  localStorage.setItem('theme-preference', next);
  applyTheme(next);
}

function trapFocus(e) {
  const modal = e.currentTarget;
  const focusable = modal.querySelectorAll(
    'input:not([type="hidden"]):not([disabled]), button:not([disabled]), textarea, select, a[href], [tabindex]:not([tabindex="-1"])'
  );
  if (focusable.length === 0) return;
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  if (e.shiftKey) {
    if (document.activeElement === first) { e.preventDefault(); last.focus(); }
  } else {
    if (document.activeElement === last) { e.preventDefault(); first.focus(); }
  }
}

function openNewFolderPrompt() {
  Alpine.store('app').showFolderModal = true;
}

function formatLocalDates() {
  const times = document.querySelectorAll('time[datetime]');
  times.forEach(function(el) {
    const iso = el.getAttribute('datetime');
    if (!iso) return;
    const date = new Date(iso);
    if (isNaN(date.getTime())) return;
    const opts = el.getAttribute('data-format') === 'long'
      ? { month: 'short', day: 'numeric', year: 'numeric', hour: 'numeric', minute: '2-digit' }
      : { month: 'short', day: 'numeric', year: 'numeric' };
    el.textContent = new Intl.DateTimeFormat(undefined, opts).format(date);
  });
}

function buildBookmarklet() {
  const origin = window.location.origin;
  const code = "(function(){" +
    "var links=document.querySelectorAll('link[rel*=\"icon\"],link[rel=\"apple-touch-icon\"],link[rel=\"apple-touch-icon-precomposed\"]');" +
    "var best='',bestSize=0;" +
    "for(var i=0;i<links.length;i++){" +
    "var l=links[i];" +
    "if(!l.href||l.href.indexOf('data:')===0)continue;" +
    "var s=parseInt(l.getAttribute('sizes'),10)||0;" +
    "if(l.rel.indexOf('apple-touch-icon')!==-1&&s===0)s=180;" +
    "if(s>bestSize||(best===''&&s===0)){best=l.href;bestSize=s}}" +
    "if(!best)best=location.origin+'/favicon.ico';" +
    "window.open('" + origin + "/?url='+encodeURIComponent(location.href)+'&title='+encodeURIComponent(document.title)+'&favicon_url='+encodeURIComponent(best),'_blank')})()";
  return "javascript:void(" + code + ")";
}

function initApp() {
  const savedPref = localStorage.getItem('theme-preference') || 'system';
  const effectiveTheme = (savedPref === 'system') ? getSystemTheme() : savedPref;
  applyTheme(savedPref);

  const savedViewMode = localStorage.getItem('view-mode') || 'list';
  const savedSortOrder = localStorage.getItem('sort-order') || 'name_asc';

  Alpine.store('app', {
    themePreference: savedPref,
    effectiveTheme: effectiveTheme,
    viewMode: savedViewMode,
    sortOrder: savedSortOrder,
    showAddModal: false,
    showFolderModal: false,
    showEditModal: false,
    showMoveModal: false,
    showRenameModal: false,
    renameFolderId: '',
    renameFolderTitle: '',
    sidebarOpen: false,
    detailOpen: false,
    searchExpanded: false,
    prefillTitle: '',
    prefillUrl: '',
    prefillFaviconUrl: '',
    openRenameFolder: function(id, title) {
      this.renameFolderId = id;
      this.renameFolderTitle = title;
      this.showRenameModal = true;
    },
    openMovePicker: function(itemId) {
      htmx.ajax('GET', '/move-picker/' + itemId, {target: '#move-picker-body', swap: 'innerHTML'}).then(function() {
        Alpine.store('app').showMoveModal = true;
        const form = document.querySelector('#move-picker-body form');
        if (form) htmx.process(form);
      });
    },
    setSortOrder: function(order) {
      Alpine.store('app').sortOrder = order;
      const folderId = getCurrentFolderId();
      if (folderId) {
        htmx.ajax('GET', '/folders/' + folderId + '/content',
                  {target: '#folder-content', swap: 'innerHTML'});
      }
    }
  });

  const urlParams = new URLSearchParams(window.location.search);
  if (urlParams.has('url') || urlParams.has('title')) {
    Alpine.store('app').prefillTitle = urlParams.get('title') || '';
    Alpine.store('app').prefillUrl = urlParams.get('url') || '';
    Alpine.store('app').prefillFaviconUrl = urlParams.get('favicon_url') || '';
    Alpine.store('app').showAddModal = true;
    history.replaceState(null, '', window.location.pathname);
  }

  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', function() {
    const store = Alpine.store('app');
    if (store.themePreference === 'system') {
      store.effectiveTheme = getSystemTheme();
    }
  });

  function isMobile() { return window.innerWidth <= 768; }

  function updateDrawerOpenClass() {
    const store = Alpine.store('app');
    document.body.classList.toggle('drawer-open', store.sidebarOpen || store.detailOpen);
  }

  Alpine.effect(updateDrawerOpenClass);

  Alpine.effect(function() {
    localStorage.setItem('view-mode', Alpine.store('app').viewMode);
  });

  Alpine.effect(function() {
    localStorage.setItem('sort-order', Alpine.store('app').sortOrder);
  });

  document.body.addEventListener('htmx:configRequest', function(e) {
    const path = e.detail.path || '';
    if (path.match(/\/folders\/[^/]+\/content/) || path.match(/\/folders\/[^/]+$/) || path.match(/\/search/)) {
      e.detail.parameters.sort = Alpine.store('app').sortOrder;
    }
  });

  document.body.addEventListener('htmx:afterSwap', function(e) {
    if (e.detail.target && e.detail.target.id === 'detail-body') {
      htmx.process(e.detail.target);
      if (isMobile()) Alpine.store('app').detailOpen = true;
    }
  });

  document.body.addEventListener('htmx:afterSwap', function(e) {
    if (e.detail.target && e.detail.target.id === 'folder-content' && isMobile()) {
      Alpine.store('app').sidebarOpen = false;
    }
  });

  document.body.addEventListener('htmx:afterSwap', function(e) {
    if (e.detail.target && e.detail.target.id === 'edit-modal-body') {
      Alpine.store('app').showEditModal = true;
    }
  });

  document.body.addEventListener('htmx:afterSettle', function(e) {
    const path = e.detail.pathInfo && e.detail.pathInfo.requestPath;
    if (!path) return;
    if (path === '/bookmarks/new') {
      Alpine.store('app').showAddModal = false;
    } else if (path === '/folders/new') {
      Alpine.store('app').showFolderModal = false;
    } else if (path === '/import') {
      Alpine.store('app').showImportModal = false;
    } else if (path.match(/\/folders\/[^/]+\/rename/)) {
      Alpine.store('app').showRenameModal = false;
    }
  });

  document.body.addEventListener('htmx:afterRequest', function(e) {
    if (e.detail.elt && e.detail.elt.closest && e.detail.elt.closest('#move-picker-body')) {
      if (e.detail.successful) {
        Alpine.store('app').showMoveModal = false;
        const folderId = getCurrentFolderId();
        htmx.ajax('GET', '/sidebar' + (folderId ? '?folder_id=' + encodeURIComponent(folderId) : ''),
                  {target: '#sidebar-tree', swap: 'innerHTML'});
      }
    }
  });

  const searchBar = document.querySelector('.search-bar');
  const searchInput = document.getElementById('searchInput');
  if (searchBar) {
    searchBar.addEventListener('click', function() {
      if (isMobile() && !searchBar.classList.contains('expanded')) {
        searchBar.classList.add('expanded');
        searchInput.focus();
      }
    });
    searchInput.addEventListener('blur', function() {
      if (isMobile() && !searchInput.value) {
        searchBar.classList.remove('expanded');
      }
    });
  }

  formatLocalDates();
  document.body.addEventListener('htmx:afterSettle', formatLocalDates);

  (function() {
    let retryDelay = 1000;
    const maxRetryDelay = 30000;
    let pendingRefresh = null;
    let folderContentVersion = 0;
    let sseSwapVersion = -1;

    function cancelPendingRefresh() {
      if (pendingRefresh) {
        clearTimeout(pendingRefresh);
        pendingRefresh = null;
      }
    }

    document.body.addEventListener('htmx:afterSwap', function(e) {
      if (e.detail.target && e.detail.target.id === 'folder-content') {
        folderContentVersion++;
        cancelPendingRefresh();
      }
    });

    const sseSource = document.createElement('div');
    sseSource.id = 'sse-source';
    sseSource.style.display = 'none';
    document.body.appendChild(sseSource);

    document.body.addEventListener('htmx:beforeSwap', function(e) {
      if (e.detail.target && e.detail.target.id === 'folder-content' &&
          e.detail.elt === sseSource && sseSwapVersion !== folderContentVersion) {
        e.detail.shouldSwap = false;
      }
    });

    function connectSSE() {
      const evtSource = new EventSource('/events');

      evtSource.addEventListener('refresh', function() {
        retryDelay = 1000;
        cancelPendingRefresh();

        const folderId = getCurrentFolderId();

        const statusEl = document.getElementById('status-text');
        if (statusEl) statusEl.textContent = 'Syncing\u2026';

        htmx.ajax('GET', '/sidebar' + (folderId ? '?folder_id=' + encodeURIComponent(folderId) : ''),
                  {target: '#sidebar-tree', swap: 'innerHTML'});

        if (folderId) {
          pendingRefresh = setTimeout(function() {
            pendingRefresh = null;
            if (folderContentVersion === sseSwapVersion) return;
            sseSwapVersion = folderContentVersion;
            htmx.ajax('GET', '/folders/' + folderId + '/content',
                      {target: '#folder-content', swap: 'innerHTML', source: sseSource});
          }, 500);
        }

        const selected = document.querySelector('.list-item.selected[data-item-id]');
        if (selected) {
          htmx.ajax('GET', '/bookmarks/' + selected.getAttribute('data-item-id') + '/detail',
                    {target: '#detail-body', swap: 'innerHTML'});
        }
      });

      evtSource.onerror = function() {
        evtSource.close();
        setTimeout(connectSSE, retryDelay);
        retryDelay = Math.min(retryDelay * 2, maxRetryDelay);
      };
    }

    connectSSE();
  })();
}

document.addEventListener('keydown', function(e) {
  if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA') return;
  if (e.key === 'Escape') {
    Alpine.store('app').showAddModal = false;
    Alpine.store('app').showFolderModal = false;
    Alpine.store('app').showEditModal = false;
    Alpine.store('app').showMoveModal = false;
    Alpine.store('app').showRenameModal = false;
  }
  if ((e.metaKey || e.ctrlKey) && e.key === 'd') {
    e.preventDefault();
    Alpine.store('app').prefillTitle = '';
    Alpine.store('app').prefillUrl = '';
    Alpine.store('app').prefillFaviconUrl = '';
    Alpine.store('app').showAddModal = true;
  }
  if ((e.metaKey || e.ctrlKey) && e.key === 'n') {
    e.preventDefault();
    openNewFolderPrompt();
  }
  if ((e.metaKey || e.ctrlKey) && e.key === 'f') {
    e.preventDefault();
    document.getElementById('searchInput').focus();
  }
});
