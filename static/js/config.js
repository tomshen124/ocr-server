// 前端配置管理模块
const ConfigManager = {
    // 缓存的配置数据
    config: null,
    
    // 初始化配置
    async init() {
        try {
            await this.loadConfig();
            console.log('✅ 前端配置加载成功');
            return true;
        } catch (error) {
            console.error('❌ 前端配置加载失败:', error);
            // 使用备用配置
            this.loadFallbackConfig();
            return false;
        }
    },

    // 从后端加载配置
    async loadConfig() {
        const response = await fetch('/api/config/frontend', {
            method: 'GET',
            credentials: 'include'
        });

        if (!response.ok) {
            throw new Error(`HTTP ${response.status}: ${response.statusText}`);
        }

        const result = await response.json();
        
        if (!result.success) {
            throw new Error(result.errorMsg || '获取配置失败');
        }

        this.config = result.data;
        console.log('配置数据已加载:', this.config);
    },

    // 备用配置（当后端不可用时）
    loadFallbackConfig() {
        console.warn('使用备用配置数据');
        this.config = {
            themes: [
                {
                    "id": "theme_001",
                    "name": "工程渣土准运证核准",
                    "description": "杭州市工程渣土准运证核准申请材料预审",
                    "enabled": true
                }
            ],
            defaultUser: {
                "userId": "FALLBACK_USER",
                "userName": "备用测试用户",
                "certificateType": "01",
                "certificateNumber": "000000000000000000",
                "phoneNumber": "13800138000",
                "email": "fallback@example.com",
                "organizationName": "备用机构",
                "organizationCode": "FALLBACK001"
            },
            matters: [
                {
                    "matterId": "MATTER_001",
                    "matterName": "工程渣土准运证核准",
                    "matterType": "许可",
                    "category": "建设工程",
                    "enabled": true
                }
            ],
            channels: [
                { "value": "web", "label": "网上办事大厅", "enabled": true },
                { "value": "mobile", "label": "移动端", "enabled": true },
                { "value": "window", "label": "窗口办理", "enabled": true },
                { "value": "self_service", "label": "自助终端", "enabled": true }
            ],
            fileTypes: [
                { "ext": ".pdf", "mime": "application/pdf", "maxSize": "100MB" },
                { "ext": ".jpg", "mime": "image/jpeg", "maxSize": "50MB" },
                { "ext": ".jpeg", "mime": "image/jpeg", "maxSize": "50MB" },
                { "ext": ".png", "mime": "image/png", "maxSize": "50MB" },
                { "ext": ".bmp", "mime": "image/bmp", "maxSize": "50MB" }
            ],
            systemInfo: {
                "version": "1.0.0",
                "environment": "fallback",
                "maxFileSize": "100MB",
                "allowedFormats": [".pdf", ".jpg", ".jpeg", ".png", ".bmp"]
            }
        };
    },

    // 获取主题列表
    getThemes() {
        return this.config?.themes || [];
    },

    // 获取默认用户信息
    getDefaultUser() {
        return this.config?.defaultUser || {};
    },

    // 获取事项列表
    getMatters() {
        return this.config?.matters || [];
    },

    // 获取渠道列表
    getChannels() {
        return this.config?.channels || [];
    },

    // 获取文件类型配置
    getFileTypes() {
        return this.config?.fileTypes || [];
    },

    // 获取系统信息
    getSystemInfo() {
        return this.config?.systemInfo || {};
    },

    // 根据ID获取特定主题
    getThemeById(themeId) {
        return this.getThemes().find(theme => theme.id === themeId);
    },

    // 根据ID获取特定事项
    getMatterById(matterId) {
        return this.getMatters().find(matter => matter.matterId === matterId);
    },

    // 获取启用的主题
    getEnabledThemes() {
        return this.getThemes().filter(theme => theme.enabled);
    },

    // 获取启用的事项
    getEnabledMatters() {
        return this.getMatters().filter(matter => matter.enabled);
    },

    // 获取启用的渠道
    getEnabledChannels() {
        return this.getChannels().filter(channel => channel.enabled);
    },

    // 生成测试用的请求ID
    generateRequestId() {
        const timestamp = new Date().toISOString().replace(/[-:.]/g, '').slice(0, 14);
        const random = Math.random().toString(36).substr(2, 6).toUpperCase();
        return `REQ_${timestamp}_${random}`;
    },

    // 生成测试数据
    generateTestData() {
        const defaultUser = this.getDefaultUser();
        const matters = this.getEnabledMatters();
        const channels = this.getEnabledChannels();
        
        return {
            userId: defaultUser.userId || this.generateTestUserId(),
            userName: defaultUser.userName || "系统测试用户",
            requestId: this.generateRequestId(),
            matterId: matters[0]?.matterId || "MATTER_001",
            matterName: matters[0]?.matterName || "默认事项",
            matterType: matters[0]?.matterType || "许可",
            channel: channels[0]?.value || "web",
            agentInfo: {
                userId: defaultUser.userId,
                certificateType: defaultUser.certificateType || "01",
                userName: defaultUser.userName,
                certificateNumber: defaultUser.certificateNumber,
                phoneNumber: defaultUser.phoneNumber || "13800138000",
                email: defaultUser.email || "test@example.com"
            },
            subjectInfo: {
                userId: defaultUser.userId,
                certificateType: defaultUser.certificateType || "01",
                userName: defaultUser.userName,
                certificateNumber: defaultUser.certificateNumber
            }
        };
    },

    // 生成测试用户ID
    generateTestUserId() {
        const timestamp = new Date().toISOString().replace(/[-:.]/g, '').slice(8, 14);
        return `TEST_USER_${timestamp}`;
    },

    // 检查配置是否已加载
    isLoaded() {
        return this.config !== null;
    },

    // 重新加载配置
    async reload() {
        this.config = null;
        return await this.init();
    }
};

// 全局暴露配置管理器
window.ConfigManager = ConfigManager; 