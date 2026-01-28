/**
 * API增强功能测试模块
 * 专门测试新的trace_id、错误处理、结构化响应等功能
 */

class APIEnhancementTests {
    constructor(framework) {
        this.framework = framework;
    }

    /**
     * 测试Trace ID功能
     */
    async testTraceIdFeature() {
        this.framework.showResult('traceIdResult', '🔍 正在测试Trace ID功能...', 'info');
        
        try {
            // 测试健康检查接口的trace_id
            const result = await this.framework.request('GET', '/api/health');
            
            if (result.traceId) {
                this.framework.showResult('traceIdResult', 
                    `✅ Trace ID功能正常工作`, 'success', {
                        traceId: result.traceId,
                        duration: result.duration,
                        data: { message: 'API返回了trace_id响应头' }
                    });
            } else {
                this.framework.showResult('traceIdResult', 
                    '⚠️ 未检测到Trace ID功能，可能未启用', 'warning', result);
            }
            
            return result;
        } catch (error) {
            this.framework.showResult('traceIdResult', 
                '❌ Trace ID测试失败', 'error', { error: error.message });
            throw error;
        }
    }

    /**
     * 测试增强错误处理
     */
    async testEnhancedErrorHandling() {
        this.framework.showResult('errorHandlingResult', '🔍 正在测试增强错误处理...', 'info');
        
        try {
            // 故意发送无效请求来触发错误处理
            const result = await this.framework.request('POST', '/api/preview', {
                invalid_data: 'this should fail'
            });
            
            // 检查错误响应格式
            if (!result.success && result.data) {
                const errorData = result.data;
                const hasEnhancedFields = errorData.user_msg || errorData.trace_id || errorData.timestamp;
                
                if (hasEnhancedFields) {
                    this.framework.showResult('errorHandlingResult', 
                        '✅ 增强错误处理功能正常', 'success', {
                            traceId: result.traceId,
                            duration: result.duration,
                            data: {
                                enhanced: true,
                                error_msg: errorData.error_msg,
                                user_msg: errorData.user_msg,
                                has_trace_id: !!errorData.trace_id
                            }
                        });
                } else {
                    this.framework.showResult('errorHandlingResult', 
                        '⚠️ 使用传统错误处理格式', 'warning', result);
                }
            } else {
                this.framework.showResult('errorHandlingResult', 
                    '⚠️ 未能触发错误处理测试', 'warning', result);
            }
            
            return result;
        } catch (error) {
            this.framework.showResult('errorHandlingResult', 
                '❌ 错误处理测试失败', 'error', { error: error.message });
            throw error;
        }
    }

    /**
     * 测试结构化响应格式
     */
    async testStructuredResponse() {
        this.framework.showResult('structuredResponseResult', '🔍 正在测试结构化响应格式...', 'info');
        
        try {
            const result = await this.framework.request('GET', '/api/auth/status');
            
            // 检查响应是否包含结构化字段
            if (result.data && typeof result.data === 'object') {
                const hasStructuredFields = result.data.success !== undefined || 
                                          result.data.timestamp !== undefined ||
                                          result.data.trace_id !== undefined;
                
                if (hasStructuredFields) {
                    this.framework.showResult('structuredResponseResult', 
                        '✅ 结构化响应格式正常', 'success', {
                            traceId: result.traceId,
                            duration: result.duration,
                            data: {
                                structured: true,
                                fields: Object.keys(result.data)
                            }
                        });
                } else {
                    this.framework.showResult('structuredResponseResult', 
                        '⚠️ 使用传统响应格式', 'warning', result);
                }
            } else {
                this.framework.showResult('structuredResponseResult', 
                    '⚠️ 响应数据格式异常', 'warning', result);
            }
            
            return result;
        } catch (error) {
            this.framework.showResult('structuredResponseResult', 
                '❌ 结构化响应测试失败', 'error', { error: error.message });
            throw error;
        }
    }

    /**
     * 执行完整的API增强功能测试套件
     */
    async runFullSuite() {
        this.framework.showResult('apiEnhancementSuite', '🚀 开始执行API增强功能测试套件...', 'info');
        
        const tests = [
            { name: 'Trace ID功能', execute: () => this.testTraceIdFeature() },
            { name: '增强错误处理', execute: () => this.testEnhancedErrorHandling() },
            { name: '结构化响应', execute: () => this.testStructuredResponse() }
        ];
        
        const results = await this.framework.runTestSuite('API增强功能', tests);
        
        const successCount = results.filter(r => r.success).length;
        const totalCount = results.length;
        
        if (successCount === totalCount) {
            this.framework.showResult('apiEnhancementSuite', 
                `✅ API增强功能测试套件完成 (${successCount}/${totalCount})`, 'success', {
                    data: { results, summary: '所有测试通过' }
                });
        } else {
            this.framework.showResult('apiEnhancementSuite', 
                `⚠️ API增强功能测试套件完成 (${successCount}/${totalCount})`, 'warning', {
                    data: { results, summary: '部分测试失败' }
                });
        }
        
        return results;
    }
}

// 导出给全局使用
window.APIEnhancementTests = APIEnhancementTests;