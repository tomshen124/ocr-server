/**
 * OCR预审系统测试框架 - 核心模块
 * 提供统一的测试基础设施和工具函数
 */

class OCRTestFramework {
    constructor() {
        this.config = {
            baseUrl: window.location.origin,
            timeout: 30000,
            retryCount: 3
        };
        this.currentTests = new Map();
        this.testResults = [];
    }

    /**
     * 统一的HTTP请求方法
     */
    async request(method, url, data = null, options = {}) {
        const requestId = this.generateTraceId();
        const startTime = Date.now();
        
        try {
            const config = {
                method,
                headers: {
                    'Content-Type': 'application/json',
                    'X-Test-Request-ID': requestId,
                    ...options.headers
                },
                ...options
            };

            if (data && method !== 'GET') {
                config.body = JSON.stringify(data);
            }

            console.log(`🚀 [${requestId}] ${method} ${url}`, data || '');
            
            const response = await fetch(url, config);
            const duration = Date.now() - startTime;
            
            // 提取trace_id（如果有）
            const traceId = response.headers.get('X-Trace-ID');
            
            let responseData;
            const contentType = response.headers.get('content-type');
            if (contentType && contentType.includes('application/json')) {
                responseData = await response.json();
            } else {
                responseData = await response.text();
            }

            const result = {
                requestId,
                traceId,
                status: response.status,
                statusText: response.statusText,
                headers: Object.fromEntries(response.headers.entries()),
                data: responseData,
                duration,
                success: response.ok
            };

            console.log(`✅ [${requestId}] 响应 ${response.status} (${duration}ms)`, {
                traceId,
                data: responseData
            });

            return result;
        } catch (error) {
            const duration = Date.now() - startTime;
            console.error(`❌ [${requestId}] 请求失败 (${duration}ms)`, error);
            
            return {
                requestId,
                error: error.message,
                duration,
                success: false
            };
        }
    }

    /**
     * 生成测试用的trace_id
     */
    generateTraceId() {
        return 'test_' + Date.now().toString(36) + Math.random().toString(36).substr(2);
    }

    /**
     * 显示测试结果的统一方法
     */
    showResult(containerId, message, type = 'info', data = null) {
        const container = document.getElementById(containerId);
        if (!container) {
            console.error(`找不到结果容器: ${containerId}`);
            return;
        }

        const timestamp = new Date().toLocaleTimeString();
        const icons = {
            success: '✅',
            error: '❌',
            warning: '⚠️',
            info: '🔍'
        };

        let html = `
            <div class="result-item ${type}">
                <div class="result-header">
                    ${icons[type] || '📝'} ${message}
                    <span class="timestamp">${timestamp}</span>
                </div>
        `;

        if (data) {
            // 格式化数据显示
            if (data.traceId) {
                html += `<div class="trace-id">追踪ID: <code>${data.traceId}</code></div>`;
            }
            
            if (data.duration) {
                html += `<div class="duration">耗时: ${data.duration}ms</div>`;
            }

            if (data.data || data.error) {
                const content = data.data || data.error;
                html += `
                    <details class="result-details">
                        <summary>详细信息</summary>
                        <pre class="result-data">${JSON.stringify(content, null, 2)}</pre>
                    </details>
                `;
            }
        }

        html += '</div>';
        container.innerHTML = html;
        
        // 记录测试结果
        this.testResults.push({
            containerId,
            message,
            type,
            data,
            timestamp: Date.now()
        });
    }

    /**
     * 执行测试套件
     */
    async runTestSuite(suiteName, tests) {
        console.group(`🧪 开始执行测试套件: ${suiteName}`);
        const results = [];
        
        for (const test of tests) {
            try {
                console.log(`🔬 执行测试: ${test.name}`);
                const result = await test.execute();
                results.push({ name: test.name, result, success: true });
            } catch (error) {
                console.error(`❌ 测试失败: ${test.name}`, error);
                results.push({ name: test.name, error: error.message, success: false });
            }
        }
        
        console.groupEnd();
        return results;
    }

    /**
     * 系统状态检查
     */
    async checkSystemStatus() {
        const statusChecks = [
            { name: '健康检查', url: '/api/health' },
            { name: '认证状态', url: '/api/auth/status' },
            { name: '主题列表', url: '/api/themes' },
            { name: '系统监控', url: '/api/monitoring/status' }
        ];

        const results = {};
        for (const check of statusChecks) {
            try {
                const result = await this.request('GET', check.url);
                results[check.name] = {
                    status: result.success ? 'OK' : 'FAIL',
                    traceId: result.traceId,
                    duration: result.duration,
                    data: result.data
                };
            } catch (error) {
                results[check.name] = {
                    status: 'ERROR',
                    error: error.message
                };
            }
        }

        return results;
    }

    /**
     * 导出测试报告
     */
    exportTestReport() {
        const report = {
            timestamp: new Date().toISOString(),
            testResults: this.testResults,
            systemInfo: {
                userAgent: navigator.userAgent,
                url: window.location.href,
                testFrameworkVersion: '1.0.0'
            }
        };

        const blob = new Blob([JSON.stringify(report, null, 2)], {
            type: 'application/json'
        });
        
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = `ocr-test-report-${Date.now()}.json`;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);
    }
}

// 全局测试框架实例
window.OCRTest = new OCRTestFramework();