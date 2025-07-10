/**
 * Debug环境测试配置
 * 统一管理所有调试工具的配置和数据
 */
window.DebugConfig = {
    // 基础配置
    baseConfig: {
        apiBaseUrl: '/api',
        debugMode: true,
        logLevel: 'debug',
        autoRefresh: 5000 // 状态自动刷新间隔(ms)
    },

    // API端点配置
    endpoints: {
        // 配置相关
        debugConfig: '/api/config/debug',
        frontendConfig: '/api/config',
        
        // 认证相关
        mockLogin: '/api/dev/mock_login',
        authStatus: '/api/auth/status',
        
        // 预审相关
        preview: '/api/preview',
        previewSubmit: '/api/preview/submit',
        previewStatus: '/api/preview/status',
        previewData: '/api/preview/data',
        previewLookup: '/api/preview/lookup',
        
        // 系统监控
        healthCheck: '/api/health',
        healthDetails: '/api/health/details',
        healthComponents: '/api/health/components',
        
        // 日志管理
        logStats: '/api/logs/stats',
        logCleanup: '/api/logs/cleanup',
        logHealth: '/api/logs/health'
    },

    // 模拟数据配置
    mockData: {
        users: [
            {
                userId: 'debug_user_001',
                userName: 'Debug测试用户001',
                certificateType: '01',
                certificateNumber: '330102199001010001',
                phoneNumber: '13800138001',
                email: 'debug001@example.com',
                organizationName: 'Debug测试公司A',
                organizationCode: '91330100MA28DEBUG01'
            },
            {
                userId: 'debug_user_002', 
                userName: 'Debug测试用户002',
                certificateType: '01',
                certificateNumber: '330102199001010002',
                phoneNumber: '13800138002',
                email: 'debug002@example.com',
                organizationName: 'Debug测试公司B',
                organizationCode: '91330100MA28DEBUG02'
            },
            {
                userId: 'admin_debug',
                userName: 'Debug管理员',
                certificateType: '01',
                certificateNumber: '330102199001010999',
                phoneNumber: '13800138999',
                email: 'admin@debug.com',
                organizationName: 'Debug系统管理',
                organizationCode: '91330100MA28DEBUG99'
            }
        ],
        
        matters: [
            {
                matterId: 'DEBUG_MATTER_001',
                matterName: 'Debug工程渣土运输许可',
                matterType: '许可',
                category: '建设工程',
                channel: 'web',
                enabled: true
            },
            {
                matterId: 'DEBUG_MATTER_002',
                matterName: 'Debug内资公司变更登记',
                matterType: '变更',
                category: '市场监管',
                channel: 'mobile',
                enabled: true
            },
            {
                matterId: 'DEBUG_MATTER_003',
                matterName: 'Debug建筑工程施工许可',
                matterType: '许可',
                category: '建设工程',
                channel: 'web',
                enabled: true
            },
            {
                matterId: 'DEBUG_MATTER_004',
                matterName: 'Debug环境影响评价备案',
                matterType: '备案',
                category: '环保',
                channel: 'window',
                enabled: true
            }
        ],

        // 模拟材料数据
        materials: [
            {
                name: 'Debug营业执照',
                type: 'pdf',
                size: 1024000,
                required: true,
                description: '企业营业执照副本'
            },
            {
                name: 'Debug法人身份证',
                type: 'jpg',
                size: 512000,
                required: true,
                description: '法定代表人身份证正反面'
            },
            {
                name: 'Debug授权委托书',
                type: 'pdf',
                size: 256000,
                required: false,
                description: '授权委托书（如非法人办理）'
            }
        ],

        // 渠道配置
        channels: [
            { value: 'web', label: 'Debug网上办事大厅', enabled: true },
            { value: 'mobile', label: 'Debug移动端', enabled: true },
            { value: 'window', label: 'Debug窗口办理', enabled: true },
            { value: 'self_service', label: 'Debug自助终端', enabled: false }
        ]
    },

    // Debug工具配置
    tools: {
        // 模拟登录工具
        mockLogin: {
            enabled: true,
            defaultUser: 'debug_user_001',
            autoFillForm: true,
            showWarning: true
        },

        // API测试工具
        apiTest: {
            enabled: true,
            includeAuth: true,
            showRequestDetails: true,
            showResponseTime: true
        },

        // 预审演示工具
        previewDemo: {
            enabled: true,
            autoGenerate: false,
            showAllStates: true,
            mockResults: true
        },

        // 流程测试工具
        flowTest: {
            enabled: true,
            stepDelay: 1000,
            autoAdvance: false,
            fullWorkflow: true
        },

        // 系统监控工具
        systemMonitor: {
            enabled: true,
            realTimeUpdate: true,
            showMetrics: true,
            alertThreshold: 80
        },

        // 数据管理工具
        dataManager: {
            enabled: true,
            allowEdit: true,
            allowDelete: false,
            backupData: true
        }
    },

    // 测试场景配置
    scenarios: {
        // 正常流程
        normal: {
            name: 'Debug正常预审流程',
            description: '完整的正常预审业务流程测试',
            steps: [
                { id: 'check_config', name: '检查Debug配置', required: true },
                { id: 'mock_login', name: '执行模拟登录', required: true },
                { id: 'prepare_data', name: '准备测试数据', required: true },
                { id: 'submit_preview', name: '提交预审请求', required: true },
                { id: 'check_status', name: '检查处理状态', required: true },
                { id: 'view_result', name: '查看预审结果', required: true }
            ]
        },
        
        // 错误场景
        error: {
            name: 'Debug错误场景测试',
            description: '各种异常情况和错误处理测试',
            steps: [
                { id: 'invalid_login', name: '无效登录测试', required: false },
                { id: 'auth_failure', name: '认证失败测试', required: false },
                { id: 'empty_data', name: '空数据提交测试', required: false },
                { id: 'invalid_request', name: '无效请求测试', required: false },
                { id: 'timeout_test', name: '超时场景测试', required: false }
            ]
        },

        // 性能测试
        performance: {
            name: 'Debug性能测试',
            description: '系统性能和并发能力测试',
            steps: [
                { id: 'concurrent_login', name: '并发登录测试', required: false },
                { id: 'batch_preview', name: '批量预审测试', required: false },
                { id: 'stress_test', name: '压力测试', required: false },
                { id: 'memory_test', name: '内存使用测试', required: false }
            ]
        }
    },

    // 工具方法
    utils: {
        // 生成随机ID
        generateId(prefix = 'DEBUG') {
            const timestamp = Date.now().toString(36);
            const random = Math.random().toString(36).substr(2, 5);
            return `${prefix}_${timestamp}_${random}`.toUpperCase();
        },

        // 生成模拟请求ID
        generateRequestId() {
            return this.generateId('REQ');
        },

        // 生成预审ID
        generatePreviewId() {
            return this.generateId('PV');
        },

        // 获取API完整URL
        getApiUrl(endpoint) {
            const baseUrl = this.parent.baseConfig.apiBaseUrl;
            const url = this.parent.endpoints[endpoint];
            return url ? `${baseUrl}${url}`.replace('/api/api', '/api') : null;
        },

        // 格式化JSON显示
        formatJson(obj, space = 2) {
            return JSON.stringify(obj, null, space);
        },

        // 日志输出
        log(message, level = 'info', data = null) {
            const timestamp = new Date().toISOString();
            const logData = {
                timestamp,
                level: level.toUpperCase(),
                message,
                data
            };
            
            console[level](
                `[${timestamp}] [${level.toUpperCase()}] ${message}`,
                data ? data : ''
            );
            
            // 发送到远程日志（如果配置）
            if (this.parent.baseConfig.remoteLogging) {
                this.sendRemoteLog(logData);
            }
        },

        // 发送远程日志
        sendRemoteLog(logData) {
            // 实现远程日志发送
            // 这里可以添加发送到服务器的逻辑
        },

        // 获取Mock用户
        getMockUser(userId = null) {
            const users = this.parent.mockData.users;
            if (userId) {
                return users.find(user => user.userId === userId) || users[0];
            }
            return users[0];
        },

        // 获取Mock事项
        getMockMatter(matterId = null) {
            const matters = this.parent.mockData.matters;
            if (matterId) {
                return matters.find(matter => matter.matterId === matterId) || matters[0];
            }
            return matters[0];
        },

        // 检查工具是否启用
        isToolEnabled(toolName) {
            return this.parent.tools[toolName]?.enabled || false;
        },

        // 等待指定时间
        async delay(ms) {
            return new Promise(resolve => setTimeout(resolve, ms));
        },

        // 检查Debug配置
        async checkDebugConfig() {
            try {
                const response = await fetch(this.getApiUrl('debugConfig'));
                if (response.ok) {
                    const result = await response.json();
                    return result.data || {};
                }
            } catch (error) {
                this.log('Debug配置检查失败', 'error', error);
            }
            return { enabled: false };
        }
    },

    // 初始化配置
    async init() {
        this.utils.parent = this; // 设置父引用
        
        try {
            // 检查Debug配置
            const debugConfig = await this.utils.checkDebugConfig();
            
            if (!debugConfig.enabled) {
                this.utils.log('Debug模式已禁用', 'warn');
                return false;
            }
            
            // 更新工具配置
            if (debugConfig.tools) {
                Object.keys(debugConfig.tools).forEach(tool => {
                    if (this.tools[tool]) {
                        this.tools[tool].enabled = debugConfig.tools[tool];
                    }
                });
            }
            
            this.utils.log('Debug配置初始化完成', 'info', debugConfig);
            return true;
            
        } catch (error) {
            this.utils.log('Debug配置初始化失败', 'error', error);
            return false;
        }
    },

    // 获取完整配置
    getConfig() {
        return this;
    }
};

// 自动初始化
document.addEventListener('DOMContentLoaded', () => {
    DebugConfig.init();
});

// 为了兼容现有代码，保留TestConfig别名
window.TestConfig = window.DebugConfig; 