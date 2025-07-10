// OCR监控工具 - 基于原有监控工具改进
// 集成到主服务的监控JavaScript

// 全局变量
let isRequesting = false;
let updateInterval;

// 基础配置
const config = {
    refreshInterval: 60000,  // 1分钟刷新一次系统资源
    logRefreshInterval: 5000  // 5秒刷新一次日志
};

// 初始化监控
function initMonitoring() {
    console.log('初始化OCR监控系统');

    // 检查认证状态
    checkAuth();

    // 先获取日志文件列表，然后显示最新的日志
    fetchLogFileList().then(() => {
        const logSelector = document.getElementById('log-file-selector');
        if (logSelector && logSelector.options.length > 1) {
            const latestDate = logSelector.options[1].value;
            updateLogs(latestDate);
        } else {
            updateLogs();
        }
    });

    // 立即执行其他更新
    updateSystemStatus();

    // 设置定时刷新
    setInterval(updateSystemStatus, config.refreshInterval);
    setInterval(() => {
        const logSelector = document.getElementById('log-file-selector');
        const selectedDate = logSelector && logSelector.value ? logSelector.value : null;
        updateLogs(selectedDate);
    }, config.logRefreshInterval);

    // 添加事件监听器
    setupEventListeners();
}

// 设置事件监听器
function setupEventListeners() {
    // 登出按钮 - 监控页面改为返回主页
    const logoutBtn = document.getElementById('logout-btn');
    if (logoutBtn) {
        logoutBtn.addEventListener('click', () => {
            window.location.href = '/static/index.html';
        });
    }

    // 重启OCR服务按钮
    const restartOcrBtn = document.getElementById('restartOcrBtn');
    if (restartOcrBtn) {
        restartOcrBtn.addEventListener('click', handleRestartOcr);
    }

    // 日志文件选择器
    const logSelector = document.getElementById('log-file-selector');
    if (logSelector) {
        logSelector.addEventListener('change', (e) => {
            updateLogs(e.target.value);
        });
    }
}

// 检查认证状态
async function checkAuth() {
    try {
        // 监控页面不需要业务用户认证，直接显示为系统管理员
        const userElement = document.getElementById('current-user');
        if (userElement) {
            userElement.textContent = '用户: 系统管理员';
        }
        
        console.log('监控页面已加载，无需业务用户认证');
        return true;
    } catch (error) {
        console.error('Auth check failed:', error);
        // 如果认证检查失败，假设是未启用认证，继续运行
        const userElement = document.getElementById('current-user');
        if (userElement) {
            userElement.textContent = '用户: 系统管理员';
        }
        return true;
    }
}

// 更新系统状态和资源
async function updateSystemStatus() {
    if (isRequesting) return;
    isRequesting = true;

    try {
        // 获取健康检查状态
        const healthResponse = await fetch('/api/health/details');
        if (healthResponse.ok) {
            const healthData = await healthResponse.json();
            updateStatusDisplay(healthData);
            updateMetricsDisplay(healthData);
            updateResourcesTable(healthData);
        }

        // 获取组件健康状态
        const componentsResponse = await fetch('/api/health/components');
        if (componentsResponse.ok) {
            const componentsData = await componentsResponse.json();
            updateComponentsStatus(componentsData);
        }

    } catch (error) {
        console.error('更新系统状态失败:', error);
        showError('获取监控数据失败: ' + error.message);
    } finally {
        isRequesting = false;
        updateLastRefreshTime();
    }
}

// 更新状态显示
function updateStatusDisplay(healthData) {
    // 更新OCR服务状态
    const ocrStatusElement = document.getElementById('ocrStatus');
    const ocrDetailElement = document.getElementById('ocrDetail');

    if (ocrStatusElement && ocrDetailElement) {
        const isHealthy = healthData.status === 'healthy';
        ocrStatusElement.className = `status-indicator ${isHealthy ? 'status-running' : 'status-stopped'}`;
        ocrDetailElement.textContent = `状态: ${isHealthy ? '正常运行' : '服务异常'} | 版本: ${healthData.version}`;
    }

    // 更新监控API状态
    const apiStatusElement = document.getElementById('apiStatus');
    const apiResponseElement = document.getElementById('apiResponseTime');

    if (apiStatusElement && apiResponseElement) {
        const isHealthy = healthData.status === 'healthy';
        apiStatusElement.className = `status-indicator ${isHealthy ? 'status-running' : 'status-stopped'}`;
        apiResponseElement.textContent = `运行时长: ${formatUptime(healthData.uptime || 0)}`;
    }
}

// 更新指标显示
function updateMetricsDisplay(healthData) {
    // 更新CPU使用率
    const cpuElement = document.getElementById('cpuUsage');
    if (cpuElement && healthData.cpu) {
        cpuElement.textContent = healthData.cpu.usage_percent.toFixed(1) + '%';
        cpuElement.className = 'metric-value ' + getStatusClass(healthData.cpu.usage_percent);
    }

    // 更新内存使用率
    const memoryElement = document.getElementById('memoryUsage');
    if (memoryElement && healthData.memory) {
        memoryElement.textContent = healthData.memory.usage_percent.toFixed(1) + '%';
        memoryElement.className = 'metric-value ' + getStatusClass(healthData.memory.usage_percent);
    }

    // 更新磁盘使用率
    const diskElement = document.getElementById('diskUsage');
    if (diskElement && healthData.disk) {
        diskElement.textContent = healthData.disk.usage_percent.toFixed(1) + '%';
        diskElement.className = 'metric-value ' + getStatusClass(healthData.disk.usage_percent);
    }

    // 更新进程数
    const processElement = document.getElementById('processCount');
    if (processElement && healthData.queue) {
        const totalTasks = (healthData.queue.pending || 0) + (healthData.queue.processing || 0);
        processElement.textContent = totalTasks || '--';
        processElement.className = 'metric-value normal';
    }
}

// 更新组件状态
function updateComponentsStatus(componentsData) {
    // 这里可以根据组件状态更新UI
    // 目前组件状态信息主要用于后台监控
    console.log('组件状态:', componentsData);
}

// 更新OCR服务状态
function updateOcrStatus(status) {
    const ocrDetailElement = document.getElementById('ocrDetail');
    if (ocrDetailElement) {
        const portStatus = status.port_listening ? '正常' : '异常';
        const apiStatus = status.api_responsive ? '正常' : '异常';
        ocrDetailElement.textContent = `端口: ${portStatus} | API: ${apiStatus}`;
    }
}

// 更新资源表格
function updateResourcesTable(healthData) {
    const tbody = document.getElementById('metricsBody');
    if (!tbody || !healthData.cpu || !healthData.memory || !healthData.disk) return;

    const row = document.createElement('tr');
    const totalTasks = healthData.queue ? (healthData.queue.pending || 0) + (healthData.queue.processing || 0) : 0;
    
    row.innerHTML = `
        <td>${new Date().toLocaleTimeString()}</td>
        <td>${healthData.cpu.usage_percent.toFixed(1)}%</td>
        <td>${healthData.memory.usage_percent.toFixed(1)}%</td>
        <td>${healthData.disk.usage_percent.toFixed(1)}%</td>
        <td>${totalTasks || '--'}</td>
    `;

    // 添加告警样式
    if (healthData.cpu.usage_percent > 90 ||
        healthData.memory.usage_percent > 90 ||
        healthData.disk.usage_percent > 90) {
        row.classList.add('resource-warning');
    }

    // 更新表格内容（只保留最新一条记录）
    tbody.innerHTML = '';
    tbody.appendChild(row);
}

// 获取状态样式类
function getStatusClass(value) {
    if (value < 70) return 'normal';
    if (value < 90) return 'warning';
    return 'critical';
}

// 格式化运行时间
function formatUptime(seconds) {
    const days = Math.floor(seconds / 86400);
    const hours = Math.floor((seconds % 86400) / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);

    if (days > 0) {
        return `${days}天${hours}小时`;
    } else if (hours > 0) {
        return `${hours}小时${minutes}分钟`;
    } else {
        return `${minutes}分钟`;
    }
}

// 更新最后刷新时间
function updateLastRefreshTime() {
    const lastUpdateElement = document.getElementById('lastUpdate');
    if (lastUpdateElement) {
        lastUpdateElement.textContent = '最后更新: ' + new Date().toLocaleString();
    }
}

// 刷新数据
async function refreshData() {
    console.log('手动刷新数据');
    await updateSystemStatus();
}

// 显示错误信息
function showError(message) {
    console.error(message);

    // 创建错误提示
    const errorDiv = document.createElement('div');
    errorDiv.style.cssText = `
        position: fixed;
        top: 20px;
        right: 20px;
        background: #ff4d4f;
        color: white;
        padding: 15px 20px;
        border-radius: 4px;
        z-index: 1000;
        box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
        max-width: 300px;
    `;
    errorDiv.textContent = message;
    document.body.appendChild(errorDiv);

    setTimeout(() => {
        if (document.body.contains(errorDiv)) {
            document.body.removeChild(errorDiv);
        }
    }, 5000);
}

// 页面加载完成后初始化
document.addEventListener('DOMContentLoaded', () => {
    console.log('页面加载完成，开始初始化监控');
    initMonitoring();
});

// 获取日志文件列表
async function fetchLogFileList() {
    try {
        // 使用日志统计API获取日志信息
        const response = await fetch('/api/logs/stats');
        if (response.ok) {
            const data = await response.json();
            if (data.success && data.data) {
                // 模拟日志文件列表（基于统计信息）
                const files = [
                    {
                        date: new Date().toISOString().split('T')[0],
                        size: '当前日志'
                    }
                ];
                updateLogFileSelector(files);
            }
        }
    } catch (error) {
        console.error('获取日志文件列表失败:', error);
    }
}

// 更新日志文件选择器
function updateLogFileSelector(files) {
    const selector = document.getElementById('log-file-selector');
    if (!selector) return;

    // 清空现有选项（保留"当前日志"选项）
    while (selector.children.length > 1) {
        selector.removeChild(selector.lastChild);
    }

    // 添加日志文件选项
    files.forEach(file => {
        const option = document.createElement('option');
        option.value = file.date;
        option.textContent = `${file.date} (${file.size})`;
        selector.appendChild(option);
    });
}

// 更新日志
async function updateLogs(date = null) {
    try {
        // 更新日期显示
        updateLogDate(date);

        // 获取日志健康状态
        const response = await fetch('/api/logs/health');
        if (response.ok) {
            const data = await response.json();
            if (data.success && data.data) {
                const logInfo = [
                    `日志系统状态: ${data.data.status}`,
                    `日志目录: ${data.data.directory}`,
                    `日志启用: ${data.data.enabled ? '是' : '否'}`,
                    `保留天数: ${data.data.retention_days || '未设置'}`,
                    `检查时间: ${new Date().toLocaleString()}`
                ];
                displayLogInfo(logInfo);
            }
        }
    } catch (error) {
        console.error('更新日志失败:', error);
        displayLogInfo(['获取日志信息失败: ' + error.message]);
    }
}

// 显示日志信息
function displayLogInfo(logInfo) {
    const logContent = document.getElementById('logContent');
    if (!logContent) return;

    if (!logInfo || logInfo.length === 0) {
        logContent.innerHTML = '<div style="text-align: center; color: #666; padding: 20px;">暂无日志信息</div>';
        return;
    }

    // 格式化日志信息
    const formattedInfo = logInfo.map(info => {
        return `<span class="log-info">${info}</span>`;
    }).join('\n');

    logContent.innerHTML = `<pre>${formattedInfo}</pre>`;
}

// 显示日志
function displayLogs(logs) {
    const logContent = document.getElementById('logContent');
    if (!logContent) return;

    if (!logs || logs.length === 0) {
        logContent.innerHTML = '<div style="text-align: center; color: #666; padding: 20px;">暂无日志数据</div>';
        return;
    }

    // 格式化日志内容
    const formattedLogs = logs.map(log => {
        let logLine = `${log.timestamp} [${log.level}] ${log.message}`;

        // 根据日志级别添加颜色
        if (log.level === 'ERROR') {
            return `<span class="log-error">${logLine}</span>`;
        } else if (log.level === 'WARN') {
            return `<span class="log-warning">${logLine}</span>`;
        } else if (log.level === 'INFO') {
            return `<span class="log-info">${logLine}</span>`;
        } else if (log.level === 'DEBUG') {
            return `<span class="log-debug">${logLine}</span>`;
        }
        return logLine;
    }).join('\n');

    logContent.innerHTML = `<pre>${formattedLogs}</pre>`;

    // 滚动到底部
    logContent.scrollTop = logContent.scrollHeight;
}

// 更新日志日期显示
function updateLogDate(date) {
    const logDateElement = document.getElementById('log-date');
    if (logDateElement) {
        logDateElement.textContent = date ? `(${date})` : '(当前)';
    }
}

// 刷新日志
function refreshLog() {
    const logSelector = document.getElementById('log-file-selector');
    const selectedDate = logSelector ? logSelector.value : null;
    updateLogs(selectedDate);
}

// 选择日志文件
function selectLogFile(date) {
    updateLogs(date);
}

// 重启OCR服务
async function handleRestartOcr() {
    if (!confirm('确定要重启OCR服务吗？这可能会中断正在进行的OCR任务。')) {
        return;
    }

    try {
        // 暂时显示提示信息，因为我们没有实现重启API
        showSuccess('重启功能暂未实现，请使用命令行工具重启服务');
        
        // 可以通过以下命令重启：
        // ./ocr-server.sh restart
        
    } catch (error) {
        showError('重启OCR服务失败: ' + error.message);
    }
}

// 显示成功信息
function showSuccess(message) {
    const successDiv = document.createElement('div');
    successDiv.style.cssText = `
        position: fixed;
        top: 20px;
        right: 20px;
        background: #52c41a;
        color: white;
        padding: 15px 20px;
        border-radius: 4px;
        z-index: 1000;
        box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
        max-width: 300px;
    `;
    successDiv.textContent = message;
    document.body.appendChild(successDiv);

    setTimeout(() => {
        if (document.body.contains(successDiv)) {
            document.body.removeChild(successDiv);
        }
    }, 3000);
}

// 页面卸载时清理定时器
window.addEventListener('beforeunload', () => {
    if (updateInterval) {
        clearInterval(updateInterval);
    }
});
