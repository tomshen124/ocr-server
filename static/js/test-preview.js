/**
 * 预审功能测试模块
 * 包含预审提交、数据测试、完整流程测试等
 */

class PreviewTests {
    constructor(framework) {
        this.framework = framework;
        this.testData = {
            // 标准预审数据
            standard: {
                user_id: "test_user_001",
                third_party_request_id: `test_req_${Date.now()}`,
                matter_id: "330100000000000001",
                matter_name: "企业设立登记",
                form_data: [
                    { code: "DWMC", value: "测试公司有限公司", type: "string" },
                    { code: "legalRep.FDDBR", value: "张三", type: "string" }
                ],
                materials: [
                    {
                        name: "营业执照",
                        file_url: "https://example.com/license.pdf",
                        file_size: 1024000,
                        upload_time: new Date().toISOString()
                    }
                ]
            },
            // 简化测试数据
            minimal: {
                user_id: "test_user_002",
                third_party_request_id: `minimal_test_${Date.now()}`,
                matter_name: "简化测试事项",
                materials: []
            }
        };
    }

    /**
     * 测试预审数据提交
     */
    async testPreviewSubmission(dataType = 'standard') {
        const testData = this.testData[dataType];
        this.framework.showResult('previewSubmissionResult', 
            `🔍 正在测试预审数据提交 (${dataType})...`, 'info');
        
        try {
            const result = await this.framework.request('POST', '/api/preview', testData);
            
            if (result.success) {
                this.framework.showResult('previewSubmissionResult', 
                    '✅ 预审数据提交成功', 'success', {
                        traceId: result.traceId,
                        duration: result.duration,
                        data: {
                            preview_id: result.data.preview_id || '未返回',
                            status: result.data.status || '未知',
                            message: result.data.message
                        }
                    });
            } else {
                this.framework.showResult('previewSubmissionResult', 
                    '❌ 预审数据提交失败', 'error', result);
            }
            
            return result;
        } catch (error) {
            this.framework.showResult('previewSubmissionResult', 
                '❌ 预审提交测试失败', 'error', { error: error.message });
            throw error;
        }
    }

    /**
     * 测试预审数据查询
     */
    async testPreviewDataQuery(previewId = 'test_preview_001') {
        this.framework.showResult('previewQueryResult', 
            `🔍 正在查询预审数据 (${previewId})...`, 'info');
        
        try {
            const result = await this.framework.request('GET', `/api/preview/data/${previewId}`);
            
            if (result.success) {
                this.framework.showResult('previewQueryResult', 
                    '✅ 预审数据查询成功', 'success', {
                        traceId: result.traceId,
                        duration: result.duration,
                        data: {
                            preview_id: previewId,
                            has_data: !!result.data,
                            data_keys: result.data ? Object.keys(result.data) : []
                        }
                    });
            } else if (result.status === 404) {
                this.framework.showResult('previewQueryResult', 
                    '⚠️ 预审数据不存在', 'warning', result);
            } else {
                this.framework.showResult('previewQueryResult', 
                    '❌ 预审数据查询失败', 'error', result);
            }
            
            return result;
        } catch (error) {
            this.framework.showResult('previewQueryResult', 
                '❌ 预审查询测试失败', 'error', { error: error.message });
            throw error;
        }
    }

    /**
     * 测试预审状态查询
     */
    async testPreviewStatus(previewId = 'test_preview_001') {
        this.framework.showResult('previewStatusResult', 
            `🔍 正在查询预审状态 (${previewId})...`, 'info');
        
        try {
            const result = await this.framework.request('GET', `/api/preview/status/${previewId}`);
            
            if (result.success) {
                this.framework.showResult('previewStatusResult', 
                    '✅ 预审状态查询成功', 'success', {
                        traceId: result.traceId,
                        duration: result.duration,
                        data: {
                            preview_id: previewId,
                            status: result.data.status || '未知',
                            progress: result.data.progress || 0
                        }
                    });
            } else {
                this.framework.showResult('previewStatusResult', 
                    '❌ 预审状态查询失败', 'error', result);
            }
            
            return result;
        } catch (error) {
            this.framework.showResult('previewStatusResult', 
                '❌ 预审状态测试失败', 'error', { error: error.message });
            throw error;
        }
    }

    /**
     * 测试完整预审流程
     */
    async testFullPreviewFlow() {
        this.framework.showResult('fullFlowResult', '🚀 开始完整预审流程测试...', 'info');
        
        try {
            // 步骤1: 提交预审数据
            console.log('🔄 步骤1: 提交预审数据');
            const submitResult = await this.testPreviewSubmission('standard');
            
            if (!submitResult.success) {
                throw new Error('预审数据提交失败');
            }
            
            const previewId = submitResult.data?.preview_id || `flow_test_${Date.now()}`;
            
            // 步骤2: 等待处理（模拟）
            console.log('🔄 步骤2: 等待处理完成');
            await new Promise(resolve => setTimeout(resolve, 2000));
            
            // 步骤3: 查询处理状态
            console.log('🔄 步骤3: 查询处理状态');
            const statusResult = await this.testPreviewStatus(previewId);
            
            // 步骤4: 获取处理结果
            console.log('🔄 步骤4: 获取处理结果');
            const dataResult = await this.testPreviewDataQuery(previewId);
            
            this.framework.showResult('fullFlowResult', 
                '✅ 完整预审流程测试完成', 'success', {
                    data: {
                        preview_id: previewId,
                        steps_completed: 4,
                        submit_success: submitResult.success,
                        status_success: statusResult.success,
                        data_success: dataResult.success
                    }
                });
            
            return {
                previewId,
                submitResult,
                statusResult,
                dataResult
            };
            
        } catch (error) {
            this.framework.showResult('fullFlowResult', 
                '❌ 完整预审流程测试失败', 'error', { error: error.message });
            throw error;
        }
    }

    /**
     * 执行预审功能测试套件
     */
    async runPreviewTestSuite() {
        this.framework.showResult('previewTestSuite', '🚀 开始执行预审功能测试套件...', 'info');
        
        const tests = [
            { name: '标准预审提交', execute: () => this.testPreviewSubmission('standard') },
            { name: '简化预审提交', execute: () => this.testPreviewSubmission('minimal') },
            { name: '预审数据查询', execute: () => this.testPreviewDataQuery() },
            { name: '预审状态查询', execute: () => this.testPreviewStatus() }
        ];
        
        const results = await this.framework.runTestSuite('预审功能', tests);
        
        const successCount = results.filter(r => r.success).length;
        const totalCount = results.length;
        
        if (successCount === totalCount) {
            this.framework.showResult('previewTestSuite', 
                `✅ 预审功能测试套件完成 (${successCount}/${totalCount})`, 'success', {
                    data: { results, summary: '所有测试通过' }
                });
        } else {
            this.framework.showResult('previewTestSuite', 
                `⚠️ 预审功能测试套件完成 (${successCount}/${totalCount})`, 'warning', {
                    data: { results, summary: '部分测试失败' }
                });
        }
        
        return results;
    }
}

// 导出给全局使用
window.PreviewTests = PreviewTests;