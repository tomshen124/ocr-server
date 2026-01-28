/**
 * CYBERPUNK MONITOR V2 - JavaScript
 * OCR智能预审系统监控中心
 */

(() => {
    'use strict';

    // ================================
    // CONFIGURATION
    // ================================

    const CONFIG = {
        API_BASE: window.__MONITOR_API_BASE || '/api',
        AUTO_REFRESH_INTERVAL: 5000,
        CHART_REFRESH_INTERVAL: 2000,
        MAX_DATA_POINTS: 20
    };

    // ================================
    // STATE MANAGEMENT
    // ================================

    const state = {
        currentTab: 'overview',
        autoRefresh: false,
        refreshTimer: null,
        charts: {},
        lastData: {},
        dataHistory: {
            cpu: [],
            memory: [],
            requests: [],
            timestamps: []
        }
    };

    // ================================
    // UTILITY FUNCTIONS
    // ================================

    const $ = (selector) => document.querySelector(selector);
    const $$ = (selector) => document.querySelectorAll(selector);

    function apiUrl(path) {
        return `${CONFIG.API_BASE}${path}`;
    }

    async function fetchAPI(endpoint) {
        try {
            const response = await fetch(apiUrl(endpoint));
            if (!response.ok) throw new Error(`HTTP ${response.status}`);
            return await response.json();
        } catch (error) {
            console.error(`API Error [${endpoint}]:`, error);
            showToast(`Failed to fetch ${endpoint}`, 'error');
            return null;
        }
    }

    function showToast(message, type = 'info') {
        const container = $('#toastContainer');
        if (!container) return;

        const toast = document.createElement('div');
        toast.className = 'toast';
        toast.innerHTML = `
            <div style="font-family: var(--font-mono); font-weight: 600; color: var(--neon-cyan); margin-bottom: 0.25rem;">
                ${type.toUpperCase()}
            </div>
            <div style="color: var(--text-secondary); font-size: 0.85rem;">
                ${escapeHtml(message)}
            </div>
        `;

        container.appendChild(toast);

        setTimeout(() => {
            toast.style.opacity = '0';
            toast.style.transform = 'translateX(100%)';
            setTimeout(() => toast.remove(), 300);
        }, 3000);
    }

    function escapeHtml(text) {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }

    // ================================
    // COUNTER ANIMATION
    // ================================

    function animateCounter(element, target, duration = 1000) {
        const start = parseFloat(element.textContent) || 0;
        const end = parseFloat(target);
        const startTime = performance.now();

        function update(currentTime) {
            const elapsed = currentTime - startTime;
            const progress = Math.min(elapsed / duration, 1);

            // Easing function
            const easeOutQuad = progress * (2 - progress);
            const current = start + (end - start) * easeOutQuad;

            element.textContent = Math.round(current);

            if (progress < 1) {
                requestAnimationFrame(update);
            }
        }

        requestAnimationFrame(update);
    }

    // ================================
    // CLOCK UPDATE
    // ================================

    function updateClock() {
        const now = new Date();
        const timeStr = now.toTimeString().split(' ')[0];
        const clockEl = $('#currentTime');
        if (clockEl) clockEl.textContent = timeStr;
    }

    setInterval(updateClock, 1000);
    updateClock();

    // ================================
    // TAB NAVIGATION
    // ================================

    function switchTab(tabName) {
        // Update nav links
        $$('.nav-link').forEach(link => {
            link.classList.remove('active');
            if (link.dataset.tab === tabName) {
                link.classList.add('active');
            }
        });

        // Update panes
        $$('.tab-pane').forEach(pane => {
            pane.classList.remove('active');
        });
        const targetPane = $(`#${tabName}-pane`);
        if (targetPane) {
            targetPane.classList.add('active');
        }

        // Update breadcrumb & title
        const titles = {
            overview: 'SYSTEM OVERVIEW',
            business: 'BUSINESS STATS',
            resources: 'RESOURCES',
            ocr: 'OCR ENGINE POOL',
            failover: 'FAILOVER STATUS',
            tracing: 'DISTRIBUTED TRACING'
        };

        const titleEl = $('#pageTitle');
        if (titleEl) titleEl.textContent = titles[tabName] || 'OVERVIEW';

        const breadcrumbActive = $('.breadcrumb-item.active');
        if (breadcrumbActive) {
            breadcrumbActive.textContent = titles[tabName] || 'OVERVIEW';
        }

        state.currentTab = tabName;

        // Load data for the tab
        loadTabData(tabName);
    }

    // Attach click handlers
    $$('.nav-link').forEach(link => {
        link.addEventListener('click', (e) => {
            e.preventDefault();
            const tabName = link.dataset.tab;
            if (tabName) switchTab(tabName);
        });
    });

    // ================================
    // CHART INITIALIZATION
    // ================================

    function initCharts() {
        // Radar Chart for System Health
        const radarCtx = $('#radarChart');
        if (radarCtx) {
            state.charts.radar = new Chart(radarCtx, {
                type: 'radar',
                data: {
                    labels: ['CPU', 'Memory', 'Disk', 'Network', 'Database', 'OCR'],
                    datasets: [{
                        label: 'System Health',
                        data: [95, 88, 92, 97, 90, 94],
                        backgroundColor: 'rgba(0, 240, 255, 0.1)',
                        borderColor: 'rgba(0, 240, 255, 0.8)',
                        borderWidth: 2,
                        pointBackgroundColor: '#00f0ff',
                        pointBorderColor: '#fff',
                        pointHoverBackgroundColor: '#fff',
                        pointHoverBorderColor: '#00f0ff'
                    }]
                },
                options: {
                    responsive: true,
                    maintainAspectRatio: false,
                    scales: {
                        r: {
                            beginAtZero: true,
                            max: 100,
                            ticks: {
                                color: '#5a6376',
                                backdropColor: 'transparent'
                            },
                            grid: {
                                color: 'rgba(0, 240, 255, 0.1)'
                            },
                            pointLabels: {
                                color: '#8b95a8',
                                font: {
                                    family: 'JetBrains Mono',
                                    size: 12
                                }
                            }
                        }
                    },
                    plugins: {
                        legend: {
                            display: false
                        }
                    }
                }
            });
        }

        // Line Chart for Resources
        const resourceCtx = $('#resourceChart');
        if (resourceCtx) {
            state.charts.resource = new Chart(resourceCtx, {
                type: 'line',
                data: {
                    labels: [],
                    datasets: [
                        {
                            label: 'CPU %',
                            data: [],
                            borderColor: '#00f0ff',
                            backgroundColor: 'rgba(0, 240, 255, 0.1)',
                            tension: 0.4,
                            fill: true
                        },
                        {
                            label: 'Memory %',
                            data: [],
                            borderColor: '#ff006e',
                            backgroundColor: 'rgba(255, 0, 110, 0.1)',
                            tension: 0.4,
                            fill: true
                        }
                    ]
                },
                options: {
                    responsive: true,
                    maintainAspectRatio: false,
                    interaction: {
                        mode: 'index',
                        intersect: false
                    },
                    scales: {
                        x: {
                            ticks: { color: '#5a6376' },
                            grid: { color: 'rgba(255, 255, 255, 0.05)' }
                        },
                        y: {
                            beginAtZero: true,
                            max: 100,
                            ticks: { color: '#5a6376' },
                            grid: { color: 'rgba(255, 255, 255, 0.05)' }
                        }
                    },
                    plugins: {
                        legend: {
                            labels: {
                                color: '#8b95a8',
                                font: {
                                    family: 'JetBrains Mono',
                                    size: 11
                                }
                            }
                        }
                    }
                }
            });
        }

        // Line Chart for Throughput
        const throughputCtx = $('#throughputChart');
        if (throughputCtx) {
            state.charts.throughput = new Chart(throughputCtx, {
                type: 'line',
                data: {
                    labels: [],
                    datasets: [{
                        label: 'Requests/sec',
                        data: [],
                        borderColor: '#39ff14',
                        backgroundColor: 'rgba(57, 255, 20, 0.1)',
                        tension: 0.4,
                        fill: true
                    }]
                },
                options: {
                    responsive: true,
                    maintainAspectRatio: false,
                    scales: {
                        x: {
                            ticks: { color: '#5a6376' },
                            grid: { color: 'rgba(255, 255, 255, 0.05)' }
                        },
                        y: {
                            beginAtZero: true,
                            ticks: { color: '#5a6376' },
                            grid: { color: 'rgba(255, 255, 255, 0.05)' }
                        }
                    },
                    plugins: {
                        legend: {
                            labels: {
                                color: '#8b95a8',
                                font: {
                                    family: 'JetBrains Mono',
                                    size: 11
                                }
                            }
                        }
                    }
                }
            });
        }

        // Doughnut Chart for Status Distribution
        const statusCtx = $('#statusChart');
        if (statusCtx) {
            state.charts.status = new Chart(statusCtx, {
                type: 'doughnut',
                data: {
                    labels: ['Completed', 'Processing', 'Failed', 'Queued'],
                    datasets: [{
                        data: [12450, 8, 389, 0],
                        backgroundColor: [
                            'rgba(57, 255, 20, 0.8)',
                            'rgba(255, 190, 11, 0.8)',
                            'rgba(255, 0, 110, 0.8)',
                            'rgba(0, 240, 255, 0.8)'
                        ],
                        borderColor: '#151933',
                        borderWidth: 2
                    }]
                },
                options: {
                    responsive: true,
                    maintainAspectRatio: false,
                    plugins: {
                        legend: {
                            position: 'right',
                            labels: {
                                color: '#8b95a8',
                                font: {
                                    family: 'JetBrains Mono',
                                    size: 11
                                }
                            }
                        }
                    }
                }
            });
        }
    }

    // ================================
    // DATA LOADING
    // ================================

    async function loadTabData(tabName) {
        switch (tabName) {
            case 'overview':
                await loadOverviewData();
                break;
            case 'business':
                await loadBusinessData();
                break;
            case 'resources':
                await loadResourcesData();
                break;
            case 'ocr':
                await loadOCRData();
                break;
            case 'failover':
                await loadFailoverData();
                break;
            case 'tracing':
                await loadTracingData();
                break;
        }
    }

    async function loadOverviewData() {
        // Load system metrics
        const health = await fetchAPI('/health/details');
        const monitoring = await fetchAPI('/monitoring/status');

        if (monitoring) {
            updateMetricCard('cpu', monitoring.cpu_usage || 0);
            updateMetricCard('mem', monitoring.memory_usage || 0);
            updateMetricCard('disk', monitoring.disk_usage || 0);
            updateMetricCard('net', Math.random() * 100); // Mock network

            // Update health score
            const healthScore = calculateHealthScore(monitoring);
            const healthEl = $('#overallHealth');
            if (healthEl) {
                animateCounter(healthEl, healthScore);
            }
        }

        // Update charts with real-time data
        updateResourceChart();
    }

    async function loadBusinessData() {
        const stats = await fetchAPI('/preview/statistics');

        if (stats && stats.data) {
            const data = stats.data;

            // Update counters
            updateCounter('totalRequests', data.total_count || 0);
            updateCounter('successRate', ((data.completed_count / data.total_count) * 100).toFixed(1) || 0);
            updateCounter('avgResponse', Math.random() * 500); // Mock
            updateCounter('activeTasks', data.processing_count || 0);

            // Update table
            loadActivityTable();
        }
    }

    async function loadResourcesData() {
        const monitoring = await fetchAPI('/monitoring/status');
        // Populate resource details
        // Implementation depends on API structure
    }

    async function loadOCRData() {
        const queue = await fetchAPI('/queue/status');
        // Populate OCR pool status
        // Implementation depends on API structure
    }

    async function loadFailoverData() {
        const failover = await fetchAPI('/failover/status');
        // Populate failover status
        // Implementation depends on API structure
    }

    async function loadTracingData() {
        const tracing = await fetchAPI('/tracing/status');
        // Populate tracing data
        // Implementation depends on API structure
    }

    async function loadActivityTable() {
        const tbody = $('#activityTable tbody');
        if (!tbody) return;

        // Mock data - replace with real API call
        const activities = [
            { timestamp: '2025-12-08 14:23:45', requestId: 'REQ-8A3F2', status: 'completed', duration: '234ms', user: 'admin' },
            { timestamp: '2025-12-08 14:22:12', requestId: 'REQ-7B4E1', status: 'completed', duration: '189ms', user: 'user01' },
            { timestamp: '2025-12-08 14:21:03', requestId: 'REQ-6C5D0', status: 'processing', duration: '2.1s', user: 'user02' }
        ];

        tbody.innerHTML = activities.map(act => `
            <tr>
                <td>${act.timestamp}</td>
                <td style="color: var(--neon-cyan);">${act.requestId}</td>
                <td><span class="status-${act.status}">${act.status}</span></td>
                <td>${act.duration}</td>
                <td>${act.user}</td>
            </tr>
        `).join('');
    }

    function updateMetricCard(type, value) {
        const valueEl = $(`#${type}Value`);
        const barEl = $(`#${type}Bar`);

        if (valueEl) {
            valueEl.textContent = `${Math.round(value)}%`;
        }

        if (barEl) {
            barEl.style.width = `${value}%`;
        }
    }

    function updateCounter(id, value) {
        const el = $(`#${id} .counter`);
        if (el) {
            animateCounter(el, value);
        }
    }

    function calculateHealthScore(data) {
        if (!data) return 98.7;

        const cpu = 100 - (data.cpu_usage || 0);
        const mem = 100 - (data.memory_usage || 0);
        const disk = 100 - (data.disk_usage || 0);

        return ((cpu + mem + disk) / 3).toFixed(1);
    }

    function updateResourceChart() {
        const chart = state.charts.resource;
        if (!chart) return;

        const now = new Date();
        const timeLabel = now.toTimeString().split(' ')[0];

        // Generate mock data - replace with real API data
        const cpuValue = 30 + Math.random() * 40;
        const memValue = 40 + Math.random() * 30;

        // Add data point
        chart.data.labels.push(timeLabel);
        chart.data.datasets[0].data.push(cpuValue);
        chart.data.datasets[1].data.push(memValue);

        // Keep only last N points
        if (chart.data.labels.length > CONFIG.MAX_DATA_POINTS) {
            chart.data.labels.shift();
            chart.data.datasets[0].data.shift();
            chart.data.datasets[1].data.shift();
        }

        chart.update('none');
    }

    function updateThroughputChart() {
        const chart = state.charts.throughput;
        if (!chart) return;

        const now = new Date();
        const timeLabel = now.toTimeString().split(' ')[0];

        // Generate mock data
        const throughput = 50 + Math.random() * 100;

        chart.data.labels.push(timeLabel);
        chart.data.datasets[0].data.push(throughput);

        if (chart.data.labels.length > CONFIG.MAX_DATA_POINTS) {
            chart.data.labels.shift();
            chart.data.datasets[0].data.shift();
        }

        chart.update('none');
    }

    // ================================
    // AUTO REFRESH
    // ================================

    function toggleAutoRefresh() {
        const checkbox = $('#autoRefresh');
        if (!checkbox) return;

        state.autoRefresh = checkbox.checked;

        if (state.autoRefresh) {
            startAutoRefresh();
            showToast('Auto refresh enabled', 'info');
        } else {
            stopAutoRefresh();
            showToast('Auto refresh disabled', 'info');
        }
    }

    function startAutoRefresh() {
        stopAutoRefresh();
        state.refreshTimer = setInterval(() => {
            loadTabData(state.currentTab);
            updateResourceChart();
            updateThroughputChart();
        }, CONFIG.AUTO_REFRESH_INTERVAL);
    }

    function stopAutoRefresh() {
        if (state.refreshTimer) {
            clearInterval(state.refreshTimer);
            state.refreshTimer = null;
        }
    }

    // Attach event listener
    const autoRefreshCheckbox = $('#autoRefresh');
    if (autoRefreshCheckbox) {
        autoRefreshCheckbox.addEventListener('change', toggleAutoRefresh);
    }

    // Manual refresh button
    const refreshBtn = $('#refreshBtn');
    if (refreshBtn) {
        refreshBtn.addEventListener('click', () => {
            loadTabData(state.currentTab);
            showToast('Data refreshed', 'info');
        });
    }

    // ================================
    // INITIALIZATION
    // ================================

    function init() {
        console.log('🚀 CyberOps Monitor V2 Initialized');

        // Initialize charts
        initCharts();

        // Load initial data
        loadTabData('overview');

        // Animate counters on page load
        $$('.counter').forEach(counter => {
            const target = counter.dataset.target;
            if (target) {
                animateCounter(counter, target, 2000);
            }
        });

        // Start chart updates
        setInterval(() => {
            if (state.currentTab === 'overview') {
                updateResourceChart();
                updateThroughputChart();
            }
        }, CONFIG.CHART_REFRESH_INTERVAL);
    }

    // Run initialization when DOM is ready
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', init);
    } else {
        init();
    }

    // Expose global functions
    window.CyberMonitor = {
        switchTab,
        loadTabData,
        showToast
    };
})();
