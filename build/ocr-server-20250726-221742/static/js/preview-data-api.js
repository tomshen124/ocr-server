/**
 * 预审数据API - 从后端获取真实的预审数据
 * 支持模拟测试模式
 */
class PreviewDataAPI {
    constructor() {
        this.baseUrl = '/api';
        this.currentRequestId = null;
        this.testMode = null;
        this.mockConfig = null;
        this.init();
    }

    /**
     * 初始化，获取前端配置
     */
    async init() {
        try {
            const response = await fetch('/api/config/frontend');
            if (response.ok) {
                const config = await response.json();
                if (config.success && config.data) {
                    this.testMode = config.data.test_mode;
                    this.mockConfig = config.data.debug;
                    console.log('前端配置加载成功:', {
                        testMode: this.testMode,
                        mockConfig: this.mockConfig
                    });
                }
            }
        } catch (error) {
            console.warn('获取前端配置失败，使用默认配置:', error);
        }
    }

    /**
     * 设置当前请求ID
     */
    setRequestId(requestId) {
        this.currentRequestId = requestId;
        console.log('设置预审请求ID:', requestId);
    }

    /**
     * 模拟登录（仅测试模式）
     */
    async mockLogin(userId = null, userName = null) {
        if (!this.mockConfig?.mock_login) {
            throw new Error('模拟登录功能未启用');
        }

        const mockUser = {
            userId: userId || '1472176',
            userName: userName || '张三'
        };

        console.log('🧪 执行模拟登录:', mockUser);

        try {
            const response = await fetch(`${this.baseUrl}/dev/mock_login`, {
                method: 'POST',
                credentials: 'include',
                headers: {
                    'Content-Type': 'application/json'
                },
                body: JSON.stringify(mockUser)
            });

            const result = await response.json();
            if (result.success) {
                console.log('✅ 模拟登录成功:', result.data);
                return result.data;
            } else {
                throw new Error(result.errorMsg || '模拟登录失败');
            }
        } catch (error) {
            console.error('❌ 模拟登录失败:', error);
            throw error;
        }
    }

    /**
     * 检查认证状态
     */
    async checkAuthStatus() {
        try {
            const response = await fetch(`${this.baseUrl}/auth/status`, {
                method: 'GET',
                credentials: 'include'
            });

            if (response.ok) {
                const result = await response.json();
                return result;
            }
            return { authenticated: false };
        } catch (error) {
            console.warn('检查认证状态失败:', error);
            return { authenticated: false };
        }
    }

    /**
     * 获取预审数据
     */
    async getPreviewData(requestId = null) {
        const id = requestId || this.currentRequestId;

        // 如果是测试模式且没有requestId，使用模拟数据
        if (this.testMode?.enabled && !id) {
            console.log('🧪 测试模式：使用模拟预审数据');
            return this.getMockData();
        }

        if (!id) {
            throw new Error('缺少预审请求ID');
        }

        try {
            console.log('获取预审数据:', id);
            // 使用正确的预审结果接口，这个接口返回结构化数据
            const response = await fetch(`${this.baseUrl}/preview/result/${id}`, {
                method: 'GET',
                credentials: 'include',
                headers: {
                    'Content-Type': 'application/json'
                }
            });

            if (!response.ok) {
                // 如果是测试模式，降级到模拟数据
                if (this.testMode?.enabled) {
                    console.warn('⚠️ 获取真实数据失败，使用模拟数据');
                    return this.getMockData();
                }
                throw new Error(`获取预审数据失败: ${response.status}`);
            }

            const result = await response.json();
            console.log('预审数据获取成功:', result);

            // 检查响应格式
            if (result.success && result.data) {
                return this.transformToUIData(result.data);
            } else {
                // 如果响应格式不正确，使用模拟数据
                console.warn('⚠️ 响应格式不正确，使用模拟数据');
                return this.getMockData();
            }
        } catch (error) {
            console.error('获取预审数据失败:', error);
            // 返回模拟数据作为降级方案
            if (this.testMode?.enabled) {
                console.warn('⚠️ 使用模拟数据作为降级方案');
                return this.getMockData();
            }
            throw error;
        }
    }

    /**
     * 将后端数据转换为前端UI需要的格式
     */
    transformToUIData(backendData) {
        // 检查是否是预审结果数据（来自 /api/preview/result/{id}）
        if (backendData.materials && Array.isArray(backendData.materials)) {
            return this.transformPreviewResult(backendData);
        }

        // 如果后端返回的是评估结果数据
        if (backendData.basic_info && backendData.material_results) {
            return this.transformEvaluationResult(backendData);
        }

        // 如果是原始预审数据
        return this.transformRawPreviewData(backendData);
    }

    /**
     * 转换预审结果数据（来自 /api/preview/result/{id}）
     */
    transformPreviewResult(previewResult) {
        const materials = previewResult.materials || [];

        // 计算统计信息
        const stats = materials.reduce((acc, material) => {
            acc.total++;
            switch (material.status) {
                case 'passed':
                    acc.passed++;
                    break;
                case 'failed':
                    acc.failed++;
                    break;
                case 'warning':
                    acc.warning++;
                    break;
                default:
                    acc.pending++;
            }
            return acc;
        }, { total: 0, passed: 0, failed: 0, warning: 0, pending: 0 });

        return {
            basicInfo: {
                applicantName: previewResult.applicant_name || previewResult.applicant || '未知申请人',
                applicantId: previewResult.applicant_id || '',
                agentName: previewResult.agent_name || '未知经办人',
                agentId: previewResult.agent_id || '',
                matterName: previewResult.matter_name || '未知事项',
                matterId: previewResult.matter_id || '',
                requestId: previewResult.preview_id || '',
                sequenceNo: previewResult.sequence_no || '',
                themeId: previewResult.theme_id || '',
                themeName: previewResult.theme_name || '默认主题'
            },

            materialsToReview: materials.map(material => ({
                id: material.id || `material_${Math.random()}`,
                name: material.name || '未知材料',
                code: material.code || '',
                status: material.status || 'pending',
                statusText: this.getStatusText(material.status),
                statusClass: `status-${material.status || 'pending'}`,
                required: material.required !== false,
                description: material.description || '',
                image: material.image || material.preview_url,
                attachments: material.attachments || [],
                reviewPoints: material.review_points || [],
                subItems: material.subItems || []
            })),

            summary: {
                totalMaterials: stats.total,
                passedMaterials: stats.passed,
                failedMaterials: stats.failed,
                warningMaterials: stats.warning,
                pendingMaterials: stats.pending,
                overallResult: this.getOverallResult(stats),
                suggestions: previewResult.suggestions || []
            },

            metadata: {
                evaluationTime: previewResult.created_at || new Date().toISOString(),
                version: '1.0',
                source: 'preview_result'
            }
        };
    }

    /**
     * 转换评估结果数据
     */
    transformEvaluationResult(evaluationResult) {
        const basicInfo = evaluationResult.basic_info;
        const materialResults = evaluationResult.material_results || [];
        const summary = evaluationResult.evaluation_summary || {};

        return {
            // 基本信息
            basicInfo: {
                applicantName: basicInfo.applicant_name || '未知申请人',
                applicantId: basicInfo.applicant_id || '',
                agentName: basicInfo.agent_name || '未知经办人',
                agentId: basicInfo.agent_id || '',
                matterName: basicInfo.matter_name || '未知事项',
                matterId: basicInfo.matter_id || '',
                requestId: basicInfo.request_id || '',
                sequenceNo: basicInfo.sequence_no || '',
                themeId: basicInfo.theme_id || 'default',
                themeName: basicInfo.theme_name || '默认规则'
            },

            // 材料检查列表
            materialsToReview: materialResults.map((material, index) => {
                const ruleResult = material.rule_evaluation || {};
                const status = this.getStatusFromRuleResult(ruleResult);
                
                return {
                    id: `material_${index + 1}`,
                    name: material.material_name || material.material_code || `材料${index + 1}`,
                    code: material.material_code || `CODE_${index + 1}`,
                    status: status,
                    statusText: this.getStatusText(status),
                    statusClass: this.getStatusClass(status),
                    required: true,
                    description: ruleResult.description || '请提供相关材料',
                    attachments: material.attachments || [],
                    ocrContent: material.ocr_content || '',
                    ruleResult: ruleResult,
                    subItems: this.generateSubItems(material)
                };
            }),

            // 评估摘要
            summary: {
                totalMaterials: summary.total_materials || 0,
                passedMaterials: summary.passed_materials || 0,
                failedMaterials: summary.failed_materials || 0,
                warningMaterials: summary.warning_materials || 0,
                overallResult: summary.overall_result || 'Passed',
                suggestions: summary.overall_suggestions || []
            },

            // 元数据
            metadata: {
                evaluationTime: evaluationResult.evaluation_time || new Date().toISOString(),
                version: '1.0',
                source: 'backend_evaluation'
            }
        };
    }

    /**
     * 转换原始预审数据
     */
    transformRawPreviewData(rawData) {
        // 处理原始的第三方请求数据
        const agentInfo = rawData.agentInfo || rawData.agent_info || {};
        const subjectInfo = rawData.subjectInfo || rawData.subject_info || {};
        const materialData = rawData.materialData || rawData.material_data || [];

        return {
            basicInfo: {
                applicantName: subjectInfo.userName || subjectInfo.user_name || '未知申请人',
                applicantId: subjectInfo.userId || subjectInfo.user_id || '',
                agentName: agentInfo.userName || agentInfo.user_name || '',
                agentId: agentInfo.userId || agentInfo.user_id || '',
                matterName: rawData.matterName || rawData.matter_name || '未知事项',
                matterId: rawData.matterId || rawData.matter_id || '',
                requestId: rawData.requestId || rawData.request_id || '',
                sequenceNo: rawData.sequenceNo || rawData.sequence_no || '',
                themeId: rawData.themeId || 'default',
                themeName: '待评估'
            },

            materialsToReview: materialData.map((material, index) => ({
                id: `material_${index + 1}`,
                name: material.name || material.code || `材料${index + 1}`,
                code: material.code || `CODE_${index + 1}`,
                status: 'pending',
                statusText: '待检查',
                statusClass: 'status-pending',
                required: material.required !== false,
                description: material.description || '请提供相关材料',
                attachments: material.attachmentList || material.attachment_list || [],
                subItems: []
            })),

            summary: {
                totalMaterials: materialData.length,
                passedMaterials: 0,
                failedMaterials: 0,
                warningMaterials: 0,
                overallResult: 'Pending',
                suggestions: []
            },

            metadata: {
                evaluationTime: new Date().toISOString(),
                version: '1.0',
                source: 'raw_preview_data'
            }
        };
    }

    /**
     * 根据规则评估结果确定状态
     */
    getStatusFromRuleResult(ruleResult) {
        if (!ruleResult || !ruleResult.overall_status) {
            return 'pending';
        }

        switch (ruleResult.overall_status) {
            case 'Passed':
                return 'passed';
            case 'Failed':
                return 'failed';
            case 'Warning':
                return 'warning';
            default:
                return 'pending';
        }
    }

    /**
     * 获取状态文本
     */
    getStatusText(status) {
        const statusMap = {
            'passed': '通过',
            'failed': '不通过',
            'warning': '有问题',
            'pending': '待检查'
        };
        return statusMap[status] || '未知';
    }

    /**
     * 获取状态样式类
     */
    getStatusClass(status) {
        return `status-${status}`;
    }

    /**
     * 生成子项目（如果材料有多个附件）
     */
    generateSubItems(material) {
        const attachments = material.attachments || [];
        if (attachments.length <= 1) {
            return [];
        }

        return attachments.map((attachment, index) => ({
            id: `${material.material_code}_sub_${index + 1}`,
            name: attachment.name || `附件${index + 1}`,
            status: 'pending',
            statusText: '待检查',
            statusClass: 'status-pending',
            attachmentInfo: attachment
        }));
    }

    /**
     * 获取模拟数据（降级方案）
     */
    getMockData() {
        console.warn('使用模拟数据作为降级方案');
        
        return {
            basicInfo: {
                applicantName: '浙江某建设公司',
                applicantId: 'COMP_12345',
                agentName: '张三',
                agentId: 'USER_67890',
                matterName: '工程渣土准运证核准',
                matterId: '101104353',
                requestId: 'REQ_' + Date.now(),
                sequenceNo: 'SEQ_001',
                themeId: 'theme_001',
                themeName: '工程渣土准运证核准'
            },

            materialsToReview: [
                {
                    id: 'material_1',
                    name: '营业执照',
                    code: 'BUSINESS_LICENSE',
                    status: 'passed',
                    statusText: '通过',
                    statusClass: 'status-passed',
                    required: true,
                    description: '企业营业执照副本',
                    attachments: [
                        { name: '营业执照.pdf', size: '2.1MB', type: 'pdf' }
                    ],
                    subItems: []
                },
                {
                    id: 'material_2',
                    name: '建设工程规划许可证',
                    code: 'CONSTRUCTION_PERMIT',
                    status: 'warning',
                    statusText: '有问题',
                    statusClass: 'status-warning',
                    required: true,
                    description: '建设工程规划许可证原件或复印件',
                    attachments: [
                        { name: '规划许可证.jpg', size: '1.8MB', type: 'image' }
                    ],
                    subItems: []
                },
                {
                    id: 'material_3',
                    name: '渣土处置协议',
                    code: 'DISPOSAL_AGREEMENT',
                    status: 'failed',
                    statusText: '不通过',
                    statusClass: 'status-failed',
                    required: true,
                    description: '与有资质的渣土处置单位签订的处置协议',
                    attachments: [],
                    subItems: []
                }
            ],

            summary: {
                totalMaterials: 3,
                passedMaterials: 1,
                failedMaterials: 1,
                warningMaterials: 1,
                overallResult: 'RequiresAdditionalMaterials',
                suggestions: [
                    '请补充渣土处置协议',
                    '建设工程规划许可证需要重新拍照，当前图片不够清晰'
                ]
            },

            metadata: {
                evaluationTime: new Date().toISOString(),
                version: '1.0',
                source: 'mock_data'
            }
        };
    }

    /**
     * 获取状态文本
     */
    getStatusText(status) {
        const statusMap = {
            'passed': '通过',
            'failed': '不通过',
            'warning': '有问题',
            'pending': '待审核'
        };
        return statusMap[status] || '未知状态';
    }

    /**
     * 获取整体结果
     */
    getOverallResult(stats) {
        if (stats.failed > 0) return 'RequiresAdditionalMaterials';
        if (stats.warning > 0) return 'RequiresCorrection';
        if (stats.passed === stats.total && stats.total > 0) return 'Approved';
        return 'Pending';
    }
}

// 全局实例
window.previewDataAPI = new PreviewDataAPI();
