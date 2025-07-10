// 认证管理模块
const AUTH = {
    // 当前预审相关的用户ID（来自之前的preview调用）
    expectedUserId: null,

    // 检查用户是否已登录 - 更严格的会话检查
    async checkAuth() {
        try {
            // 首先检查会话状态
            const response = await fetch('/api/auth/status', {
                method: 'GET',
                credentials: 'include'  // 确保发送会话cookie
            });

            const result = await response.json();
            
            if (result.authenticated && result.user) {
                console.log('用户已认证:', result.user.userId);
                
                // 如果有预期的用户ID，进行一致性检查
                if (this.expectedUserId && this.expectedUserId !== result.user.userId) {
                    console.warn('用户ID不一致:', {
                        expected: this.expectedUserId,
                        current: result.user.userId
                    });
                    this.redirectToLogin('用户身份不匹配，请重新登录');
                    return false;
                }
                
                return result.user;
            } else {
                console.log('用户未认证或会话已过期');
                this.redirectToLogin();
                return false;
            }
        } catch (error) {
            console.error('认证检查失败:', error);
            this.redirectToLogin('网络错误，请重试');
            return false;
        }
    },

    // 设置预期的用户ID（用于从其他系统跳转时的验证）
    setExpectedUserId(userId) {
        this.expectedUserId = userId;
        console.log('设置预期用户ID:', userId);
    },

    // 从URL参数获取预期的用户ID和请求ID
    getExpectedUserIdFromUrl() {
        const urlParams = new URLSearchParams(window.location.search);
        const userId = urlParams.get('userId') || urlParams.get('user_id') || urlParams.get('user');
        const requestId = urlParams.get('requestId');
        const verified = urlParams.get('verified');
        
        if (userId) {
            this.setExpectedUserId(userId);
        }
        
        // 如果有requestId和verified标记，说明是通过安全接口访问的
        if (requestId && verified === 'true') {
            this.currentRequestId = requestId;
            console.log('安全访问模式，请求ID:', requestId);
        }
        
        return userId;
    },

    // 获取当前预审请求ID
    getCurrentRequestId() {
        return this.currentRequestId || null;
    },

    // 获取用户信息（兼容旧接口）
    async getUserInfo() {
        try {
            const ticketId = sessionStorage.getItem('ticketId');
            const appId = this.getAppId();
            
            // 获取token
            const tokenResponse = await fetch('/api/get_token', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json'
                },
                body: JSON.stringify({ ticketId, appId })
            });

            const tokenResult = await tokenResponse.json();
            if (!tokenResult.success) {
                throw new Error('获取token失败');
            }

            const token = tokenResult.data;

            // 获取用户信息
            const userResponse = await fetch('/api/user_info', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json'
                },
                body: JSON.stringify({ token })
            });

            const userResult = await userResponse.json();
            if (!userResult.success) {
                throw new Error('获取用户信息失败');
            }

            return userResult.data;
        } catch (error) {
            console.error('获取用户信息失败:', error);
            return null;
        }
    },

    // 获取应用ID（从配置或环境变量中）
    getAppId() {
        // 这里可以根据实际情况修改
        return 'default_app_id';
    },

    // 重定向到登录页
    redirectToLogin(message = null) {
        let loginUrl = '/static/login.html';
        if (message) {
            loginUrl += `?error=${encodeURIComponent(message)}`;
        }
        console.log('重定向到登录页:', loginUrl);
        window.location.href = loginUrl;
    },

    // 重定向到第三方SSO登录
    redirectToSSOLogin() {
        // 构建SSO登录URL，回调地址指向我们的SSO回调接口
        const callbackUrl = window.location.origin + '/api/sso/callback';
        const ssoUrl = this.buildSSOLoginUrl(callbackUrl);
        
        console.log('重定向到第三方SSO登录:', ssoUrl);
        console.log('回调URL:', callbackUrl);
        
        if (ssoUrl) {
            window.location.href = ssoUrl;
        } else {
            this.redirectToLogin('SSO配置错误');
        }
    },

    // 构建SSO登录URL
    buildSSOLoginUrl(returnUrl) {
        // 这里需要根据实际的第三方SSO系统配置
        const ssoBaseUrl = window.SSO_CONFIG?.loginUrl || 'https://mapi.zjzwfw.gov.cn/web/mgop/gov-open/zj/2002387292/lastTest/index.html';
        const appId = window.SSO_CONFIG?.appId || 'ocr-service';

        const params = new URLSearchParams({
            appId: appId,
            returnUrl: encodeURIComponent(returnUrl)
        });

        return `${ssoBaseUrl}?${params.toString()}`;
    },

    // 退出登录
    logout() {
        sessionStorage.removeItem('ticketId');
        this.expectedUserId = null;
        this.redirectToLogin();
    },

    // 页面认证检查和初始化
    async initPageAuth() {
        console.log('=== 页面认证检查开始 ===');
        
        // 如果是登录页面，不需要检查认证
        if (window.location.pathname.includes('login.html')) {
            return true;
        }

        // 检查URL中是否有预期的用户ID
        this.getExpectedUserIdFromUrl();

        // 检查认证状态
        const user = await this.checkAuth();
        if (!user) {
            return false;
        }

        console.log('认证检查通过，用户信息:', user);
        
        // 更新页面上的用户信息显示
        this.updateUserDisplay(user);
        
        console.log('=== 页面认证检查完成 ===');
        return user;
    },

    // 更新页面上的用户信息显示
    updateUserDisplay(user) {
        const usernameElement = document.getElementById('username');
        if (usernameElement) {
            usernameElement.textContent = user.userName || user.userId || '用户';
        }

        // 可以在这里添加更多用户信息的显示
        console.log('用户信息已更新到页面显示');
    }
};

// 页面加载时检查认证状态
document.addEventListener('DOMContentLoaded', async () => {
    // 初始化页面认证
    const authResult = await AUTH.initPageAuth();
    
    if (authResult) {
        // 认证成功，绑定退出登录按钮
        const logoutBtn = document.getElementById('logoutBtn');
        if (logoutBtn) {
            logoutBtn.addEventListener('click', () => {
                if (confirm('确定要退出登录吗？')) {
                    AUTH.logout();
                }
            });
        }
    }
}); 

// SSO配置
window.SSO_CONFIG = {
    loginUrl: 'https://mapi.zjzwfw.gov.cn/web/mgop/gov-open/zj/2002387292/lastTest/index.html',
    appId: 'ocr-service'
}; 