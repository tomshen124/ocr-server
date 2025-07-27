// 前端配置文件 - 统一管理API接口和数据映射

// API 基础配置
const API_CONFIG = {
    baseUrl: '/api',
    timeout: 30000,
    
    // API 端点映射
    endpoints: {
        // 预审相关接口
        preview: '/preview',
        previewData: '/preview/data/{previewId}',
        previewStatus: '/preview/status/{previewId}',
        previewView: '/preview/view/{previewId}',
        
        // 认证相关接口
        authStatus: '/auth/status',
        authLogout: '/auth/logout',
        mockLogin: '/test/mock_login',
        
        // 系统相关接口
        health: '/health',
        healthDetails: '/health/details',
        config: '/config/frontend',
        themes: '/themes',
        
        // 队列状态接口 - 新增并发控制监控
        queueStatus: '/queue/status',
        
        // 下载接口
        download: '/download',
        
        // 测试接口
        mockData: '/test/mock/data'
    }
};

// 数据映射配置
const DATA_MAPPING = {
    // 后端数据字段到前端字段的映射
    preview: {
        // 基本信息映射
        basicInfo: {
            applicant: ['applicant_name', 'legalRep.FDDBR', 'self.DWMC'],
            applicationType: ['matter_name', 'application_type'],
            auditOrgan: ['audit_organ', () => '智能预审系统']
        },
        
        // 材料状态映射
        statusMapping: {
            'success': 'passed',
            'passed': 'passed',
            'pass': 'passed',
            'warning': 'warning', 
            'warn': 'warning',
            'error': 'error',
            'failed': 'error',
            'fail': 'error',
            'pending': 'loading',
            'processing': 'loading',
            'running': 'loading'
        },
        
        // 审核状态映射
        auditStatusMapping: {
            'completed': 'completed',
            'finished': 'completed', 
            'done': 'completed',
            'processing': 'processing',
            'running': 'processing',
            'pending': 'pending',
            'waiting': 'pending',
            'error': 'error',
            'failed': 'error',
            'fail': 'error'
        }
    }
};

// 图片路径配置
const IMAGE_PATHS = {
    // 根据不同环境的图片路径配置
    development: {
        base: '/static/images/',
        ocr: '/static/images/ocr/',
        documents: '/static/images/documents/'
    },
    production: {
        base: '/static/images/',
        ocr: '/static/images/ocr/', 
        documents: '/static/images/documents/'
    }
};

// 主题配置
const THEME_CONFIG = {
    // 预审主题映射
    themes: {
        'theme_001': '工程渣土准运证核准',
        'theme_002': '工程渣土消纳场地登记',
        'theme_003': '排水接管技术审查',
        'theme_004': '临时占用、挖掘河道设施许可',
        'theme_005': '设置其他户外广告设施和招牌、指示牌备案',
        'theme_006': '利用广场等公共场所举办文化、商业等活动许可'
    },
    
    // 主题到matter_id的映射
    matterMapping: {
        'theme_001': '101104353',
        'theme_002': '101306405', 
        'theme_003': '101102043',
        'theme_004': '101304025',
        'theme_005': '101105083',
        'theme_006': '101303167'
    }
};

// 错误消息配置
const ERROR_MESSAGES = {
    network: '网络连接失败，请检查网络设置',
    timeout: '请求超时，请稍后重试',
    auth: '用户认证失败，请重新登录',
    notFound: '请求的资源不存在',
    serverError: '服务器内部错误，请联系管理员',
    dataFormat: '数据格式错误，请检查输入',
    preview: '预审数据获取失败',
    permission: '权限不足，无法访问该资源'
};

// 工具函数
const ConfigUtils = {
    // 获取API端点URL
    getApiUrl(endpoint, params = {}) {
        let url = API_CONFIG.baseUrl + API_CONFIG.endpoints[endpoint];
        
        // 替换URL中的参数
        Object.keys(params).forEach(key => {
            url = url.replace(`{${key}}`, params[key]);
        });
        
        return url;
    },
    
    // 根据环境获取图片路径
    getImagePath(type = 'base') {
        const env = this.getEnvironment();
        return IMAGE_PATHS[env][type] || IMAGE_PATHS.production[type];
    },
    
    // 检测当前环境
    getEnvironment() {
        const hostname = window.location.hostname;
        if (hostname === 'localhost' || hostname === '127.0.0.1' || hostname.includes('myide.io')) {
            return 'development';
        }
        return 'production';
    },
    
    // 映射数据字段
    mapField(data, fieldMapping) {
        if (typeof fieldMapping === 'function') {
            return fieldMapping(data);
        }
        
        if (Array.isArray(fieldMapping)) {
            for (const field of fieldMapping) {
                if (typeof field === 'function') {
                    const result = field(data);
                    if (result) return result;
                } else {
                    const value = this.getNestedValue(data, field);
                    if (value) return value;
                }
            }
        }
        
        return this.getNestedValue(data, fieldMapping);
    },
    
    // 获取嵌套对象的值
    getNestedValue(obj, path) {
        if (!obj || !path) return null;
        
        const keys = path.split('.');
        let value = obj;
        
        for (const key of keys) {
            if (value && typeof value === 'object' && key in value) {
                value = value[key];
            } else {
                return null;
            }
        }
        
        return value;
    },
    
    // 映射状态
    mapStatus(status, mapping) {
        return mapping[status] || status;
    },
    
    // 获取错误消息
    getErrorMessage(errorType, defaultMessage = '') {
        return ERROR_MESSAGES[errorType] || defaultMessage;
    }
};

// 导出配置
window.CONFIG = {
    API_CONFIG,
    DATA_MAPPING,
    IMAGE_PATHS,
    THEME_CONFIG,
    ERROR_MESSAGES,
    ConfigUtils
};

// 为了兼容性，也导出到全局
window.ApiConfig = API_CONFIG;
window.DataMapping = DATA_MAPPING;
window.ConfigUtils = ConfigUtils;