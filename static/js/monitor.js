(() => {
    'use strict';

    // 全局状态
    let currentPage = 1;
    let pageSize = 20;
    let lastPaginationMeta = null;
    let currentFilters = {};
    let authSession = null;
    let monitorUsersCache = [];
    let isAuthenticated = false;
    let hasAuthenticatedBootstrap = false;
    let createUserModalRole = 'ops_admin';
    let storedApiBase = '';
    let sessionInvalidNotified = false;
    // Auto-refresh state
    let autoRefreshTimer = null;
    let isAutoRefreshEnabled = false;
    const AUTO_REFRESH_INTERVAL = 5000;
    // 业务耗时趋势历史（毫秒）
    const durationTrendHistory = [];

    // 简单的 HTML 转义
    const h = (str) =>
        String(str ?? '')
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#39;');

    // Toast Notification
    function showToast(message, type = 'info') {
        const container = document.getElementById('toastContainer');
        if (!container) return;

        const toast = document.createElement('div');
        toast.className = `toast ${type}`;

        const iconMap = {
            success: '✓',
            error: '✕',
            warning: '⚠',
            info: 'ℹ'
        };

        const titleMap = {
            success: '成功',
            error: '错误',
            warning: '警告',
            info: '提示'
        };

        toast.innerHTML = `
            <div class="toast-icon">${iconMap[type] || iconMap.info}</div>
            <div class="toast-content">
                <div class="toast-title">${titleMap[type] || '提示'}</div>
                <div class="toast-message">${h(message)}</div>
            </div>
            <button class="toast-close" onclick="this.parentElement.remove()">×</button>
        `;

        container.appendChild(toast);

        // Auto dismiss
        setTimeout(() => {
            toast.style.opacity = '0';
            toast.style.transform = 'translateX(100%)';
            setTimeout(() => toast.remove(), 300);
        }, 3000);
    }

    // Loading State Helper
    function setLoading(element, isLoading) {
        if (!element) return;
        if (isLoading) {
            element.classList.add('disabled');
            element.disabled = true;
            if (!element.querySelector('.loading-spinner')) {
                const spinner = document.createElement('span');
                spinner.className = 'loading-spinner';
                element.prepend(spinner);
            }
        } else {
            element.classList.remove('disabled');
            element.disabled = false;
            const spinner = element.querySelector('.loading-spinner');
            if (spinner) spinner.remove();
        }
    }

    // Helper to get API base URL
    function getApiUrl(path) {
        if (!path) return path;
        // 绝对地址直接返回
        if (/^https?:\/\//.test(path)) return path;

        if (!storedApiBase) {
            const fromStorage = localStorage.getItem('monitor_api_base') || '/api';
            if (
                fromStorage.startsWith('http://') ||
                fromStorage.startsWith('https://') ||
                fromStorage.startsWith('/')
            ) {
                storedApiBase = fromStorage.replace(/\/+$/, '');
            } else {
                storedApiBase = '/api';
            }
        }

        const base = storedApiBase.replace(/\/+$/, '');
        const normalizedPath = path.startsWith('/') ? path : `/${path}`;

        // 绝对 base（含协议），需要避免出现 /api/api/... 的重复前缀
        if (/^https?:\/\//.test(base)) {
            try {
                const baseUrl = new URL(base, window.location.origin);
                const basePath = baseUrl.pathname.replace(/\/+$/, '') || '';
                const hasPrefix =
                    normalizedPath === basePath ||
                    (basePath && normalizedPath.startsWith(`${basePath}/`));
                const mergedPath = hasPrefix
                    ? normalizedPath
                    : `${basePath}${normalizedPath}`;
                // mergedPath 已包含前导 /，直接拼到 origin 上即可
                return `${baseUrl.origin}${mergedPath}`;
            } catch (error) {
                console.warn('解析 API Base 失败，回退使用相对路径:', error);
                return normalizedPath;
            }
        }

        // 避免重复前缀，例如 base=/api 且 path=/api/...
        if (base && normalizedPath.startsWith(`${base}/`)) {
            return normalizedPath;
        }

        return `${base}${normalizedPath}`;
    }

    // Generic API fetch wrapper
    async function apiFetch(url, options = {}) {
        // 统一在所有请求上附加 monitor_session_id，避免漏加导致 401
        const fullUrl = appendMonitorSessionParam(getApiUrl(url));
        const headers = {
            'Content-Type': 'application/json',
            ...(authSession ? {
                'Authorization': `Bearer ${authSession}`,
                'x-monitor-session-id': authSession
            } : {})
        };

        try {
            const response = await fetch(fullUrl, {
                credentials: 'include',
                headers: headers,
                ...options
            });

            if (response.status === 401) {
                console.error('Unauthorized access, redirecting to login.');
                logoutUser();
                return new Response(null, { status: 401 });
            }

            return response;
        } catch (error) {
            console.error('API fetch error:', error);
            throw error; // Re-throw to be caught by calling function
        }
    }

    function toggleAutoRefresh() {
        const toggle = document.getElementById('autoRefreshToggle');
        if (!toggle) return;
        // 关闭自动刷新，避免并发请求堆积
        toggle.checked = false;
        isAutoRefreshEnabled = false;
        stopAutoRefresh();
        showToast('为避免接口压力，已禁用自动刷新，请手动点击“刷新”获取最新数据。', 'info');
    }

    function startAutoRefresh() {
        stopAutoRefresh();
        if (!isAutoRefreshEnabled) return;

        autoRefreshTimer = setInterval(() => {
            if (!isAuthenticated) {
                stopAutoRefresh();
                return;
            }

            // Refresh data based on active tab
            const activeTab = document.querySelector('.sidebar-nav .nav-item.active');
            if (activeTab) {
                const tabId = activeTab.id;
                switch (tabId) {
                    case 'tab-business':
                        // Silent refresh for business data
                        loadStatistics();
                        // Only refresh table if not manually paging/filtering to avoid UX disruption
                        // For now, we just refresh stats to keep it safe
                        break;
                    case 'tab-system':
                        refreshSystemData(true); // true for silent
                        break;
                    case 'tab-failover':
                        checkFailoverStatus();
                        break;
                    case 'tab-ocr':
                        refreshOCRStats();
                        break;
                    case 'tab-concurrency':
                        refreshConcurrency();
                        break;
                }
            }
        }, AUTO_REFRESH_INTERVAL);
    }

    function stopAutoRefresh() {
        if (autoRefreshTimer) {
            clearInterval(autoRefreshTimer);
            autoRefreshTimer = null;
        }
    }

    // Expose to window
    window.toggleAutoRefresh = toggleAutoRefresh;

    function initializePage() {
        if (!isAuthenticated) return;
        loadBusinessData();

        if (isCurrentUserFullAccess()) {
            loadSystemData();
            checkFailoverStatus();
            refreshOCRStats();
            refreshConcurrency();
        }

        // Restore auto-refresh state if needed (optional, for now defaults to off)
        // const storedAutoRefresh = localStorage.getItem('monitor_auto_refresh') === 'true';
        // if (storedAutoRefresh) {
        //     const toggle = document.getElementById('autoRefreshToggle');
        //     if (toggle) {
        //         toggle.checked = true;
        //         toggleAutoRefresh();
        //     }
        // }
    }

    function switchTab(tabName) {
        if (!isAuthenticated && tabName !== 'auth') {
            enforceAuthLock();
            return;
        }

        if (
            !isCurrentUserFullAccess() &&
            ['system', 'failover', 'ocr', 'concurrency'].includes(tabName)
        ) {
            showToast('当前角色无权限查看该页面', 'warning');
            return;
        }

        activateTab(tabName);

        if (tabName === 'auth') {
            showLoginForm();
            return;
        }

        // Restart auto-refresh to trigger immediate update if enabled
        if (isAutoRefreshEnabled) {
            startAutoRefresh();
        }

        switch (tabName) {
            case 'business':
                loadBusinessData();
                break;
            case 'system':
                loadSystemData();
                break;
            case 'failover':
                checkFailoverStatus();
                break;
            case 'ocr':
                refreshOCRStats();
                break;
            case 'concurrency':
                refreshConcurrency();
                break;
            default:
                break;
        }
    }

    // ... (existing code) ...

    async function loadSystemData(silent = false) {
        const content = document.getElementById('systemTableContent');
        if (content && !silent) {
            content.innerHTML = '<div class="loading">正在加载系统数据...</div>';
        }

        try {
            const response = await apiFetch('/api/resources/status?detailed=true');
            if (response.ok) {
                const data = await response.json();
                if (data.success) {
                    updateSystemMetrics(data.data);
                    renderSystemTable(data.data);
                    return;
                }
            }
            if (content && !silent) {
                content.innerHTML = '<p style="color:#f5222d;">加载失败</p>';
            }
        } catch (error) {
            console.error('加载系统数据失败:', error);
            if (content && !silent) {
                content.innerHTML = '<p style="color:#f5222d;">加载失败</p>';
            }
        }
    }

    function updateSystemMetrics(data) {
        if (!data) return;
        const sys = data.system_resources || {};

        const cpu = sys.cpu_usage || {};
        const mem = sys.memory_usage || {};
        const disk = sys.disk_usage || {};

        // Update text values
        setText('cpuUsage', `${cpu.usage_percent || 0}%`);
        setText('memoryUsage', `${mem.usage_percent || 0}%`);
        setText('diskUsage', `${disk.usage_percent || 0}%`);

        // Update progress bars
        updateProgressBar('cpuBar', cpu.usage_percent || 0);
        updateProgressBar('memoryBar', mem.usage_percent || 0);
        updateProgressBar('diskBar', disk.usage_percent || 0);

        // ... (rest of updateSystemMetrics) ...
        const ocr = data.ocr_pool || {};
        const active = ocr.in_use || 0;
        const total = ocr.capacity || 0;
        setText('ocrStatus', `${active} / ${total}`);
        setText('ocrRestartsHint', `重启 ${ocr.total_restarted || 0} 次`);

        const watchdog = sys.watchdog_states || {};
        const watchdogCount = Object.keys(watchdog).length;
        setText('watchdogStatus', `${watchdogCount} 个服务`);
        setText('watchdogHint', '运行正常');
    }

    function updateProgressBar(elementId, percentage) {
        const bar = document.getElementById(elementId);
        if (!bar) return;

        bar.style.width = `${percentage}%`;

        // Reset classes
        bar.className = 'progress-bar';

        if (percentage >= 90) {
            bar.classList.add('error');
        } else if (percentage >= 75) {
            bar.classList.add('warning');
        } else {
            bar.classList.add('success');
        }
    }

    // Expose refreshSystemData to window for button click
    window.refreshSystemData = () => loadSystemData(false);

    try {
        storedApiBase = localStorage.getItem('monitor_api_base') || '';
    } catch (error) {
        storedApiBase = '';
    }

    const candidateApiBase =
        typeof window !== 'undefined' && window.__MONITOR_API_BASE
            ? window.__MONITOR_API_BASE
            : storedApiBase;

    const apiBase =
        typeof candidateApiBase === 'string'
            ? candidateApiBase.replace(/\/+$/, '')
            : '';

    if (apiBase && apiBase !== storedApiBase) {
        try {
            localStorage.setItem('monitor_api_base', apiBase);
        } catch (error) {
            console.warn('无法缓存 API Base 地址:', error);
        }
    }

    function buildApiUrl(path = '') {
        if (!path) {
            return apiBase || '';
        }
        if (/^https?:\/\//i.test(path)) {
            return path;
        }
        const normalizedPath = path.startsWith('/')
            ? path
            : `/${path}`;
        return `${apiBase}${normalizedPath}`;
    }



    const inflightRequests = new Map();

    function runSingleFlight(key, factory, { forceReload = false } = {}) {
        if (!forceReload && inflightRequests.has(key)) {
            return inflightRequests.get(key);
        }

        const promise = (async () => {
            try {
                return await factory();
            } finally {
                if (inflightRequests.get(key) === promise) {
                    inflightRequests.delete(key);
                }
            }
        })();

        inflightRequests.set(key, promise);
        return promise;
    }

    async function safeJsonRequest(path, options) {
        try {
            const response = await apiFetch(path, options);
            if (!response.ok) {
                console.warn(`请求 ${path} 返回状态 ${response.status}`);
                return null;
            }
            try {
                return await response.json();
            } catch (parseError) {
                console.warn(`解析 ${path} 的响应失败`, parseError);
                return null;
            }
        } catch (error) {
            console.error(`请求 ${path} 失败:`, error);
            return null;
        }
    }

    document.addEventListener('DOMContentLoaded', () => {
        switchTab('auth');
        enforceAuthLock();
        checkAuthStatus();

        applyDefaultDateFilters();

        toggleUserManagementVisibility();

        const detailModal = document.getElementById('requestDetailModal');
        if (detailModal) {
            detailModal.addEventListener('click', (event) => {
                if (event.target === detailModal) {
                    closeRequestDetail();
                }
            });
        }

        const createUserModal = document.getElementById('createUserModal');
        if (createUserModal) {
            createUserModal.addEventListener('click', (event) => {
                if (event.target === createUserModal) {
                    closeCreateUserModal();
                }
            });
        }

        document.addEventListener('keydown', (event) => {
            if (event.key === 'Escape') {
                closeRequestDetail();
            }
        });

        const pageSizeSelect = document.getElementById('pageSizeSelect');
        if (pageSizeSelect) {
            pageSizeSelect.value = String(pageSize);
        }
    });

    function formatDateForInput(date) {
        const year = date.getFullYear();
        const month = `${date.getMonth() + 1}`.padStart(2, '0');
        const day = `${date.getDate()}`.padStart(2, '0');
        return `${year}-${month}-${day}`;
    }

    function getDefaultDateRange() {
        const today = new Date();
        const dayOfWeek = today.getDay() || 7; // 将周日视为 7
        const monday = new Date(today);
        monday.setDate(today.getDate() - (dayOfWeek - 1));
        return {
            from: formatDateForInput(monday),
            to: formatDateForInput(today),
        };
    }

    function applyDefaultDateFilters() {
        const { from, to } = getDefaultDateRange();
        const dateToInput = document.getElementById('dateTo');
        const dateFromInput = document.getElementById('dateFrom');
        if (dateFromInput) dateFromInput.value = from;
        if (dateToInput) dateToInput.value = to;

        currentFilters = {
            date_from: from,
            date_to: to,
        };
    }

    function initializePage() {
        if (!isAuthenticated) return;
        loadBusinessData();
        if (isCurrentUserFullAccess()) {
            loadSystemData();
            checkFailoverStatus();
            refreshOCRStats();
            refreshConcurrency();
        }
    }

    function activateTab(tabName) {
        // Update Sidebar
        document
            .querySelectorAll('.sidebar-nav .nav-item')
            .forEach((item) =>
                item.classList.toggle(
                    'active',
                    item.id === `tab-${tabName}`,
                ),
            );

        // Update Content Pane
        document
            .querySelectorAll('.view-pane')
            .forEach((pane) =>
                pane.classList.toggle(
                    'active',
                    pane.id === `${tabName}-pane`,
                ),
            );

        // Update Page Title
        const titleMap = {
            'business': '业务统计',
            'system': '系统监控',
            'auth': '监控认证',
            'failover': '故障转移',
            'ocr': 'OCR引擎池',
            'concurrency': '并发控制'
        };
        const pageTitle = document.getElementById('pageTitle');
        if (pageTitle) {
            pageTitle.textContent = titleMap[tabName] || '运维中心';
        }
    }

    function enforceAuthLock() {
        const navItems = document.querySelectorAll('.sidebar-nav .nav-item');
        navItems.forEach((item) => {
            const isAuthTab = item.id === 'tab-auth';
            const locked = !isAuthenticated && !isAuthTab;

            // Disable pointer events for locked items
            item.style.pointerEvents = locked ? 'none' : 'auto';
            item.style.opacity = locked ? '0.5' : '1';
        });

        if (!isAuthenticated) {
            activateTab('auth');
            showLoginForm();
        }
    }

    function switchTab(tabName) {
        if (!isAuthenticated && tabName !== 'auth') {
            enforceAuthLock();
            return;
        }

        if (
            !isCurrentUserFullAccess() &&
            ['system', 'failover', 'ocr', 'concurrency'].includes(tabName)
        ) {
            showToast('当前角色无权限查看该页面', 'warning');
            return;
        }

        activateTab(tabName);

        if (tabName === 'auth') {
            showLoginForm();
            return;
        }

        switch (tabName) {
            case 'business':
                loadBusinessData();
                break;
            case 'system':
                loadSystemData();
                break;
            case 'failover':
                checkFailoverStatus();
                break;
            case 'ocr':
                refreshOCRStats();
                break;
            case 'concurrency':
                refreshConcurrency();
                break;
            default:
                break;
        }
    }

    async function checkAuthStatus() {
        try {
            const sessionId = localStorage.getItem('monitor_session_id');
            if (!sessionId) {
                handleLoggedOutState();
                return;
            }

            // 先写入内存，保证请求会自动附加 monitor_session_id
            authSession = sessionId;

            const response = await apiFetch('/api/monitor/auth/status');
            if (!response.ok) {
                handleLoggedOutState();
                return;
            }

            const data = await response.json();
            if (data.success && data.data) {
                handleAuthenticatedState({ id: sessionId });
            } else {
                handleLoggedOutState();
            }
        } catch (error) {
            console.error('检查认证状态失败:', error);
            handleLoggedOutState();
        }
    }

    function handleAuthenticatedState(session) {
        const sessionId = typeof session === 'string' ? session : session?.id;
        if (!sessionId) return;

        authSession = sessionId;
        localStorage.setItem('monitor_session_id', sessionId);

        updateAuthStatus(true);
        showAuthManagement();
        enforceAuthLock();

        if (session?.user) {
            updateAuthDashboardFromUser(session.user);
        }
        toggleUserManagementVisibility();
        applyRoleBasedVisibility();

        sessionInvalidNotified = false;

        if (!hasAuthenticatedBootstrap) {
            hasAuthenticatedBootstrap = true;
            activateTab('business');
            initializePage();
        } else {
            refreshBusinessData();
        }
    }

    function handleLoggedOutState() {
        updateAuthStatus(false);
        authSession = null;
        monitorUsersCache = [];
        hasAuthenticatedBootstrap = false;
        updateAuthDashboardFromUser(null);
        renderMonitorUsers([]);
        toggleUserManagementVisibility();
        applyRoleBasedVisibility();
        enforceAuthLock();
    }

    function handleSessionInvalid(message) {
        if (!sessionInvalidNotified && message) {
            showToast(message, 'warning');
        } else if (!sessionInvalidNotified) {
            showToast('登录会话已失效，请重新登录', 'warning');
        }
        sessionInvalidNotified = true;
        handleLoggedOutState();
    }

    function updateAuthStatus(authenticated) {
        isAuthenticated = Boolean(authenticated);
        const authStatus = document.getElementById('authStatus');
        const logoutBtn = document.getElementById('logoutBtn');

        if (!authStatus || !logoutBtn) return;

        if (isAuthenticated) {
            authStatus.className = 'auth-status authenticated';
            authStatus.querySelector('span').textContent = '已登录';
            logoutBtn.style.display = 'inline-flex';
        } else {
            authStatus.className = 'auth-status unauthenticated';
            authStatus.querySelector('span').textContent = '未登录';
            logoutBtn.style.display = 'none';
            localStorage.removeItem('monitor_session_id');
            localStorage.removeItem('monitor_username');
            localStorage.removeItem('monitor_role');
            authSession = null;
        }
    }

    function updateAuthDashboardFromUser(user) {
        const activeSessions = document.getElementById('activeSessions');
        const loginCount = document.getElementById('loginCount');
        const currentUser = document.getElementById('currentUser');
        const sessionStatus = document.getElementById('sessionStatus');

        if (activeSessions) activeSessions.textContent = user ? '1' : '-';
        if (loginCount) loginCount.textContent = user?.login_count ?? '-';
        if (currentUser) currentUser.textContent = user?.username ?? '-';
        if (sessionStatus) {
            sessionStatus.textContent = user ? '活跃' : '未登录';
        }

        if (user?.username) {
            localStorage.setItem('monitor_username', user.username);
            if (user.role) {
                localStorage.setItem('monitor_role', user.role);
            }
        } else {
            localStorage.removeItem('monitor_username');
            localStorage.removeItem('monitor_role');
        }

        toggleUserManagementVisibility();
    }

    async function login() {
        const username = document.getElementById('username')?.value;
        const password = document.getElementById('password')?.value;
        const loginBtn = document.querySelector('#loginForm .btn-primary');

        if (!username || !password) {
            showToast('请输入用户名和密码', 'warning');
            return;
        }

        setLoading(loginBtn, true);

        try {
            const response = await apiFetch('/api/monitor/auth/login', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({ username, password }),
            });

            const data = await response.json();

            if (data.success && data.session) {
                handleAuthenticatedState(data.session);
                showToast('登录成功', 'success');
            } else {
                showToast(`登录失败: ${data.message || '未知错误'}`, 'error');
            }
        } catch (error) {
            console.error('登录失败:', error);
            showToast('登录失败，请稍后重试', 'error');
        } finally {
            setLoading(loginBtn, false);
        }
    }

    async function logout() {
        if (!authSession) {
            handleLoggedOutState();
            return;
        }

        try {
            await apiFetch('/api/monitor/auth/logout', {
                method: 'POST',
            });
        } catch (error) {
            console.error('登出请求失败:', error);
        }

        handleLoggedOutState();
    }

    function showLoginForm() {
        const loginForm = document.getElementById('loginForm');
        const authManagement = document.getElementById('authManagement');
        if (loginForm) loginForm.style.display = 'block';
        if (authManagement) authManagement.style.display = 'none';
    }

    function showAuthManagement() {
        const loginForm = document.getElementById('loginForm');
        const authManagement = document.getElementById('authManagement');
        if (loginForm) loginForm.style.display = 'none';
        if (authManagement) authManagement.style.display = 'block';
        refreshAuthData();
    }

    async function refreshAuthData() {
        if (!authSession) return;
        if (isCurrentUserFullAccess()) {
            await loadMonitorUsers();

            const storedUsername = localStorage.getItem('monitor_username');
            let userInfo = null;
            if (storedUsername) {
                userInfo =
                    monitorUsersCache.find((u) => u.username === storedUsername) ||
                    null;
            } else if (monitorUsersCache.length > 0) {
                userInfo = monitorUsersCache[0];
            }
            if (userInfo) {
                updateAuthDashboardFromUser(userInfo);
            }
        } else {
            // 受限角色不具备用户管理权限，避免触发 403 导致误判会话失效
            monitorUsersCache = [];
            renderMonitorUsers([]);
        }

        try {
            const response = await apiFetch('/api/monitor/auth/status');
            if (!response.ok) {
                handleSessionInvalid('会话已失效，请重新登录');
                return;
            }
            const data = await response.json();
            if (!data.success || !data.data) {
                handleSessionInvalid(data.message || '会话已失效，请重新登录');
            }
        } catch (error) {
            console.error('刷新认证状态失败:', error);
            handleSessionInvalid('无法验证会话，请重新登录');
        }
    }

    async function cleanupSessions() {
        if (!authSession) return;

        try {
            const response = await apiFetch('/api/monitor/auth/cleanup', {
                method: 'POST',
            });

            if (response.ok) {
                const data = await response.json();
                showToast(`已清理 ${data.data || 0} 个过期会话`, 'success');
                refreshAuthData();
            } else {
                showToast('清理会话失败', 'error');
            }
        } catch (error) {
            console.error('清理会话失败:', error);
            showToast('清理会话失败，请稍后重试', 'error');
        }
    }

    function loadBusinessData() {
        loadStatistics();
        loadPreviewRequests();
        loadEnhancedMonitoring();
        loadRecentFailures();
    }

    async function loadStatistics() {
        try {
            const response = await apiFetch('/api/preview/statistics');
            if (response.ok) {
                const data = await response.json();
                if (data.success) {
                    updateStatisticsDisplay(data.data);
                }
            }
        } catch (error) {
            console.error('加载统计数据失败:', error);
        }
    }

    function updateStatisticsDisplay(stats) {
        if (!stats) return;
        const total = Number(stats.total) || 0;
        const completed = Number(stats.completed) || 0;
        const processing = Number(stats.processing) || 0;
        const failed = Number(stats.failed) || 0;

        setText('totalCount', total);
        setText('completedCount', completed);
        setText('processingCount', processing);
        setText('failedCount', failed);

        const successRate =
            total > 0 ? Math.round((completed / total) * 1000) / 10 : null;
        setText(
            'successRateMetric',
            successRate !== null ? `${successRate.toFixed(successRate % 1 === 0 ? 0 : 1)}%` : '--',
        );

        const avgDurationSeconds =
            stats.avg_duration_seconds ??
            stats.avgDurationSeconds ??
            stats.avg_duration ??
            stats.avgDuration ??
            null;
        setText(
            'avgDurationMetric',
            formatDuration(avgDurationSeconds) || '--',
        );
        setText(
            'avgDurationHint',
            total > 0 ? `基于 ${total} 次预审` : '暂无数据',
        );

        setText('statisticsLastUpdate', formatDateTime(new Date()));

        // [charts] 更新业务统计图表
        updateBusinessCharts(stats);
    }

    function updateBusinessCharts(stats) {
        if (!stats || typeof window.MonitorCharts === 'undefined') {
            return;
        }

        try {
            const completed = Number(stats.completed) || 0;
            const processing = Number(stats.processing) || 0;
            const failed = Number(stats.failed) || 0;
            const queued = Number(stats.pending || stats.queued) || 0;

            if (typeof window.MonitorCharts.updateStatusDistribution === 'function') {
                window.MonitorCharts.updateStatusDistribution(
                    completed,
                    processing,
                    failed,
                    queued,
                );
            }

            const avgDurationSeconds =
                stats.avg_duration_seconds ??
                stats.avgDurationSeconds ??
                stats.avg_duration ??
                stats.avgDuration ??
                null;
            if (avgDurationSeconds && avgDurationSeconds > 0) {
                durationTrendHistory.push(Number(avgDurationSeconds) * 1000);
                if (durationTrendHistory.length > 20) {
                    durationTrendHistory.shift();
                }
                if (typeof window.MonitorCharts.updateDurationTrend === 'function') {
                    window.MonitorCharts.updateDurationTrend(durationTrendHistory);
                }
            }
        } catch (error) {
            console.error('更新业务图表失败:', error);
        }
    }

    async function loadPreviewRequests() {
        const content = document.getElementById('businessTableContent');
        if (content) {
            content.innerHTML = '<div class="loading">正在加载数据...</div>';
        }

        try {
            const params = new URLSearchParams({
                page: currentPage.toString(),
                size: pageSize.toString(),
                ...currentFilters,
            });

            const response = await apiFetch(
                `/api/preview/requests?${params.toString()}`,
            );

            if (response.ok) {
                const data = await response.json();
                if (data.success) {
                    renderPreviewRequestsTable(data.data);
                    return;
                }
                if (content) {
                    content.innerHTML = `<p style="color:#f5222d;">加载失败：${data.message || '未知错误'
                        }</p>`;
                }
                renderPaginationControls(null);
                return;
            }
            if (content) {
                content.innerHTML =
                    '<p style="color:#f5222d;">加载失败，请稍后重试</p>';
            }
            renderPaginationControls(null);
        } catch (error) {
            console.error('加载预审记录失败:', error);
            if (content) {
                content.innerHTML =
                    '<p style="color:#f5222d;">加载失败，请稍后重试</p>';
            }
            renderPaginationControls(null);
        }
    }

    function renderPreviewRequestsTable(data) {
        const content = document.getElementById('businessTableContent');
        if (!content) return;

        const records = data?.records || [];
        const pagination = data?.pagination || null;

        if (!records.length) {
            content.innerHTML =
                '<p style="text-align:center;padding:24px;color:#666;">暂无预审记录</p>';
            renderPaginationControls(pagination);
            return;
        }

        const rows = records
            .map((record) => {
                const requestIdRaw = record.request_id || '-';
                const requestId = h(requestIdRaw);
                const previewId =
                    record.preview_id ||
                    record.latest_preview_id ||
                    '';
                const statusHtml = formatStatus(record.latest_status);
                const createdAt = h(formatDateTime(record.created_at));
                const updatedAt = h(formatDateTime(record.updated_at));
                const matterName = h(record.matter_name || '-');
                const matterId = h(record.matter_id || '-');
                const channel = h(record.channel || '-');
                const userId = h(record.user_id || '-');
                const userName = h(
                    record.user_name ||
                    record.user_info?.user_name ||
                    '-',
                );
                const viewUrl = previewId ? `/api/preview/view/${previewId}` : '';
                const downloadFallback = previewId
                    ? `/api/preview/download/${previewId}`
                    : '';
                const preferredDownloadUrl =
                    record.preview_download_url && record.preview_download_url.trim().length
                        ? record.preview_download_url
                        : downloadFallback;

                const encodedViewUrl = viewUrl ? encodeURIComponent(viewUrl) : '';
                const encodedDownloadUrl = preferredDownloadUrl
                    ? encodeURIComponent(preferredDownloadUrl)
                    : '';

                const previewBtn = previewId
                    ? `<button class="btn btn-secondary btn-sm" onclick="viewPreview('${previewId}', '${encodedViewUrl}')">预览</button>`
                    : '<button class="btn btn-secondary btn-sm" disabled>预览</button>';
                const detailBtn =
                    requestId && requestId !== '-'
                        ? `<button class="btn btn-secondary btn-sm" onclick="showRequestDetail('${requestIdRaw}')">详情</button>`
                        : '<button class="btn btn-secondary btn-sm" disabled>详情</button>';

                const downloadBtn = encodedDownloadUrl
                    ? `<button class="btn btn-primary btn-sm" onclick="downloadResult('${previewId}', '${encodedDownloadUrl}')">下载</button>`
                    : '<button class="btn btn-primary btn-sm" disabled>下载</button>';

                return `
                    <tr>
                        <td><span class="text-clip">${requestId}</span></td>
                        <td><span class="text-clip">${matterName} (${matterId})</span></td>
                        <td><span class="text-clip">${previewId ? h(previewId) : '-'}</span></td>
                        <td>${channel}</td>
                        <td><span class="text-clip">${userId} / ${userName}</span></td>
                        <td>${statusHtml}</td>
                        <td>${createdAt}</td>
                        <td>${updatedAt}</td>
                        <td>
                            <div class="table-actions">${previewBtn} ${detailBtn} ${downloadBtn}</div>
                        </td>
                    </tr>
                `;
            })
            .join('');

        content.innerHTML = `
	            <table class="data-table">
	                <thead>
	                    <tr>
	                        <th>请求ID</th>
	                        <th>事项</th>
	                        <th>预审ID</th>
	                        <th>渠道</th>
	                        <th>用户</th>
	                        <th>状态</th>
	                        <th>创建时间</th>
	                        <th>更新时间</th>
                        <th>操作</th>
                    </tr>
                </thead>
                <tbody>
                    ${rows}
                </tbody>
            </table>
        `;

        renderPaginationControls(pagination);
    }



    function renderPaginationControls(pagination) {
        const container = document.getElementById('businessPagination');
        if (!container) return;

        if (!pagination) {
            lastPaginationMeta = null;
            container.style.display = 'none';
            container.innerHTML = '';
            return;
        }

        const totalRecords = Number(pagination.total_records) || 0;
        const pageSizeFromServer = Number(pagination.page_size);
        const totalPagesFromServer = Number(pagination.total_pages);
        const effectivePageSize =
            Number.isFinite(pageSizeFromServer) && pageSizeFromServer > 0
                ? pageSizeFromServer
                : pageSize || 20;
        const totalPages =
            totalRecords > 0
                ? Math.max(
                    Number.isFinite(totalPagesFromServer) && totalPagesFromServer > 0
                        ? totalPagesFromServer
                        : Math.ceil(totalRecords / effectivePageSize),
                    1,
                )
                : 0;
        const currentPageFromServer = Number(pagination.current_page);
        const effectiveCurrent =
            totalRecords > 0
                ? Math.min(
                    Math.max(
                        Number.isFinite(currentPageFromServer)
                            ? currentPageFromServer
                            : currentPage,
                        1,
                    ),
                    totalPages,
                )
                : 1;

        if (
            Number.isFinite(pageSizeFromServer) &&
            pageSizeFromServer > 0 &&
            pageSize !== pageSizeFromServer
        ) {
            pageSize = pageSizeFromServer;
        }
        currentPage = effectiveCurrent;
        lastPaginationMeta = {
            totalRecords,
            totalPages,
            currentPage: effectiveCurrent,
        };

        const infoText =
            totalRecords > 0
                ? `第 ${effectiveCurrent} / ${totalPages} 页 · 共 ${totalRecords} 条`
                : '共 0 条记录';

        if (totalRecords === 0 || totalPages <= 1) {
            container.innerHTML = `<div class="pagination-info">${infoText}</div>`;
            container.style.display = 'flex';
            return;
        }

        const pageButtonsHtml = buildPageNumberButtons(
            effectiveCurrent,
            totalPages,
        );
        const prevDisabled = effectiveCurrent === 1;
        const nextDisabled = effectiveCurrent === totalPages;
        const prevPage = prevDisabled ? 1 : effectiveCurrent - 1;
        const nextPage = nextDisabled ? totalPages : effectiveCurrent + 1;

        const selectorHtml = `
        <div class="page-size-selector">
            <span>每页</span>
            <select onchange="changePageSize(this.value)" class="form-select form-select-sm">
                <option value="10" ${pageSize === 10 ? 'selected' : ''}>10</option>
                <option value="20" ${pageSize === 20 ? 'selected' : ''}>20</option>
                <option value="50" ${pageSize === 50 ? 'selected' : ''}>50</option>
                <option value="100" ${pageSize === 100 ? 'selected' : ''}>100</option>
            </select>
        </div>
    `;

        container.innerHTML = `
        <div class="pagination-left">
            ${selectorHtml}
            <div class="pagination-info">${infoText}</div>
        </div>
        <div class="pagination-controls">
            ${buildPaginationButton('首页', 1, prevDisabled)}
            ${buildPaginationButton('上一页', prevPage, prevDisabled)}
            ${pageButtonsHtml}
            ${buildPaginationButton('下一页', nextPage, nextDisabled)}
            ${buildPaginationButton('末页', totalPages, nextDisabled)}
        </div>
    `;
        container.style.display = 'flex';
    }

    function buildPageNumberButtons(current, totalPages) {
        const pages = new Set([current]);
        for (let offset = 1; offset <= 2; offset += 1) {
            const lower = current - offset;
            const upper = current + offset;
            if (lower >= 1) {
                pages.add(lower);
            }
            if (upper <= totalPages) {
                pages.add(upper);
            }
        }
        pages.add(1);
        pages.add(totalPages);

        const sortedPages = Array.from(pages).sort((a, b) => a - b);
        const parts = [];

        sortedPages.forEach((page, index) => {
            if (index > 0 && page - sortedPages[index - 1] > 1) {
                parts.push('<span class="pagination-ellipsis">...</span>');
            }
            parts.push(
                buildPaginationButton(
                    String(page),
                    page,
                    false,
                    page === current,
                    true,
                ),
            );
        });

        return parts.join('');
    }

    function buildPaginationButton(label, page, disabled = false, active = false, isNumber = false) {
        const safePage = Math.max(1, Math.floor(Number(page) || 1));
        const disabledAttr = disabled ? ' disabled' : '';
        const classes = ['pagination-btn'];
        if (isNumber) {
            classes.push('pagination-page');
        }
        if (active) {
            classes.push('active');
        }
        if (disabled) {
            classes.push('disabled');
        }

        return `<button class="${classes.join(' ')}" onclick="changePage(${safePage})"${disabledAttr}>${label}</button>`;
    }

    function changePage(targetPage) {
        const numericTarget = Math.floor(Number(targetPage));
        if (!Number.isFinite(numericTarget) || numericTarget < 1) {
            return;
        }

        const totalPages =
            lastPaginationMeta && lastPaginationMeta.totalPages
                ? lastPaginationMeta.totalPages
                : null;
        const normalizedTarget =
            totalPages && totalPages > 0
                ? Math.min(numericTarget, totalPages)
                : numericTarget;

        if (normalizedTarget === currentPage) {
            return;
        }

        currentPage = normalizedTarget;
        loadPreviewRequests();
    }

    function changePageSize(value) {
        const parsed = Number(value);
        if (!Number.isFinite(parsed) || parsed <= 0) {
            updatePageSizeSelector();
            return;
        }

        const normalized = Math.floor(parsed);
        if (normalized === pageSize) {
            updatePageSizeSelector();
            return;
        }

        pageSize = normalized;
        currentPage = 1;
        updatePageSizeSelector();
        loadPreviewRequests();
    }

    function maskSensitive(value, left = 3, right = 2) {
        if (!value && value !== 0) return '-';
        const str = String(value);
        if (str.length <= left + right) {
            return '*'.repeat(Math.max(str.length, 3));
        }
        const middle = '*'.repeat(str.length - left - right);
        return `${str.slice(0, left)}${middle}${str.slice(str.length - right)}`;
    }

    function formatCertificateNumber(value) {
        if (!value) return '-';
        return maskSensitive(value, 4, 4);
    }

    function formatPhoneNumber(value) {
        if (!value) return '-';
        return maskSensitive(value, 3, 4);
    }

    function formatStatus(status) {
        if (!status) return '<span class="status-badge">-</span>';
        const key = typeof status === 'string' ? status.toLowerCase() : status;
        const statusMap = {
            pending: '<span class="status-badge status-pending">待处理</span>',
            queued: '<span class="status-badge status-pending">排队中</span>',
            processing:
                '<span class="status-badge status-processing">处理中</span>',
            completed:
                '<span class="status-badge status-completed">正常</span>',
            failed: '<span class="status-badge status-failed">失败</span>',
        };
        return statusMap[key] || key;
    }

    function formatCallbackStatus(status) {
        if (!status) {
            return '<span class="status-badge">未触发</span>';
        }
        const key = status.toLowerCase();
        const map = {
            success: '<span class="status-badge status-completed">成功</span>',
            failed: '<span class="status-badge status-failed">失败</span>',
            retrying:
                '<span class="status-badge status-processing">重试中</span>',
            scheduled:
                '<span class="status-badge status-pending">待发送</span>',
        };
        return map[key] || status;
    }

    function formatCallbackStats(record) {
        if (!record) return '0/0/0';
        const success = record.callback_successes ?? 0;
        const failure = record.callback_failures ?? 0;
        const attempts = record.callback_attempts ?? success + failure;
        return `${success}/${failure}/${attempts}`;
    }

    function truncateText(value, maxLen = 120) {
        if (!value) return '';
        const str = String(value);
        if (str.length <= maxLen) return str;
        return `${str.slice(0, maxLen)}...`;
    }

    function escapeHtml(str = '') {
        return String(str)
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#39;');
    }

    function buildSlowAttachmentList(info) {
        if (!info) return '-';
        if (!Array.isArray(info)) {
            return escapeHtml(typeof info === 'string' ? info : JSON.stringify(info));
        }
        if (!info.length) return '-';
        return info
            .map((item) => {
                const code = item.material_code || '-';
                const idx =
                    typeof item.attachment_index === 'number'
                        ? item.attachment_index
                        : '-';
                const elapsed =
                    typeof item.elapsed_ms === 'number'
                        ? `${item.elapsed_ms}ms`
                        : '-';
                const outcome = item.outcome || '-';
                const source = item.source_url ? escapeHtml(item.source_url) : '-';
                return `${code} #${idx} · ${elapsed} · ${outcome}<br><span style="color:#888;font-size:12px;">${source}</span>`;
            })
            .join('<br>');
    }

    function formatCallbackResponse(attempt) {
        if (!attempt) return '-';
        const statusCode =
            attempt.last_callback_status_code !== undefined &&
                attempt.last_callback_status_code !== null
                ? attempt.last_callback_status_code
                : '-';
        const response = attempt.last_callback_response;
        if (!response) {
            return statusCode === '-' ? '-' : `状态码 ${statusCode}`;
        }
        const preview = truncateText(response, 160);
        return `状态码 ${statusCode} · ${escapeHtml(preview)}`;
    }

    function formatJsonPreview(payload) {
        if (!payload) return '-';
        try {
            const json =
                typeof payload === 'string'
                    ? JSON.parse(payload)
                    : payload;
            return escapeHtml(truncateText(JSON.stringify(json), 160));
        } catch (error) {
            return escapeHtml(truncateText(payload, 160));
        }
    }

    function sortJsonValue(value) {
        if (Array.isArray(value)) {
            return value.map(sortJsonValue);
        }
        if (value && typeof value === 'object') {
            const sorted = {};
            Object.keys(value)
                .sort((a, b) => a.localeCompare(b, 'zh-CN'))
                .forEach((key) => {
                    sorted[key] = sortJsonValue(value[key]);
                });
            return sorted;
        }
        return value;
    }

    function prettyPrintJson(value) {
        try {
            const parsed =
                typeof value === 'string' ? JSON.parse(value) : value;
            const sorted = sortJsonValue(parsed);
            return JSON.stringify(sorted, null, 2);
        } catch (error) {
            return typeof value === 'string'
                ? value
                : JSON.stringify(value, null, 2);
        }
    }

    function toArraySafe(value) {
        if (!value) return [];
        if (Array.isArray(value)) return value;
        if (typeof value === 'string') {
            try {
                const parsed = JSON.parse(value);
                return Array.isArray(parsed) ? parsed : [];
            } catch (error) {
                return [];
            }
        }
        return [];
    }

    function normalizeMaterialData(materials) {
        const items = toArraySafe(materials);
        return items.map((item) => {
            const material = item || {};
            const code =
                material.code ||
                material.materialCode ||
                material.material_code ||
                '-';
            const name =
                material.name ||
                material.materialName ||
                material.material_name ||
                '';
            const attachments =
                material.attachmentList ||
                material.attachment_list ||
                material.attachments ||
                [];
            const normalizedAttachments = toArraySafe(attachments).map(
                (attachment) => {
                    const att = attachment || {};
                    const attName =
                        att.attachName ||
                        att.attaName ||
                        att.name ||
                        '-';
                    const attUrl =
                        att.attachUrl ||
                        att.attaUrl ||
                        att.url ||
                        att.link ||
                        '';
                    const isCloud =
                        att.isCloudShare ||
                        att.cloudShare ||
                        att.is_cloud_share ||
                        false;
                    return {
                        name: attName,
                        url: attUrl,
                        isCloudShare: Boolean(isCloud),
                        extra: att.extra || att,
                    };
                },
            );
            return {
                code,
                name,
                raw: material,
                attachments: normalizedAttachments,
            };
        });
    }

    function buildMaterialDetails(materialData) {
        const materials = normalizeMaterialData(materialData);
        if (!materials.length) {
            return `
                <p style="padding:12px 0;color:#999;">
                    暂无原始材料信息
                </p>
            `;
        }

        const rows = materials
            .map((material, index) => {
                const titleParts = [];
                if (material.code && material.code !== '-') {
                    titleParts.push(escapeHtml(material.code));
                }
                if (material.name) {
                    titleParts.push(escapeHtml(material.name));
                }
                const headerText =
                    titleParts.length > 0
                        ? titleParts.join(' · ')
                        : `材料 ${index + 1}`;

                const attachments = material.attachments.length
                    ? material.attachments
                        .map((attachment, idx) => {
                            const normalizedUrl = normalizeInternalUrl(attachment.url);
                            const urlWithSession = appendMonitorSessionParam(normalizedUrl);
                            const linkText = escapeHtml(attachment.name || `附件 ${idx + 1}`);
                            if (urlWithSession) {
                                const finalHref = /^https?:\/\//i.test(urlWithSession)
                                    ? urlWithSession
                                    : buildApiUrl(urlWithSession);
                                const safeUrl = escapeHtml(finalHref);
                                return `<li>
                                        <a href="${safeUrl}" target="_blank" rel="noopener">
                                            ${linkText}
                                        </a>
                                        ${attachment.isCloudShare ? '<span style="font-size:11px;color:#fa8c16;margin-left:6px;">云分享</span>' : ''}
                                    </li>`;
                            }
                            return `<li>${linkText}</li>`;
                        })
                        .join('')
                    : '<li style="color:#999;">暂无附件</li>';

                const extraKeys = Object.keys(material.raw || {})
                    .filter(
                        (key) =>
                            ![
                                'code',
                                'name',
                                'materialCode',
                                'materialName',
                                'material_name',
                                'attachmentList',
                                'attachment_list',
                                'attachments',
                            ].includes(key),
                    )
                    .sort();

                const extraHtml = extraKeys.length
                    ? `<div class="material-extra" style="font-size:12px;color:#666;margin-left:4px;">
                            ${extraKeys
                        .map((key) => {
                            const value =
                                material.raw[key] !== undefined
                                    ? material.raw[key]
                                    : '';
                            return `<div style="margin:2px 0;"><span class="material-extra-key" style="color:#999;">${escapeHtml(
                                key,
                            )}</span>: ${escapeHtml(
                                typeof value === 'object'
                                    ? truncateText(
                                        JSON.stringify(value),
                                        120,
                                    )
                                    : String(value ?? ''),
                            )}</div>`;
                        })
                        .join('')}
                        </div>`
                    : '';

                return `
                    <li class="material-item" style="margin-bottom:12px;">
                        <div class="material-header" style="font-weight:600;margin-bottom:4px;">${headerText}</div>
                        <ul class="material-attachments" style="margin:4px 0 6px 18px;padding:0;list-style:disc;">${attachments}</ul>
                        ${extraHtml}
                    </li>
                `;
            })
            .join('');

        return `
            <ul class="material-list" style="margin:0;padding-left:18px;">
                ${rows}
            </ul>
        `;
    }

    function buildRequestDetailHtml(detail, rawText) {
        if (!detail) {
            return '<p style="padding:16px;color:#999;">暂无详情数据</p>';
        }

        const request = detail.request || {};
        const userInfo = request.user_info || {};
        const attempts = detail.attempts || [];
        const latestAttempt = attempts.length
            ? attempts[attempts.length - 1]
            : {};
        const slowAttachmentHtml = buildSlowAttachmentList(
            latestAttempt.slow_attachment_info,
        );
        const callbackStatus = formatCallbackStatus(
            latestAttempt.callback_status,
        );
        const callbackStats = formatCallbackStats(latestAttempt);
        const callbackResponse = formatCallbackResponse(latestAttempt);
        const callbackPayloadPreview = formatJsonPreview(
            latestAttempt.callback_payload_parsed ||
            latestAttempt.callback_payload,
        );

        const requestInfo = [
            ['请求ID', request.request_id || '-'],
            ['第三方请求ID', request.third_party_request_id || '-'],
            ['最新预审ID', request.latest_preview_id || '-'],
            ['用户ID', request.user_id || '-'],
            ['用户姓名', escapeHtml(userInfo.user_name || '-')],
            ['证件号', formatCertificateNumber(userInfo.certificate_number)],
            ['手机号', formatPhoneNumber(userInfo.phone_number)],
            ['事项名称', request.matter_name || '-'],
            ['事项ID', request.matter_id || '-'],
            ['渠道', request.channel || '-'],
            ['流水号', request.sequence_no || '-'],
            ['当前状态', formatStatus(request.latest_status)],
            ['回调状态', callbackStatus],
            ['回调统计 (成功/失败/总)', callbackStats],
            ['最后回调时间', formatDateTime(latestAttempt.last_callback_at)],
            ['最后回调响应', callbackResponse || '-'],
            ['失败原因', latestAttempt.failure_reason || '-'],
            ['错误码', latestAttempt.last_error_code || '-'],
            ['OCR stderr 摘要', latestAttempt.ocr_stderr_summary || '-'],
            ['慢附件', slowAttachmentHtml || '-'],
            ['回调请求体', callbackPayloadPreview || '-'],
            ['创建时间', formatDateTime(request.created_at)],
            ['更新时间', formatDateTime(request.updated_at)],
        ];

        const requestRows = requestInfo
            .map(
                ([label, value]) =>
                    `<tr><th>${label}</th><td>${value ?? '-'}</td></tr>`,
            )
            .join('');

        const attemptRows = attempts.length
            ? attempts
                .map((attempt) => {
                    const statusHtml = formatStatus(attempt.status);
                    const callbackStatusHtml = formatCallbackStatus(
                        attempt.callback_status,
                    );
                    const callbackStatsText = formatCallbackStats(attempt);
                    const failureReasonShort = attempt.failure_reason
                        ? escapeHtml(
                            truncateText(attempt.failure_reason, 80),
                        )
                        : '-';
                    const failureReasonFull = attempt.failure_reason
                        ? escapeHtml(attempt.failure_reason)
                        : '-';
                    return `
                        <tr>
                            <td>${attempt.attempt_no || '-'}</td>
                            <td>${attempt.id || '-'}</td>
                            <td>${statusHtml}</td>
                            <td>${callbackStatusHtml}</td>
                            <td title="成功/失败/总">${callbackStatsText}</td>
                            <td>${attempt.last_error_code || '-'}</td>
                            <td title="${failureReasonFull}">${failureReasonShort}</td>
                            <td>${formatDateTime(attempt.created_at)}</td>
                            <td>${formatDateTime(attempt.updated_at)}</td>
                            <td>${formatDateTime(attempt.queued_at)}</td>
                            <td>${formatDateTime(attempt.processing_started_at)}</td>
                            <td>${attempt.retry_count ?? '-'}</td>
                            <td>${attempt.last_worker_id || '-'}</td>
                        </tr>
                    `;
                })
                .join('')
            : '<tr><td colspan="13" style="text-align:center;color:#999;">暂无历史尝试</td></tr>';

        const rawSection = rawText
            ? (() => {
                const formatted = prettyPrintJson(rawText);
                return `
            <h3 class="detail-section-title">原始返回</h3>
            <pre class="detail-raw">${escapeHtml(formatted)}</pre>
        `;
            })()
            : '';

        return `
            <h3 class="detail-section-title">请求信息</h3>
            <table class="detail-table">
                <tbody>${requestRows}</tbody>
            </table>
            <h3 class="detail-section-title">预审尝试历史</h3>
            <table class="detail-table">
                <thead>
                    <tr>
                        <th>#</th>
                        <th>预审ID</th>
                        <th>状态</th>
                        <th>回调状态</th>
                        <th>回调统计</th>
                        <th>错误码</th>
                        <th>失败原因</th>
                        <th>创建时间</th>
                        <th>更新时间</th>
                        <th>入队时间</th>
                        <th>开始处理</th>
                        <th>重试次数</th>
                        <th>最后 Worker</th>
                  </tr>
                </thead>
                <tbody>${attemptRows}</tbody>
            </table>
            <h3 class="detail-section-title">原始材料列表</h3>
            ${buildMaterialDetails(request.material_data)}
            ${rawSection}
        `;
    }

    function renderRecentFailures(records) {
        const container = document.getElementById('recentFailuresContent');
        if (!container) return;

        if (!records.length) {
            container.innerHTML =
                '<p style="padding:16px;color:#999;">最近暂无失败任务</p>';
            return;
        }

        const rows = records
            .map((record) => {
                const previewId = record.id || '-';
                const matterName = h(record.matter_name || '-');
                const userId = h(record.user_id || '-');
                const userName =
                    record.user_info?.user_name || record.user_name || '-';
                const userDisplay = `${userId} / ${h(userName)}`;
                const errorCode = h(record.last_error_code || '-');
                const failureReason = record.failure_reason
                    ? escapeHtml(truncateText(record.failure_reason, 120))
                    : '-';
                const callbackStatus = formatCallbackStatus(record.callback_status);
                const createdAt = formatDateTime(record.created_at);
                const updatedAt = formatDateTime(record.updated_at);
                const viewUrl =
                    record.preview_view_url ||
                    (previewId ? `/api/preview/view/${previewId}` : '');
                const downloadUrl =
                    record.preview_download_url ||
                    (previewId
                        ? `/api/preview/download/${previewId}`
                        : '');
                const encodedViewUrl = encodeURIComponent(viewUrl || '');
                const encodedDownloadUrl = encodeURIComponent(
                    downloadUrl || '',
                );

                const viewBtn =
                    previewId && viewUrl
                        ? `<button class="btn btn-secondary btn-small" onclick="viewPreview('${previewId}', '${encodedViewUrl}')">查看</button>`
                        : '<button class="btn btn-secondary btn-small" disabled>查看</button>';
                const downloadBtn =
                    previewId && downloadUrl
                        ? `<button class="btn btn-secondary btn-small" onclick="downloadResult('${previewId}', '${encodedDownloadUrl}')">下载</button>`
                        : '<button class="btn btn-secondary btn-small" disabled>下载</button>';

                return `
                    <tr>
                        <td><span class="text-clip" title="${previewId}">${previewId}</span></td>
                        <td><span class="text-clip" title="${record.matter_name || '-'}">${matterName}</span></td>
                        <td><span class="text-clip" title="${record.user_id || ''}${userName ? ' / ' + userName : ''}">${userDisplay}</span></td>
                        <td><span class="text-clip" title="${record.last_error_code || '-'}">${errorCode}</span></td>
                        <td title="${record.failure_reason ? escapeHtml(record.failure_reason) : '-'}">${failureReason}</td>
                        <td>${callbackStatus}</td>
                        <td>${createdAt}</td>
                        <td>${updatedAt}</td>
                        <td>
                            <div class="user-actions">
                                ${viewBtn}
                                ${downloadBtn}
                            </div>
                        </td>
                    </tr>
                `;
            })
            .join('');

        container.innerHTML = `
            <table class="data-table">
                <thead>
                    <tr>
                        <th>预审ID</th>
                        <th>事项</th>
                        <th>用户</th>
                        <th>错误码</th>
                        <th>失败原因</th>
                        <th>回调状态</th>
                        <th>创建时间</th>
                        <th>更新时间</th>
                        <th>操作</th>
                    </tr>
                </thead>
                <tbody>${rows}</tbody>
            </table>
        `;
    }

    async function showRequestDetail(requestId) {
        if (!requestId) return;
        const overlay = document.getElementById('requestDetailModal');
        const content = document.getElementById('requestDetailContent');
        if (!overlay || !content) return;

        overlay.style.display = 'flex';
        document.body.classList.add('modal-open');
        content.innerHTML = '<div class="loading">正在加载详情...</div>';

        try {
            const response = await apiFetch(
                `/api/preview/requests/${encodeURIComponent(requestId)}`,
            );
            const text = await response.text();
            if (response.ok) {
                try {
                    const data = JSON.parse(text);
                    if (data.success) {
                        content.innerHTML = buildRequestDetailHtml(data.data, text);
                    } else {
                        content.innerHTML = `<p style="padding:16px;color:#f5222d;">加载失败：${data.errorMsg || '未知错误'
                            }</p>`;
                    }
                } catch (parseError) {
                    content.innerHTML = `
                        <div style="padding:16px;">
                            <p style="color:#f5222d;">返回数据无法解析为JSON。</p>
                            <pre class="detail-raw">${escapeHtml(text)}</pre>
                        </div>`;
                }
            } else {
                content.innerHTML = `
                    <div style="padding:16px;">
                        <p style="color:#f5222d;">加载失败（HTTP ${response.status}）。</p>
                        <pre class="detail-raw">${escapeHtml(text)}</pre>
                    </div>`;
            }
        } catch (error) {
            console.error('加载预审请求详情失败:', error);
            content.innerHTML =
                '<p style="padding:16px;color:#f5222d;">加载失败，请稍后重试</p>';
        }
    }

    function closeRequestDetail() {
        const overlay = document.getElementById('requestDetailModal');
        const content = document.getElementById('requestDetailContent');
        if (!overlay || !content) return;
        overlay.style.display = 'none';
        content.innerHTML = '';
        document.body.classList.remove('modal-open');
    }

    function formatDateTime(dateTime) {
        if (!dateTime) return '-';
        return new Date(dateTime).toLocaleString('zh-CN');
    }

    function applyFilters() {
        currentFilters = {
            date_from: document.getElementById('dateFrom')?.value,
            date_to: document.getElementById('dateTo')?.value,
            status: document.getElementById('statusFilter')?.value,
            search: document.getElementById('matterFilter')?.value,
        };

        Object.keys(currentFilters).forEach((key) => {
            if (!currentFilters[key]) delete currentFilters[key];
        });

        currentPage = 1;
        loadPreviewRequests();
        loadStatistics();
    }

    function resetFilters() {
        const dateFrom = document.getElementById('dateFrom');
        const dateTo = document.getElementById('dateTo');
        const statusFilter = document.getElementById('statusFilter');
        const matterFilter = document.getElementById('matterFilter');

        const { from, to } = getDefaultDateRange();
        if (dateFrom) dateFrom.value = from;
        if (dateTo) dateTo.value = to;
        if (statusFilter) statusFilter.value = '';
        if (matterFilter) matterFilter.value = '';

        currentFilters = {
            date_from: from,
            date_to: to,
        };
        currentPage = 1;
        loadPreviewRequests();
        loadStatistics();
        loadRecentFailures();
    }

    function refreshBusinessData() {
        loadBusinessData();
    }

    async function loadRecentFailures() {
        const container = document.getElementById('recentFailuresContent');
        if (!container) return;

        container.innerHTML = '<div class="loading">正在加载失败任务...</div>';

        const hoursSelect = document.getElementById('recentFailureHours');
        const hours =
            hoursSelect && hoursSelect.value
                ? hoursSelect.value
                : '24';

        try {
            const params = new URLSearchParams({
                limit: '20',
                hours,
            });
            const response = await apiFetch(
                `/api/preview/failures?${params.toString()}`,
            );
            if (response.ok) {
                const data = await response.json();
                if (data.success) {
                    renderRecentFailures(data.data?.records || []);
                } else {
                    container.innerHTML = `<p style="color:#f5222d;padding:16px;">加载失败：${data.errorMsg || '未知错误'
                        }</p>`;
                }
            } else {
                container.innerHTML =
                    '<p style="color:#f5222d;padding:16px;">加载失败，请稍后重试</p>';
            }
        } catch (error) {
            console.error('加载失败任务列表失败:', error);
            container.innerHTML =
                '<p style="color:#f5222d;padding:16px;">加载失败，请稍后重试</p>';
        }
    }

    function fetchFailoverStatus(options) {
        return runSingleFlight(
            'failover-status',
            () => safeJsonRequest('/api/failover/status'),
            options,
        );
    }

    function fetchQueueStatus() {
        return safeJsonRequest('/api/queue/status');
    }

    function fetchHealthDetails() {
        return safeJsonRequest('/api/health/details');
    }

    async function loadEnhancedMonitoring() {
        try {
            const [failoverResult, queueResult, healthResult] =
                await Promise.allSettled([
                    fetchFailoverStatus(),
                    fetchQueueStatus(),
                    fetchHealthDetails(),
                ]);

            if (
                failoverResult.status === 'fulfilled' &&
                failoverResult.value
            ) {
                updateFailoverCard(failoverResult.value);
                updateFailoverDetails(failoverResult.value);
            } else if (failoverResult.status === 'rejected') {
                console.error(
                    '获取故障转移数据失败:',
                    failoverResult.reason,
                );
            }

            if (queueResult.status === 'fulfilled' && queueResult.value) {
                updateOCRPoolCard(queueResult.value);
                updateConcurrencyCard(queueResult.value);
            } else if (queueResult.status === 'rejected') {
                console.error('获取队列状态失败:', queueResult.reason);
            }

            if (healthResult.status === 'fulfilled' && healthResult.value) {
                updateHealthCard(healthResult.value);
            } else if (healthResult.status === 'rejected') {
                console.error('获取健康检查失败:', healthResult.reason);
            }
        } catch (error) {
            console.error('加载增强监控数据失败:', error);
        }
    }

    function updateFailoverCard(data = {}) {
        const payload = data.data || data;
        const dbStatus =
            payload.database?.state ||
            payload.database?.current_state ||
            'unknown';
        const storageStatus =
            payload.storage?.state ||
            payload.storage?.current_state ||
            'unknown';

        let statusText = '正常';
        const isFallback = (s) =>
            s === 'fallback' || s === '备用数据库' || s === '本地存储';
        const isRecovering = (s) => s === 'recovering' || s === '恢复中';

        if (isFallback(dbStatus) || isFallback(storageStatus)) {
            statusText = '降级运行';
        } else if (isRecovering(dbStatus) || isRecovering(storageStatus)) {
            statusText = '恢复中';
        }

        setText('failoverStatus', statusText);
        setText(
            'dbStatus',
            isFallback(dbStatus)
                ? 'SQLite'
                : isRecovering(dbStatus)
                    ? '恢复中'
                    : '主库',
        );
        setText(
            'storageStatus',
            isFallback(storageStatus)
                ? '本地'
                : isRecovering(storageStatus)
                    ? '恢复中'
                    : 'OSS',
        );
    }

    function updateOCRPoolCard(data = {}) {
        const activeTasks = data.active_tasks || 0;
        const maxTasks = data.max_concurrent_tasks || 12;
        setText('ocrPoolStatus', `${activeTasks}/${maxTasks}`);
        setText('ocrRestarts', '0');
    }

    function updateConcurrencyCard(data = {}) {
        const activeTasks = data.active_tasks || 0;
        const maxTasks = data.max_concurrent_tasks || 12;
        const rate = maxTasks ? ((activeTasks / maxTasks) * 100).toFixed(0) : 0;
        setText('concurrencyRate', `${rate}%`);
        setText('activeTasks', activeTasks);
    }

    function updateHealthCard(data = {}) {
        const isHealthy = data.overall_status === 'healthy';
        setText('healthScore', isHealthy ? '100%' : '75%');
        setText('healthStatus', isHealthy ? '正常' : '异常');
    }

    function normalizeInternalUrl(url) {
        if (!url) return '';
        if (!/^https?:\/\//i.test(url)) {
            return url;
        }
        try {
            const resolved = new URL(url);
            if (resolved.hostname === '0.0.0.0' || resolved.hostname === '127.0.0.1') {
                resolved.protocol = window.location.protocol;
                resolved.host = window.location.host;
                return resolved.toString();
            }
        } catch (error) {
            console.warn('规范化内部URL失败:', error);
        }
        return url;
    }

    function appendMonitorSessionParam(target) {
        if (!authSession || !target) return target;

        const isRelative = !/^https?:\/\//i.test(target);

        try {
            const parsed = new URL(target, window.location.origin);
            // 允许跨域 API 调用携带 session_id
            // 只要 URL 有效，我们就尝试附加 session_id
            if (!parsed.searchParams.has('monitor_session_id')) {
                parsed.searchParams.set(
                    'monitor_session_id',
                    authSession,
                );
            }
            if (isRelative) {
                return `${parsed.pathname}${parsed.search}${parsed.hash}`;
            }
            return parsed.toString();
        } catch (error) {
            if (isRelative) {
                return target + (target.includes('?') ? '&' : '?') +
                    `monitor_session_id=${encodeURIComponent(authSession)}`;
            }
        }
        return target;
    }

    function viewPreview(previewId, encodedUrl) {
        let target = '';

        if (encodedUrl) {
            try {
                target = decodeURIComponent(encodedUrl);
            } catch (error) {
                console.warn('解码预览链接失败:', error);
            }
        }

        if (!target) {
            if (!previewId) {
                showToast('暂无可查看的预审结果', 'warning');
                return;
            }
            target = `/api/preview/view/${previewId}`;
        }

        target = normalizeInternalUrl(target);
        target = appendMonitorSessionParam(target);

        if (/^https?:\/\//i.test(target)) {
            window.open(target, '_blank');
        } else {
            window.open(buildApiUrl(target), '_blank');
        }
    }

    async function downloadResult(previewId, encodedUrl) {
        let target = '';
        if (encodedUrl) {
            try {
                target = decodeURIComponent(encodedUrl);
            } catch (error) {
                console.warn('解码下载链接失败:', error);
            }
        }
        if (!target) {
            if (!previewId) {
                showToast('暂无可下载的预审结果', 'warning');
                return;
            }
            target = `/api/preview/download/${previewId}?format=pdf`;
        }

        const normalized = normalizeInternalUrl(target);
        const withSession = appendMonitorSessionParam(normalized);

        try {
            const response = await apiFetch(withSession, {
                method: 'GET',
                credentials: 'include',
            });
            if (!response.ok) {
                showToast(`下载失败 (${response.status})`, 'error');
                return;
            }

            const blob = await response.blob();
            const objectUrl = URL.createObjectURL(blob);
            const disposition =
                response.headers.get('content-disposition') || '';
            const match = disposition.match(/filename\*?=([^;]+)/i);
            const rawName = match
                ? decodeURIComponent(
                    match[1].replace(/(^UTF-8''|\"|')/g, ''),
                )
                : `${previewId || 'preview'}.pdf`;

            const link = document.createElement('a');
            link.href = objectUrl;
            link.download = rawName;
            document.body.appendChild(link);
            link.click();
            link.remove();
            setTimeout(() => URL.revokeObjectURL(objectUrl), 500);
            showToast('开始下载，请查看浏览器进度', 'success');
        } catch (error) {
            console.error('下载失败:', error);
            showToast('下载失败，请稍后重试', 'error');
        }
    }

    function exportBusinessData() {
        showToast('导出功能开发中，敬请期待', 'info');
    }

    function exportSystemData() {
        showToast('系统数据导出功能开发中', 'info');
    }

    async function loadSystemData() {
        try {
            // 修复：使用正确的API端点
            const response = await apiFetch('/api/resources/status');
            if (response.ok) {
                const data = await response.json();
                renderSystemStatus(data);
            }
        } catch (error) {
            console.error('加载系统监控数据失败:', error);
        }
    }

    function renderSystemStatus(data = {}) {
        // 修复：使用正确的数据路径
        const systemRes = data.data?.system_resources;
        if (!systemRes) {
            console.warn('系统资源数据不可用');
            return;
        }

        setText('cpuUsage', `${systemRes.cpu_usage_percent?.toFixed(1) ?? '-'}%`);
        setText('memoryUsage', `${systemRes.memory_usage_percent?.toFixed(1) ?? '-'}%`);
        setText('diskUsage', `${systemRes.disk_usage_percent?.toFixed(1) ?? '-'}%`);

        // OCR状态从ocr_pool获取
        const ocrPool = data.data?.ocr_pool;
        const ocrCapacity = ocrPool?.capacity ?? 0;
        const ocrAvailable = ocrPool?.available ?? 0;
        const ocrStatus = ocrPool
            ? `${ocrAvailable}/${ocrCapacity || '-'} 可用${ocrPool.circuit_open ? ' (熔断)' : ''}`
            : '未上报';
        setText('ocrStatus', ocrStatus);
        const restartHint = ocrPool
            ? `重启 ${ocrPool.total_restarted ?? 0} 次 · 失败 ${ocrPool.total_failures ?? 0} 次`
            : '等待上报';
        setText('ocrRestartsHint', restartHint);

        const watchdogStates = systemRes.watchdog_states || [];
        const activeAlert = watchdogStates.find(
            (state) => (state.consecutive_violations || 0) > 0,
        );
        const latestState = watchdogStates[0];
        const watchdogStatus = !watchdogStates.length
            ? '未启用'
            : activeAlert
                ? '告警'
                : '正常';
        setText('watchdogStatus', watchdogStatus);
        let watchdogHint = '等待上报';
        if (activeAlert) {
            watchdogHint = `连续 ${activeAlert.consecutive_violations} 次异常 · CPU ${activeAlert.cpu_percent?.toFixed?.(1) ?? '-'
                }% / 内存 ${activeAlert.memory_percent?.toFixed?.(1) ?? '-'}%`;
        } else if (latestState) {
            const checkedAt = latestState.last_checked_at
                ? formatDateTime(latestState.last_checked_at)
                : '-';
            watchdogHint = `最近巡检 ${checkedAt}`;
        }
        setText('watchdogHint', watchdogHint);

        // 渲染详细的系统监控表格
        updateSystemCharts(systemRes);
        renderSystemDetails(data.data);
    }

    function updateSystemCharts(systemRes) {
        if (!systemRes || typeof window.MonitorCharts === 'undefined') {
            return;
        }

        try {
            const cpu = systemRes.cpu_usage_percent || 0;
            const memory = systemRes.memory_usage_percent || 0;
            const disk = systemRes.disk_usage_percent || 0;

            if (typeof window.MonitorCharts.updateResourceTrend === 'function') {
                window.MonitorCharts.updateResourceTrend(cpu, memory, disk);
            }
        } catch (error) {
            console.error('更新系统图表失败:', error);
        }
    }

    function renderSystemDetails(data = {}) {
        const container = document.getElementById('systemTableContent');
        if (!container) return;

        const systemRes = data.system_resources;
        const multiStage = data.multi_stage_status;
        const tracingStatus = data.tracing_status;
        const workerHeartbeats = data.worker_heartbeats || [];
        const workerSummary = data.worker_summary || {};
        const watchdogStates = systemRes?.watchdog_states || [];

        if (!systemRes) {
            container.innerHTML = '<div class="loading">系统数据不可用</div>';
            return;
        }

        // 构建详细信息HTML
        const workerRows = workerHeartbeats
            .map((worker) => {
                const metrics = worker.metrics || {};
                const cpu = metrics.cpu_percent !== undefined && metrics.cpu_percent !== null
                    ? `${metrics.cpu_percent.toFixed(1)}%`
                    : '-';
                const memory = metrics.memory_mb !== undefined && metrics.memory_mb !== null
                    ? `${metrics.memory_mb} MB`
                    : '-';
                const disk = metrics.disk_percent !== undefined && metrics.disk_percent !== null
                    ? `${metrics.disk_percent.toFixed(1)}%`
                    : '-';
                const load = metrics.load_1min !== undefined && metrics.load_1min !== null
                    ? metrics.load_1min.toFixed(2)
                    : '-';
                const statusBadge =
                    worker.status === 'ok'
                        ? '<span class="status-badge success">正常</span>'
                        : worker.status === 'missing'
                            ? '<span class="status-badge error">离线</span>'
                            : '<span class="status-badge warning">超时</span>';
                return `
                    <tr>
                        <td>${worker.worker_id}</td>
                        <td>${statusBadge}</td>
                        <td>${cpu}</td>
                        <td>${memory}</td>
                        <td>${disk}</td>
                        <td>${load}</td>
                        <td>${worker.queue_depth ?? '-'}</td>
                        <td>${worker.running_tasks?.length ?? 0}</td>
                    </tr>
                `;
            })
            .join('');

        const workerSummaryLine = workerSummary.total
            ? `<div class="worker-summary">
                    <span>总数: ${workerSummary.total}</span>
                    <span>正常: ${workerSummary.ok}</span>
                    <span>超时: ${workerSummary.timeout}</span>
                    <span>离线: ${workerSummary.missing}</span>
               </div>`
            : '';

        const workerSection = workerRows
            ? `
                <div class="stats-section wide-section">
                    <h3>Worker 节点</h3>
                    ${workerSummaryLine}
                    <table class="info-table worker-table">
                        <thead>
                            <tr>
                                <th>节点</th>
                                <th>状态</th>
                                <th>CPU</th>
                                <th>内存</th>
                                <th>磁盘</th>
                                <th>1min负载</th>
                                <th>队列</th>
                                <th>运行任务</th>
                            </tr>
                        </thead>
                        <tbody>
                            ${workerRows}
                        </tbody>
                    </table>
                </div>
            `
            : '';

        const watchdogRows = watchdogStates
            .map((state) => {
                const violation = state.consecutive_violations ?? 0;
                const statusBadge = violation > 0
                    ? '<span class="status-badge error">告警</span>'
                    : '<span class="status-badge success">正常</span>';
                const lastChecked = state.last_checked_at
                    ? formatDateTime(state.last_checked_at)
                    : '-';
                const lastViolation = state.last_violation_at
                    ? formatDateTime(state.last_violation_at)
                    : '-';
                const lastRestart = state.last_restart_trigger_at
                    ? formatDateTime(state.last_restart_trigger_at)
                    : '-';
                return `
                    <tr>
                        <td>${state.role}</td>
                        <td>${statusBadge}</td>
                        <td>${state.cpu_percent?.toFixed?.(1) ?? '-'}%</td>
                        <td>${state.memory_percent?.toFixed?.(1) ?? '-'}%</td>
                        <td>${state.disk_percent?.toFixed?.(1) ?? '-'}%</td>
                        <td>${violation}</td>
                        <td>${lastViolation}</td>
                        <td>${lastChecked}</td>
                        <td>${lastRestart}</td>
                    </tr>
                `;
            })
            .join('');

        const watchdogSection = watchdogRows
            ? `
                <div class="stats-section wide-section">
                    <h3>Watchdog 详情</h3>
                    <table class="info-table worker-table">
                        <thead>
                            <tr>
                                <th>角色</th>
                                <th>状态</th>
                                <th>CPU</th>
                                <th>内存</th>
                                <th>磁盘</th>
                                <th>连续告警</th>
                                <th>最后告警</th>
                                <th>最后巡检</th>
                                <th>最后触发</th>
                            </tr>
                        </thead>
                        <tbody>
                            ${watchdogRows}
                        </tbody>
                    </table>
                </div>
            `
            : '';

        const html = `
            <div class="stats-grid">
                <div class="stats-section">
                    <h3>系统资源</h3>
                    <table class="info-table">
                        <tr><td>CPU使用率</td><td>${systemRes.cpu_usage_percent?.toFixed(1) ?? '-'}%</td></tr>
                        <tr><td>内存使用率</td><td>${systemRes.memory_usage_percent?.toFixed(1) ?? '-'}%</td></tr>
                        <tr><td>可用内存</td><td>${systemRes.available_memory_mb ?? '-'} MB</td></tr>
                        <tr><td>磁盘使用率</td><td>${systemRes.disk_usage_percent?.toFixed(1) ?? '-'}%</td></tr>
                    </table>
                </div>

                <div class="stats-section">
                    <h3>系统负载</h3>
                    <table class="info-table">
                        <tr><td>1分钟负载</td><td>${systemRes.system_load?.load_1min?.toFixed(2) ?? '-'}</td></tr>
                        <tr><td>5分钟负载</td><td>${systemRes.system_load?.load_5min?.toFixed(2) ?? '-'}</td></tr>
                        <tr><td>15分钟负载</td><td>${systemRes.system_load?.load_15min?.toFixed(2) ?? '-'}</td></tr>
                    </table>
                </div>

                <div class="stats-section">
                    <h3>并发处理</h3>
                    <table class="info-table">
                        <tr><td>下载并发</td><td>${multiStage?.stage_concurrency?.download?.active_tasks ?? 0}/${multiStage?.stage_concurrency?.download?.max_concurrency ?? '-'}</td></tr>
                        <tr><td>PDF转换</td><td>${multiStage?.stage_concurrency?.pdf_convert?.active_tasks ?? 0}/${multiStage?.stage_concurrency?.pdf_convert?.max_concurrency ?? '-'}</td></tr>
                        <tr><td>OCR处理</td><td>${multiStage?.stage_concurrency?.ocr_process?.active_tasks ?? 0}/${multiStage?.stage_concurrency?.ocr_process?.max_concurrency ?? '-'}</td></tr>
                        <tr><td>存储操作</td><td>${multiStage?.stage_concurrency?.storage?.active_tasks ?? 0}/${multiStage?.stage_concurrency?.storage?.max_concurrency ?? '-'}</td></tr>
                    </table>
                </div>

                <div class="stats-section">
                    <h3>链路追踪</h3>
                    <table class="info-table">
                        <tr><td>追踪状态</td><td>${tracingStatus?.enabled ? '已启用' : '未启用'}</td></tr>
                        <tr><td>活跃追踪</td><td>${tracingStatus?.active_traces ?? 0}</td></tr>
                        <tr><td>已完成追踪</td><td>${tracingStatus?.completed_traces ?? 0}</td></tr>
                        <tr><td>平均耗时</td><td>${tracingStatus?.avg_trace_duration_ms?.toFixed(1) ?? '-'} ms</td></tr>
                    </table>
                </div>

                ${workerSection || ''}
                ${watchdogSection || ''}
            </div>
            <style>
                .stats-grid {
                    display: grid;
                    grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
                    gap: 16px;
                    margin-top: 16px;
                    align-items: start;
                }
                .stats-section {
                    background: white;
                    border-radius: 10px;
                    padding: 14px;
                    box-shadow: 0 6px 18px rgba(0,0,0,0.06);
                    border: 1px solid #f0f0f0;
                    display: flex;
                    flex-direction: column;
                    gap: 8px;
                }
                .stats-section h3 {
                    margin: 0 0 10px 0;
                    color: #1f2d3d;
                    font-size: 15px;
                    font-weight: 600;
                    border-bottom: 1px solid #e9eef3;
                    padding-bottom: 8px;
                }
                .info-table {
                    width: 100%;
                    border-collapse: collapse;
                }
                .info-table td {
                    padding: 6px 4px;
                    border-bottom: 1px dashed #f0f0f0;
                    font-size: 13px;
                }
                .info-table tr:last-child td {
                    border-bottom: none;
                }
                .info-table td:first-child {
                    color: #667085;
                    width: 55%;
                }
                .info-table td:last-child {
                    font-weight: 600;
                    text-align: right;
                }
                .worker-summary {
                    display: flex;
                    gap: 12px;
                    font-size: 13px;
                    color: #555;
                    margin-bottom: 8px;
                    flex-wrap: wrap;
                }
                .wide-section {
                    grid-column: 1 / -1;
                }
                .stats-section .info-table,
                .stats-section .worker-table {
                    width: 100%;
                    table-layout: fixed;
                }
                .worker-table th,
                .worker-table td {
                    font-size: 13px;
                    white-space: nowrap;
                }
                .worker-table td:nth-child(1) {
                    max-width: 160px;
                    word-break: break-all;
                    white-space: normal;
                }
                @media (max-width: 1024px) {
                    .stats-grid {
                        grid-template-columns: 1fr;
                    }
                    .wide-section {
                        grid-column: auto;
                    }
                }
            </style>
        `;

        container.innerHTML = html;
    }

    async function loadMonitorUsers() {
        if (!authSession) return;

        try {
            const response = await apiFetch('/api/monitor/users');
            const data = await response.json().catch(() => null);
            if (response.status === 403) {
                // 无权限查看用户列表时，不要清空会话
                showToast('当前角色无权限查看用户列表', 'warning');
                monitorUsersCache = [];
                renderMonitorUsers([]);
                return;
            }
            if (!response.ok) {
                const message =
                    data?.message || '加载用户列表失败，请重新登录';
                handleSessionInvalid(message);
                monitorUsersCache = [];
                renderMonitorUsers([]);
                return;
            }
            if (data?.success) {
                monitorUsersCache = data.data || [];
                renderMonitorUsers(monitorUsersCache);
            } else {
                const message =
                    data?.message || '加载用户列表失败，请重新登录';
                handleSessionInvalid(message);
                monitorUsersCache = [];
                renderMonitorUsers([]);
            }
        } catch (error) {
            console.error('加载监控用户失败:', error);
            handleSessionInvalid('加载用户列表失败，请重新登录');
            monitorUsersCache = [];
            renderMonitorUsers([]);
        }
        toggleUserManagementVisibility();
    }

    function renderMonitorUsers(users) {
        const container = document.getElementById('monitorUsersContent');
        if (!container) return;

        if (!users || users.length === 0) {
            container.innerHTML =
                '<p style="text-align:center;padding:24px;color:#666;">暂无监控用户数据</p>';
            return;
        }

        const admin = isCurrentUserAdmin();
        const rows = users
            .map((user) => {
                const statusBadge = user.is_active
                    ? '<span class="status-badge success">启用</span>'
                    : '<span class="status-badge error">禁用</span>';
                const roleSelect = `
                    <select onchange="changeMonitorUserRole('${user.id}', this.value)">
                        <option value="ops_admin" ${user.role === 'ops_admin' ? 'selected' : ''
                    }>运维管理员</option>
                        <option value="admin" ${user.role === 'admin' ? 'selected' : ''
                    }>系统管理员</option>
                    </select>
                `;
                const toggleBtn = user.is_active
                    ? `<button class="btn btn-secondary btn-small" onclick="toggleMonitorUserStatus('${user.id}', false)">禁用</button>`
                    : `<button class="btn btn-primary btn-small" onclick="toggleMonitorUserStatus('${user.id}', true)">启用</button>`;

                const roleCell = admin
                    ? roleSelect
                    : user.role === 'admin'
                        ? '系统管理员'
                        : '运维管理员';

                const actionCell = admin
                    ? `<button class="btn btn-secondary btn-small" onclick="resetMonitorUserPassword('${user.id}')">重置密码</button>${toggleBtn}`
                    : '<span style="color:#999;">无权限</span>';

                return `
                    <tr>
                        <td>${user.username}</td>
                        <td>${statusBadge}</td>
                        <td>${user.last_login_at || '-'}</td>
                        <td>${user.login_count}</td>
                        <td>${roleCell}</td>
                        <td class="user-actions">${actionCell}</td>
                    </tr>
                `;
            })
            .join('');

        container.innerHTML = `
            <table class="data-table">
                <thead>
                    <tr>
                        <th>用户名</th>
                        <th>状态</th>
                        <th>最近登录</th>
                        <th>登录次数</th>
                        <th>角色</th>
                        <th>操作</th>
                    </tr>
                </thead>
                <tbody>${rows}</tbody>
            </table>
        `;
    }

    function getCurrentUserRole() {
        return (localStorage.getItem('monitor_role') || '').toLowerCase();
    }

    function isRoleFullAccess(role) {
        return role === 'admin' || role === 'super_admin' || role === 'sys_admin';
    }

    function isCurrentUserFullAccess() {
        return isRoleFullAccess(getCurrentUserRole());
    }

    // 兼容旧命名：管理员（含超管/系统管理员）
    function isCurrentUserAdmin() {
        return isCurrentUserFullAccess();
    }

    function applyRoleBasedVisibility() {
        const fullAccess = isAuthenticated && isCurrentUserFullAccess();
        const limited = isAuthenticated && !fullAccess;

        // 限制角色仅保留业务统计与认证入口
        ['system', 'failover', 'ocr', 'concurrency'].forEach((tab) => {
            const nav = document.getElementById(`tab-${tab}`);
            if (nav) nav.style.display = limited ? 'none' : '';
            const pane = document.getElementById(`${tab}-pane`);
            if (pane) pane.style.display = limited ? 'none' : '';
        });

        const businessOverview = document.querySelector('#business-pane .overview-cards');
        if (businessOverview) {
            businessOverview.style.display = limited ? 'none' : '';
        }

        const businessCharts = document.querySelector('#business-pane .charts-grid');
        if (businessCharts) {
            businessCharts.style.display = limited ? 'none' : '';
        }

        const recentFailuresCard = document
            .getElementById('recentFailuresContent')
            ?.closest('.card');
        if (recentFailuresCard) {
            recentFailuresCard.style.display = limited ? 'none' : '';
        }

        document
            .querySelectorAll('#business-pane .card-actions')
            .forEach((block) => {
                block.style.display = limited ? 'none' : '';
            });

        // 若当前处于被隐藏的面板，自动切回业务统计
        if (limited) {
            const activePane = document.querySelector('.view-pane.active');
            if (activePane && activePane.id !== 'business-pane' && activePane.id !== 'auth-pane') {
                activateTab('business');
            }
        }
    }

    function toggleUserManagementVisibility() {
        const adminBlocks = document.querySelectorAll('.admin-only');
        const visible = isAuthenticated && isCurrentUserAdmin();
        adminBlocks.forEach((block) => {
            const target =
                block.dataset.display ||
                (getComputedStyle(block).display === 'none' ? 'block' : '');
            block.style.display = visible ? target || 'block' : 'none';
        });
    }

    function openCreateUserModal(role = 'ops_admin') {
        if (!isAuthenticated || !isCurrentUserAdmin()) {
            alert('仅系统管理员可以创建账号');
            return;
        }
        createUserModalRole = role === 'admin' ? 'admin' : 'ops_admin';

        const modal = document.getElementById('createUserModal');
        const roleSelect = document.getElementById('createUserRole');
        const nameInput = document.getElementById('createUserName');
        const passwordInput = document.getElementById('createUserPassword');

        if (roleSelect) {
            roleSelect.value = createUserModalRole;
        }
        if (nameInput) nameInput.value = '';
        if (passwordInput) passwordInput.value = '';

        if (modal) {
            modal.style.display = 'flex';
            document.body.classList.add('modal-open');
        }
    }

    function closeCreateUserModal() {
        const modal = document.getElementById('createUserModal');
        if (modal) {
            modal.style.display = 'none';
        }
        document.body.classList.remove('modal-open');
    }

    async function submitCreateUser() {
        if (!authSession) {
            alert('请先登录监控系统');
            return;
        }

        if (!isCurrentUserAdmin()) {
            alert('仅系统管理员可以创建账号');
            return;
        }

        const roleSelect = document.getElementById('createUserRole');
        const usernameInput = document.getElementById('createUserName');
        const passwordInput = document.getElementById('createUserPassword');

        const role = roleSelect
            ? roleSelect.value === 'admin'
                ? 'admin'
                : 'ops_admin'
            : createUserModalRole;
        const username = usernameInput?.value.trim();
        const password = passwordInput?.value.trim();

        if (!username || !password) {
            alert('请输入用户名和初始密码');
            return;
        }

        try {
            const response = await apiFetch('/api/monitor/users', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({ username, password, role }),
            });

            if (!response.ok) {
                alert('创建失败');
                return;
            }

            const data = await response.json();
            if (data.success) {
                alert('创建成功');
                if (usernameInput) usernameInput.value = '';
                if (passwordInput) passwordInput.value = '';
                closeCreateUserModal();
                loadMonitorUsers();
            } else {
                alert(data.message || '创建失败');
            }
        } catch (error) {
            console.error('创建监控用户失败:', error);
            alert('创建失败，请稍后重试');
        }
    }

    async function changeMonitorUserRole(userId, role) {
        if (!authSession) return;

        try {
            const response = await apiFetch(
                `/api/monitor/users/${userId}/role`,
                {
                    method: 'PUT',
                    headers: {
                        'Content-Type': 'application/json',
                    },
                    body: JSON.stringify({ role }),
                },
            );

            if (!response.ok) {
                alert('更新角色失败');
                loadMonitorUsers();
                return;
            }

            const data = await response.json();
            if (!data.success) {
                alert(data.message || '更新角色失败');
            }
            loadMonitorUsers();
        } catch (error) {
            console.error('更新角色失败:', error);
            alert('更新角色失败，请稍后重试');
            loadMonitorUsers();
        }
    }

    async function resetMonitorUserPassword(userId) {
        if (!authSession) return;

        const newPassword = prompt('请输入新密码 (至少8位)：');
        if (!newPassword) return;

        if (newPassword.length < 8) {
            alert('密码长度至少需要8位');
            return;
        }

        try {
            let response = await apiFetch(
                `/api/monitor/users/${userId}/password`,
                {
                    method: 'PUT',
                    headers: {
                        'Content-Type': 'application/json',
                    },
                    body: JSON.stringify({ password: newPassword }),
                },
            );

            // 兼容旧后端：若 PUT /password 不存在，回退 POST /password/reset
            if (response.status === 404) {
                response = await apiFetch(
                    `/api/monitor/users/${userId}/password/reset`,
                    {
                        method: 'POST',
                        headers: {
                            'Content-Type': 'application/json',
                        },
                        body: JSON.stringify({ password: newPassword }),
                    },
                );
            }

            if (!response.ok) {
                alert('密码重置失败');
                return;
            }

            const data = await response.json();
            if (data.success) {
                alert('密码已重置');
            } else {
                alert(data.message || '密码重置失败');
            }
        } catch (error) {
            console.error('重置密码失败:', error);
            alert('重置密码失败，请稍后重试');
        }
    }

    async function toggleMonitorUserStatus(userId, enable) {
        if (!authSession) return;

        const url = enable
            ? `/api/monitor/users/${userId}/activate`
            : `/api/monitor/users/${userId}/deactivate`;

        try {
            let response = await apiFetch(url, {
                method: 'POST',
            });

            // 兼容旧后端：deactivate 404 时回退到 POST /users/{id}
            if (response.status === 404 && !enable) {
                response = await apiFetch(`/api/monitor/users/${userId}`, {
                    method: 'POST',
                });
            }

            if (!response.ok) {
                alert('操作失败');
                return;
            }

            const data = await response.json();
            if (data.success) {
                loadMonitorUsers();
            } else {
                alert(data.message || '操作失败');
            }
        } catch (error) {
            console.error('更新用户状态失败:', error);
            alert('更新用户状态失败，请稍后重试');
        }
    }

    async function checkFailoverStatus(forceReload = false) {
        try {
            const data = await fetchFailoverStatus({ forceReload });
            if (!data) {
                console.warn('未获取到故障转移状态数据');
                return;
            }
            updateFailoverCard(data);
            updateFailoverDetails(data);
        } catch (error) {
            console.error('加载故障转移状态失败:', error);
        }
    }

    function updateFailoverDetails(data = {}) {
        const payload = data.data || data;
        const dbState =
            payload.database?.state ||
            payload.database?.current_state ||
            'unknown';
        const storageState =
            payload.storage?.state ||
            payload.storage?.current_state ||
            'unknown';

        setText('dbFailoverStatus', mapFailoverStatus(dbState));
        setText(
            'dbLastCheck',
            formatDateTime(
                payload.database?.last_failover_time ||
                    payload.database?.last_checked_at,
            ),
        );
        setText(
            'storageFailoverStatus',
            mapFailoverStatus(storageState),
        );
        setText(
            'storageLastCheck',
            formatDateTime(
                payload.storage?.last_failover_time ||
                    payload.storage?.last_checked_at,
            ),
        );
    }

    async function triggerDbRecovery() {
        if (!window.confirm('确定触发数据库恢复操作吗？')) return;

        try {
            const response = await apiFetch('/api/failover/trigger-recovery', {
                method: 'POST',
            });
            if (response.ok) {
                alert('已触发数据库恢复');
                checkFailoverStatus(true);
            } else {
                alert('触发恢复失败');
            }
        } catch (error) {
            console.error('触发数据库恢复失败:', error);
            alert('触发恢复失败');
        }
    }

    async function triggerStorageRecovery() {
        if (!window.confirm('确定触发存储恢复操作吗？')) return;

        try {
            const response = await apiFetch('/api/storage/trigger-recovery', {
                method: 'POST',
            });
            if (response.ok) {
                alert('已触发存储恢复');
                checkFailoverStatus(true);
            } else {
                alert('触发恢复失败');
            }
        } catch (error) {
            console.error('触发存储恢复失败:', error);
            alert('触发恢复失败');
        }
    }

    function fetchDetailedResourceStatus(options) {
        return runSingleFlight(
            'resource-status',
            () => safeJsonRequest('/api/resources/status?detailed=true'),
            options,
        );
    }

    async function refreshOCRStats(forceReload = false) {
        try {
            const data = await fetchDetailedResourceStatus({ forceReload });
            if (data) {
                updateOCRStats(data);
            } else {
                console.warn('未获取到OCR资源状态数据');
            }
        } catch (error) {
            console.error('加载OCR池数据失败:', error);
        }
    }

    function updateOCRStats(data = {}) {
        const status = data.data?.multi_stage_status;
        if (!status) return;

        // 修复数据路径：应该从 stage_concurrency 中获取
        const ocrStage = status.stage_concurrency?.ocr_process;
        setText('poolCapacity', ocrStage?.max_concurrency ?? '-');
        const available = ocrStage ? (ocrStage.max_concurrency - ocrStage.active_tasks) : null;
        setText('poolAvailable', available !== null ? available : '-');
        setText('poolInUse', ocrStage?.active_tasks ?? '-');

        // OCR池重启次数（从ocr_pool中获取，如果有的话）
        const ocrPool = data.data?.ocr_pool;
        setText('poolRestarted', ocrPool?.total_restarts ?? '0');
    }

    async function refreshConcurrency(forceReload = false) {
        try {
            const data = await fetchDetailedResourceStatus({ forceReload });
            if (data) {
                updateConcurrencyTable(data);
            } else {
                console.warn('未获取到并发资源状态数据');
            }
        } catch (error) {
            console.error('加载并发数据失败:', error);
        }
    }


    function updateConcurrencyTable(data = {}) {
        const status = data.data?.multi_stage_status;
        if (!status) return;

        // 修复数据路径：应该从 stage_concurrency 中获取
        const stages = status.stage_concurrency;

        // 下载阶段
        const download = stages?.download;
        setText('downloadMax', download?.max_concurrency ?? '-');
        setText('downloadUsed', download?.active_tasks ?? '-');
        const downloadAvail = download ? (download.max_concurrency - download.active_tasks) : null;
        setText('downloadAvailable', downloadAvail !== null ? downloadAvail : '-');
        setText(
            'downloadRate',
            download?.utilization_percent !== undefined
                ? `${download.utilization_percent.toFixed(1)}%`
                : '-',
        );

        // PDF转换阶段
        const pdf = stages?.pdf_convert;
        setText('pdfMax', pdf?.max_concurrency ?? '-');
        setText('pdfUsed', pdf?.active_tasks ?? '-');
        const pdfAvail = pdf ? (pdf.max_concurrency - pdf.active_tasks) : null;
        setText('pdfAvailable', pdfAvail !== null ? pdfAvail : '-');
        setText(
            'pdfRate',
            pdf?.utilization_percent !== undefined
                ? `${pdf.utilization_percent.toFixed(1)}%`
                : '-',
        );

        // OCR处理阶段
        const ocr = stages?.ocr_process;
        setText('ocrMax', ocr?.max_concurrency ?? '-');
        setText('ocrUsed', ocr?.active_tasks ?? '-');
        const ocrAvail = ocr ? (ocr.max_concurrency - ocr.active_tasks) : null;
        setText('ocrAvailable', ocrAvail !== null ? ocrAvail : '-');
        setText(
            'ocrRate',
            ocr?.utilization_percent !== undefined
                ? `${ocr.utilization_percent.toFixed(1)}%`
                : '-',
        );

        // 存储阶段
        const storage = stages?.storage;
        setText('storageMax', storage?.max_concurrency ?? '-');
        setText('storageUsed', storage?.active_tasks ?? '-');
        const storageAvail = storage ? (storage.max_concurrency - storage.active_tasks) : null;
        setText('storageAvailable', storageAvail !== null ? storageAvail : '-');
        setText(
            'storageRate',
            storage?.utilization_percent !== undefined
                ? `${storage.utilization_percent.toFixed(1)}%`
                : '-',
        );

        // 瓶颈分析
        const bottleneck = data.data?.multi_stage_status?.bottleneck_stage;
        const bottleneckText =
            bottleneck === 'none'
                ? '当前无瓶颈，系统运行流畅'
                : `当前瓶颈阶段：${bottleneck || '无'}`;
        setText('bottleneckAnalysis', bottleneckText);
    }

    function formatDuration(seconds) {
        const value = Number(seconds);
        if (!Number.isFinite(value) || value <= 0) {
            return '';
        }

        const hours = Math.floor(value / 3600);
        const minutes = Math.floor((value % 3600) / 60);
        const secs = Math.floor(value % 60);

        const parts = [];
        if (hours > 0) {
            parts.push(`${hours}小时`);
        }
        if (minutes > 0) {
            parts.push(`${minutes}分`);
        }
        if (hours === 0 && secs > 0) {
            parts.push(`${secs}秒`);
        }

        return parts.length ? parts.join('') : '不足1秒';
    }

    function setText(id, text) {
        const el = document.getElementById(id);
        if (el) {
            el.textContent = text;
        }
    }

    function setColor(id, color) {
        const el = document.getElementById(id);
        if (el) {
            el.style.color = color;
        }
    }

    function mapFailoverStatus(status) {
        const map = {
            primary: '主用',
            fallback: '备用',
            recovering: '恢复中',
            unknown: '未知',
            '主数据库': '主用',
            '备用数据库': '备用',
            'OSS存储': '主用',
            '本地存储': '备用',
            '恢复中': '恢复中',
        };
        return map[status] || status || '未知';
    }

    // 暴露给全局的函数
    window.switchTab = switchTab;
    window.applyFilters = applyFilters;
    window.resetFilters = resetFilters;
    window.refreshBusinessData = refreshBusinessData;
    window.viewPreview = viewPreview;
    window.downloadResult = downloadResult;
    window.showRequestDetail = showRequestDetail;
    window.closeRequestDetail = closeRequestDetail;
    window.exportBusinessData = exportBusinessData;
    window.changePage = changePage;
    window.changePageSize = changePageSize;
    window.exportSystemData = exportSystemData;
    window.checkFailoverStatus = checkFailoverStatus;
    window.triggerDbRecovery = triggerDbRecovery;
    window.triggerStorageRecovery = triggerStorageRecovery;
    window.refreshOCRStats = refreshOCRStats;
    window.refreshConcurrency = refreshConcurrency;
    window.login = login;
    window.logout = logout;
    window.refreshAuthData = refreshAuthData;
    window.cleanupSessions = cleanupSessions;
    window.openCreateUserModal = openCreateUserModal;
    window.closeCreateUserModal = closeCreateUserModal;
    window.submitCreateUser = submitCreateUser;
    window.changeMonitorUserRole = changeMonitorUserRole;
    window.resetMonitorUserPassword = resetMonitorUserPassword;
    window.toggleMonitorUserStatus = toggleMonitorUserStatus;
    window.loadRecentFailures = loadRecentFailures;

    // ============ 全局限流控制 ============

    function toggleAdvancedOps() {
        const content = document.getElementById('advancedOpsContent');
        const icon = document.getElementById('advancedOpsIcon');
        if (content.style.display === 'none') {
            content.style.display = 'block';
            icon.textContent = '▼';
            refreshThrottleStatus();
        } else {
            content.style.display = 'none';
            icon.textContent = '▶';
        }
    }

    async function refreshThrottleStatus() {
        if (!authSession) return;
        try {
            const response = await apiFetch('/api/monitor/system/throttle/status');
            if (!response.ok) return;
            const data = await response.json();
            if (data.success) {
                updateThrottleUI(data.data);
            }
        } catch (error) {
            console.error('获取限流状态失败:', error);
        }
    }

    function updateThrottleUI(status) {
        const dot = document.getElementById('throttleStatusDot');
        const text = document.getElementById('throttleStatusText');
        const stats = document.getElementById('throttleStats');
        const enableBtn = document.getElementById('enableThrottleBtn');
        const disableBtn = document.getElementById('disableThrottleBtn');

        if (status.enabled) {
            dot.style.background = '#f44336';
            text.textContent = '限流已启用';
            text.style.color = '#f44336';
            stats.innerHTML = `已处理: ${status.current_count}/${status.max_requests} | 已拦截: ${status.blocked_count}`;
            enableBtn.style.display = 'none';
            disableBtn.style.display = 'inline-block';
        } else {
            dot.style.background = '#4caf50';
            text.textContent = '正常运行';
            text.style.color = '#4caf50';
            stats.innerHTML = '';
            enableBtn.style.display = 'inline-block';
            disableBtn.style.display = 'none';
        }
    }

    async function enableThrottle() {
        if (!authSession) {
            alert('请先登录');
            return;
        }
        const maxRequests = parseInt(document.getElementById('throttleMaxRequests').value) || 5;
        if (!confirm(`确定要启用全局限流吗？\n最大请求数: ${maxRequests}\n\n启用后，超过限制的请求将被拒绝。`)) {
            return;
        }
        try {
            const response = await apiFetch('/api/monitor/system/throttle/enable', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ max_requests: maxRequests, reason: '手动启用' })
            });
            const data = await response.json();
            if (data.success) {
                updateThrottleUI(data.data);
                showToast('限流已启用', 'warning');
            } else {
                alert(data.message || '操作失败');
            }
        } catch (error) {
            console.error('启用限流失败:', error);
            alert('操作失败');
        }
    }

    async function disableThrottle() {
        if (!authSession) {
            alert('请先登录');
            return;
        }
        if (!confirm('确定要解除全局限流吗？')) {
            return;
        }
        try {
            const response = await apiFetch('/api/monitor/system/throttle/disable', {
                method: 'POST'
            });
            const data = await response.json();
            if (data.success) {
                updateThrottleUI({ enabled: false });
                showToast(`限流已解除，共拦截 ${data.data.total_blocked} 个请求`, 'success');
            } else {
                alert(data.message || '操作失败');
            }
        } catch (error) {
            console.error('解除限流失败:', error);
            alert('操作失败');
        }
    }

    window.toggleAdvancedOps = toggleAdvancedOps;
    window.refreshThrottleStatus = refreshThrottleStatus;
    window.enableThrottle = enableThrottle;
    window.disableThrottle = disableThrottle;
})();
