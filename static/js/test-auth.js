/**
 * 认证功能测试模块
 * 包含SSO登录、模拟登录、会话管理等测试
 */

class AuthenticationTests {
    constructor(framework) {
        this.framework = framework;
    }

    /**
     * 测试SSO登录跳转
     */
    async testSSOLogin() {
        const returnUrl = document.getElementById('returnUrl')?.value || '/static/index.html';
        this.framework.showResult('ssoResult', '🚀 正在测试SSO登录跳转...', 'info');
        
        try {
            const loginUrl = `/api/sso/login?return_url=${encodeURIComponent(returnUrl)}`;
            
            // 这里不能直接跳转，而是检查登录URL的构造
            const result = await this.framework.request('GET', loginUrl, null, {
                redirect: 'manual' // 不自动跟随重定向
            });
            
            if (result.status === 302 || result.status === 307) {
                this.framework.showResult('ssoResult', 
                    '✅ SSO登录跳转正常工作', 'success', {
                        traceId: result.traceId,
                        duration: result.duration,
                        data: {
                            redirect_status: result.status,
                            location: result.headers.location
                        }
                    });
            } else {
                this.framework.showResult('ssoResult', 
                    '⚠️ SSO登录响应异常', 'warning', result);
            }
            
            return result;
        } catch (error) {
            this.framework.showResult('ssoResult', 
                '❌ SSO登录测试失败', 'error', { error: error.message });
            throw error;
        }
    }

    /**
     * 测试调试票据认证（替代模拟登录）
     */
    async testDebugTicketAuth() {
        const userId = document.getElementById('mockUserId')?.value || 'debug_user_001';
        const userName = document.getElementById('mockUserName')?.value || '调试测试用户';
        
        this.framework.showResult('mockLoginResult', '🔍 正在执行调试票据认证...', 'info');
        
        try {
            // 使用预定义的debug ticket ID
            const debugTicketId = 'debug_tk_e4a0dc3fcc8d464ba336b9bcb1ba2072';
            
            const result = await this.framework.request('POST', '/api/verify_user', {
                ticketId: debugTicketId  // 使用ticketId而不是ticket_id
            });
            
            if (result.success) {
                this.framework.showResult('mockLoginResult', 
                    '✅ 调试票据认证成功', 'success', {
                        traceId: result.traceId,
                        duration: result.duration,
                        data: {
                            user_id: result.data?.userId || userId,
                            user_name: result.data?.userName || userName,
                            ticket_id: debugTicketId,
                            debug_mode: result.data?.debugMode || false,
                            login_method: 'debug_ticket',
                            message: result.data?.message || '调试票据认证成功',
                            redirect_url: result.data?.redirectUrl
                        }
                    });
            } else {
                this.framework.showResult('mockLoginResult', 
                    '❌ 调试票据认证失败', 'error', result);
            }
            
            return result;
        } catch (error) {
            this.framework.showResult('mockLoginResult', 
                '❌ 调试票据认证测试失败', 'error', { error: error.message });
            throw error;
        }
    }

    /**
     * 检查认证状态
     */
    async testAuthStatus() {
        this.framework.showResult('authStatusResult', '🔍 正在检查认证状态...', 'info');
        
        try {
            const result = await this.framework.request('GET', '/api/auth/status');
            
            if (result.success && result.data) {
                const isAuthenticated = result.data.authenticated || result.data.user_id;
                
                if (isAuthenticated) {
                    this.framework.showResult('authStatusResult', 
                        '✅ 用户已认证', 'success', {
                            traceId: result.traceId,
                            duration: result.duration,
                            data: {
                                authenticated: true,
                                user_info: result.data
                            }
                        });
                } else {
                    this.framework.showResult('authStatusResult', 
                        '⚠️ 用户未认证', 'warning', result);
                }
            } else {
                this.framework.showResult('authStatusResult', 
                    '❌ 认证状态检查失败', 'error', result);
            }
            
            return result;
        } catch (error) {
            this.framework.showResult('authStatusResult', 
                '❌ 认证状态测试失败', 'error', { error: error.message });
            throw error;
        }
    }

    /**
     * 测试会话清理
     */
    async testClearSession() {
        this.framework.showResult('clearSessionResult', '🔍 正在清理会话...', 'info');
        
        try {
            // 注意：这个接口可能不存在，需要根据实际情况调整
            const result = await this.framework.request('POST', '/api/clear-session');
            
            this.framework.showResult('clearSessionResult', 
                '✅ 会话清理请求已发送', 'success', {
                    traceId: result.traceId,
                    duration: result.duration,
                    data: result.data
                });
            
            return result;
        } catch (error) {
            this.framework.showResult('clearSessionResult', 
                '❌ 会话清理失败', 'error', { error: error.message });
            throw error;
        }
    }

    /**
     * 执行完整的认证测试套件
     */
    async runAuthTestSuite() {
        this.framework.showResult('authTestSuite', '🚀 开始执行认证功能测试套件...', 'info');
        
        const tests = [
            { name: '认证状态检查', execute: () => this.testAuthStatus() },
            { name: '调试票据认证', execute: () => this.testDebugTicketAuth() },
            { name: '会话清理', execute: () => this.testClearSession() }
        ];
        
        const results = await this.framework.runTestSuite('认证功能', tests);
        
        const successCount = results.filter(r => r.success).length;
        const totalCount = results.length;
        
        if (successCount === totalCount) {
            this.framework.showResult('authTestSuite', 
                `✅ 认证功能测试套件完成 (${successCount}/${totalCount})`, 'success', {
                    data: { results, summary: '所有测试通过' }
                });
        } else {
            this.framework.showResult('authTestSuite', 
                `⚠️ 认证功能测试套件完成 (${successCount}/${totalCount})`, 'warning', {
                    data: { results, summary: '部分测试失败或跳过' }
                });
        }
        
        return results;
    }
}

// 导出给全局使用
window.AuthenticationTests = AuthenticationTests;