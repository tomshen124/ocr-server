// API 接口管理模塊
class ApiService {
    constructor() {
        this.config = window.CONFIG || {};
        this.baseUrl = this.config.API_CONFIG?.baseUrl || '/api';
        this.timeout = this.config.API_CONFIG?.timeout || 30000;
        this.utils = this.config.ConfigUtils || {};
    }

    // 通用請求方法
    async request(url, options = {}) {
        const config = {
            method: 'GET',
            headers: {
                'Content-Type': 'application/json',
                ...options.headers
            },
            timeout: this.timeout,
            ...options
        };

        try {
            const response = await fetch(`${this.baseUrl}${url}`, config);
            
            if (!response.ok) {
                throw new Error(`HTTP error! status: ${response.status}`);
            }
            
            const data = await response.json();
            return {
                success: true,
                data: data,
                message: 'success'
            };
        } catch (error) {
            console.error('API request failed:', error);
            return {
                success: false,
                data: null,
                message: error.message
            };
        }
    }

    // 获取预审数据 - 映射到后端实际接口
    async getBasicInfo(previewId) {
        const url = this.utils.getApiUrl ? 
            this.utils.getApiUrl('previewData', { previewId }) : 
            `/preview/data/${previewId}`;
        return await this.request(url);
    }

    // 获取审核材料列表 - 映射到后端实际接口
    async getMaterialsList(previewId) {
        const url = this.utils.getApiUrl ? 
            this.utils.getApiUrl('previewData', { previewId }) : 
            `/preview/data/${previewId}`;
        return await this.request(url);
    }

    // 获取审核状态 - 映射到后端实际接口
    async getAuditStatus(previewId) {
        const url = this.utils.getApiUrl ? 
            this.utils.getApiUrl('previewData', { previewId }) : 
            `/preview/data/${previewId}`;
        return await this.request(url);
    }

    // 开始智能预审 - 映射到现有的预审接口
    async startAudit(requestData) {
        return await this.request(`/preview`, {
            method: 'POST',
            body: JSON.stringify(requestData)
        });
    }

    // 获取审核进度 - 映射到后端实际接口
    async getAuditProgress(previewId) {
        return await this.request(`/preview/data/${previewId}`);
    }

    // 获取文档预览 - 映射到后端实际接口
    async getDocumentPreview(previewId) {
        return await this.request(`/preview/data/${previewId}`);
    }

    // 导出材料 - 使用下载接口
    async exportMaterials(previewId) {
        window.open(`${this.baseUrl}/download?file=${previewId}.pdf`, '_blank');
        return { success: true, message: '下载已开始' };
    }

    // 下载检查要素清单
    async downloadCheckList(previewId) {
        window.open(`${this.baseUrl}/download?file=${previewId}.pdf`, '_blank');
        return { success: true, message: '下载已开始' };
    }

    // 获取前端配置
    async getFrontendConfig() {
        return await this.request('/config/frontend');
    }

    // 检查认证状态
    async checkAuthStatus() {
        return await this.request('/auth/status');
    }

    // 模拟登录（仅测试模式）
    async mockLogin(userData) {
        return await this.request('/test/mock_login', {
            method: 'POST',
            body: JSON.stringify(userData)
        });
    }

    // 获取系统队列状态 - 并发控制监控
    async getQueueStatus() {
        return await this.request('/queue/status');
    }

    // 数据转换方法 - 将后端数据转换为前端格式
    transformPreviewData(backendData) {
        if (!backendData || !backendData.success) {
            return null;
        }

        const data = backendData.data;
        console.log('转换后端数据:', data);

        // 从预审数据中提取基本信息 - 使用配置化映射
        const mapping = this.config.DATA_MAPPING?.preview?.basicInfo || {};
        const basicInfo = {
            applicant: this.utils.mapField ? 
                this.utils.mapField(data, mapping.applicant) || "申请人" :
                this.extractFormValue(data, 'legalRep.FDDBR') || this.extractFormValue(data, 'self.DWMC') || "申请人",
            applicationType: this.utils.mapField ? 
                this.utils.mapField(data, mapping.applicationType) || "业务类型" :
                data.matter_name || "业务类型",
            auditOrgan: this.utils.mapField ? 
                this.utils.mapField(data, mapping.auditOrgan) || "智能预审系统" :
                "智能预审系统"
        };

        // 转换材料列表 - 基于OCR结果和规则评估
        const materials = [];
        if (data.evaluation_result && data.evaluation_result.rules) {
            data.evaluation_result.rules.forEach((rule, index) => {
                const hasIssues = rule.result === 'failed' || rule.result === 'warning';
                materials.push({
                    id: index + 1,
                    name: rule.description || rule.field || `检查项${index + 1}`,
                    count: 1,
                    status: this.mapStatus(rule.result),
                    expanded: false,
                    items: [{
                        id: (index + 1) * 100 + 1,
                        name: rule.field || rule.description,
                        status: this.mapStatus(rule.result),
                        hasDocument: !!rule.ocr_text,
                        documentId: `doc_${index}`,
                        documentType: 'ocr_result',
                        checkPoint: rule.message || rule.details || '检查完成'
                    }]
                });
            });
        }

        // 转换已通过材料
        const passedMaterials = [];
        if (data.evaluation_result && data.evaluation_result.passed_items) {
            data.evaluation_result.passed_items.forEach((item, index) => {
                passedMaterials.push({
                    id: index + 1,
                    name: item.name || item.description || `已通过项${index + 1}`
                });
            });
        }

        // 转换审核状态
        const overallResult = this.calculateOverallResult(data.evaluation_result);
        const auditStatus = {
            status: 'completed',
            result: overallResult,
            progress: 100,
            estimatedTime: 0,
            message: this.generateStatusMessage(overallResult, materials.length)
        };

        return {
            basicInfo,
            materials,
            passedMaterials,
            auditStatus
        };
    }

    // 从表单数据中提取值
    extractFormValue(data, fieldCode) {
        if (data.form_data && Array.isArray(data.form_data)) {
            const field = data.form_data.find(f => f.code === fieldCode);
            return field ? field.value : null;
        }
        return null;
    }

    // 计算整体结果
    calculateOverallResult(evaluation) {
        if (!evaluation || !evaluation.rules) {
            return 'passed';
        }
        
        const failedCount = evaluation.rules.filter(r => r.result === 'failed').length;
        const warningCount = evaluation.rules.filter(r => r.result === 'warning').length;
        
        if (failedCount > 0) return 'error';
        if (warningCount > 0) return 'hasIssues';
        return 'passed';
    }

    // 生成状态消息
    generateStatusMessage(result, materialsCount) {
        switch (result) {
            case 'passed': return '智能预审通过，所有材料符合要求';
            case 'hasIssues': return `发现${materialsCount}个需要注意的问题`;
            case 'error': return '发现重要问题，请检查相关材料';
            default: return '智能预审完成';
        }
    }

    // 状态映射 - 使用配置化映射
    mapStatus(backendStatus) {
        const statusMap = this.config.DATA_MAPPING?.preview?.statusMapping || {
            'success': 'passed',
            'passed': 'passed',
            'warning': 'warning',
            'error': 'error',
            'failed': 'error',
            'pending': 'loading',
            'processing': 'loading'
        };
        return this.utils.mapStatus ? 
            this.utils.mapStatus(backendStatus, statusMap) :
            statusMap[backendStatus] || 'passed';
    }

    // 审核状态映射
    mapAuditStatus(backendStatus) {
        const statusMap = this.config.DATA_MAPPING?.preview?.auditStatusMapping || {
            'completed': 'completed',
            'processing': 'processing',
            'pending': 'pending',
            'error': 'error',
            'failed': 'error'
        };
        return this.utils.mapStatus ? 
            this.utils.mapStatus(backendStatus, statusMap) :
            statusMap[backendStatus] || 'completed';
    }

    // 获取默认数据（当API请求失败时使用）
    getDefaultData() {
        return {
            basicInfo: {
                applicant: "浙江一二三科技有限公司",
                applicationType: "內資公司變更", 
                auditOrgan: "經營範圍"
            },
            materials: [
                {
                    id: 1,
                    name: '《內資公司變更登記申請書》',
                    count: 2,
                    status: 'hasIssues', // passed, hasIssues, error
                    expanded: false,
                    items: [
                        { 
                            id: 101,
                            name: '法人代表簽字', 
                            status: 'passed', 
                            hasDocument: false,
                            checkPoint: '需要法人代表親筆簽字'
                        },
                        { 
                            id: 102,
                            name: '法人簽章', 
                            status: 'warning', 
                            hasDocument: false,
                            checkPoint: '簽章不清晰，建議重新蓋章'
                        }
                    ]
                },
                {
                    id: 2,
                    name: '《營業執照副本》',
                    count: 5,
                    status: 'passed',
                    expanded: false,
                    items: [
                        { 
                            id: 201,
                            name: '營業執照', 
                            status: 'passed', 
                            hasDocument: true, 
                            documentId: 'doc_001',
                            documentType: 'license'
                        },
                        { 
                            id: 202,
                            name: '法人代表簽字', 
                            status: 'passed', 
                            hasDocument: false 
                        },
                        { 
                            id: 203,
                            name: '法人簽章', 
                            status: 'passed', 
                            hasDocument: false 
                        },
                        { 
                            id: 204,
                            name: '企業法人簽章', 
                            status: 'passed', 
                            hasDocument: false 
                        },
                        { 
                            id: 205,
                            name: '企業法人簽章副本', 
                            status: 'passed', 
                            hasDocument: false 
                        }
                    ]
                },
                {
                    id: 3,
                    name: '《章程修正案》',
                    count: 4,
                    status: 'warning',
                    expanded: false,
                    items: [
                        { 
                            id: 301,
                            name: '章程修正案', 
                            status: 'warning', 
                            hasDocument: true, 
                            documentId: 'doc_002',
                            documentType: 'table',
                            checkPoint: '第3項內容需要補充完整'
                        },
                        { 
                            id: 302,
                            name: '法人代表簽字', 
                            status: 'passed', 
                            hasDocument: false 
                        },
                        { 
                            id: 303,
                            name: '法人簽章', 
                            status: 'passed', 
                            hasDocument: false 
                        },
                        { 
                            id: 304,
                            name: '企業法人簽章', 
                            status: 'passed', 
                            hasDocument: false 
                        }
                    ]
                }
            ],
            passedMaterials: [
                { id: 1, name: '《公司變更登記》' }
            ],
            auditStatus: {
                status: 'completed', // pending, processing, completed, error
                result: 'hasIssues', // passed, hasIssues, error
                progress: 100,
                estimatedTime: 0,
                message: '智能预审完成，发现2个需要注意的问题'
            }
        };
    }
}

// 导出API服务实例
window.apiService = new ApiService();