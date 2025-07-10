// 登录页面脚本
document.addEventListener('DOMContentLoaded', () => {
    // 初始化SSO配置
    initSSO();
    
    // 检查URL参数中是否有ticketId
    checkTicketInUrl();

    // 绑定登录按钮
    const ssoLoginBtn = document.getElementById('ssoLoginBtn');
    if (ssoLoginBtn) {
        ssoLoginBtn.addEventListener('click', redirectToSSO);
    }
});

// 初始化SSO配置
async function initSSO() {
    try {
        const response = await fetch('/api/config');
        if (response.ok) {
            const config = await response.json();
            if (config.success && config.data.sso) {
                window.SSO_CONFIG = config.data.sso;
                console.log('SSO配置已加载:', window.SSO_CONFIG);
            }
        }
    } catch (error) {
        console.error('获取SSO配置失败:', error);
        // 使用默认配置
        window.SSO_CONFIG = {
            enabled: true,
            loginUrl: 'https://ibcdsg.zj.gov.cn:8443/restapi/prod/IC33000020220329000006/uc/sso/login',
            appId: '2002387292',
            callbackUrl: window.location.origin + '/api/sso/callback',
            useCallback: false,  // 默认使用直跳模式，避免本地回调问题
            mockEnabled: false
        };
    }
}

// 检查URL中的ticketId参数
function checkTicketInUrl() {
    const urlParams = new URLSearchParams(window.location.search);
    const ticketId = urlParams.get('ticketId') || urlParams.get('ticket') || urlParams.get('code');
    const error = urlParams.get('error');
    const pendingRequestId = urlParams.get('pendingRequestId');
    
    // 保存待访问的预审记录ID
    if (pendingRequestId) {
        sessionStorage.setItem('pendingRequestId', pendingRequestId);
        console.log('保存待访问预审记录ID:', pendingRequestId);
    }

    // 处理错误情况
    if (error) {
        let errorMessage = '登录失败';
        switch (error) {
            case 'session_error':
                errorMessage = '会话保存失败，请重试';
                break;
            case 'no_ticket':
                errorMessage = '未获取到有效的登录凭证';
                break;
            case 'access_denied':
                errorMessage = '无权限访问该预审记录';
                break;
            case 'invalid_request':
                errorMessage = '无效的请求参数';
                break;
            case 'system_error':
                errorMessage = '系统错误，请稍后重试';
                break;
            case 'user_mismatch':
                errorMessage = '用户身份不匹配，请使用正确的账号登录查看预审材料';
                break;
            default:
                errorMessage = decodeURIComponent(error);
        }
        showError(errorMessage);
        return;
    }

    if (ticketId) {
        console.log('检测到票据ID:', ticketId);

        // 显示加载状态
        showLoginLoading(true);

        // 保存ticketId到会话存储
        sessionStorage.setItem('ticketId', ticketId);

        // 验证ticketId
        verifyTicket(ticketId);
    }
}

// 验证票据
async function verifyTicket(ticketId) {
    try {
        console.log('开始验证票据:', ticketId);
        const response = await fetch('/api/verify_user', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json'
            },
            body: JSON.stringify({ ticketId })
        });

        console.log('票据验证响应状态:', response.status);

        if (response.ok) {
            console.log('票据验证成功，准备跳转到主页');
            // 检查是否有待访问的预审记录
            const pendingRequestId = sessionStorage.getItem('pendingRequestId');
            if (pendingRequestId) {
                sessionStorage.removeItem('pendingRequestId');
                window.location.href = `/api/preview/view/${pendingRequestId}`;
            } else {
                window.location.href = '/static/index.html';
            }
        } else {
            const errorText = await response.text();
            console.error('票据验证失败:', errorText);
            showError('票据验证失败，请重新登录');
            sessionStorage.removeItem('ticketId');
            showLoginLoading(false);
        }
    } catch (error) {
        console.error('验证票据网络错误:', error);
        showError('网络错误，请稍后重试');
        showLoginLoading(false);
    }
}

// 重定向到SSO登录页面
function redirectToSSO() {
    if (!window.SSO_CONFIG) {
        showError('SSO配置未加载，请刷新页面重试');
        return;
    }

    if (!window.SSO_CONFIG.enabled) {
        showError('SSO登录功能未启用');
        return;
    }

    // 构造SSO登录URL
    const ssoUrl = getSSOLoginUrl();
    
    console.log('=== SSO登录跳转 ===');
    console.log('SSO登录地址:', ssoUrl);
    console.log('回调地址:', window.SSO_CONFIG.callbackUrl);
    console.log('应用ID:', window.SSO_CONFIG.appId);

    if (ssoUrl) {
        // 显示加载状态
        showLoginLoading(true);
        
        // 跳转到SSO登录页面
        window.location.href = ssoUrl;
    } else {
        showError('无法构造SSO登录地址');
    }
}

// 获取SSO登录URL
function getSSOLoginUrl() {
    if (!window.SSO_CONFIG || !window.SSO_CONFIG.loginUrl) {
        console.error('SSO配置无效:', window.SSO_CONFIG);
        return null;
    }

    const loginUrl = window.SSO_CONFIG.loginUrl;
    const appId = window.SSO_CONFIG.appId;
    const callbackUrl = window.SSO_CONFIG.callbackUrl;
    const useCallback = window.SSO_CONFIG.useCallback;

    console.log('SSO配置信息:');
    console.log('  - 登录URL:', loginUrl);
    console.log('  - 应用ID:', appId);
    console.log('  - 回调地址:', callbackUrl);
    console.log('  - 使用回调模式:', useCallback);

    // 根据配置决定是否添加回调参数
    let fullUrl;
    if (useCallback && callbackUrl) {
        // 回调模式：添加回调参数
    const params = new URLSearchParams({
        appId: appId,
        redirectUri: callbackUrl,  // 使用标准的redirectUri参数名
        // 也添加一些备用参数名，兼容不同的SSO系统
        returnUrl: callbackUrl,
        callback: callbackUrl
    });
        fullUrl = `${loginUrl}?${params.toString()}`;
        console.log('✅ 回调模式：构造带回调参数的SSO登录URL');
    } else {
        // 直跳模式：只添加应用ID
        const params = new URLSearchParams({
            appId: appId
        });
        fullUrl = `${loginUrl}?${params.toString()}`;
        console.log('✅ 直跳模式：构造不带回调参数的SSO登录URL');
        console.log('💡 用户登录后需要手动返回系统或使用其他方式获取登录状态');
    }

    console.log('最终SSO登录URL:', fullUrl);
    return fullUrl;
}

// 显示/隐藏登录加载状态
function showLoginLoading(show) {
    const loginForm = document.querySelector('.login-form');
    const loginLoading = document.getElementById('loginLoading');

    if (loginForm) {
        loginForm.style.display = show ? 'none' : 'block';
    }

    if (loginLoading) {
        loginLoading.style.display = show ? 'block' : 'none';
    }
}

// 显示错误消息
function showError(message) {
    // 隐藏加载状态
    showLoginLoading(false);
    
    // 创建临时的错误提示
    const errorDiv = document.createElement('div');
    errorDiv.className = 'error-message';
    errorDiv.textContent = message;
    errorDiv.style.cssText = `
        background: #e74c3c;
        color: white;
        padding: 10px 20px;
        border-radius: 5px;
        margin-top: 20px;
        text-align: center;
        animation: fadeIn 0.3s ease-in;
    `;

    const loginBox = document.querySelector('.login-box');
    if (loginBox) {
        // 移除之前的错误提示
        const oldError = loginBox.querySelector('.error-message');
        if (oldError) {
            oldError.remove();
        }

        loginBox.appendChild(errorDiv);

        // 5秒后自动移除
        setTimeout(() => {
            if (errorDiv.parentNode) {
                errorDiv.remove();
            }
        }, 5000);
    }
}

// 初始化默认SSO配置（降级方案）
window.SSO_CONFIG = {
    enabled: true,
    loginUrl: 'https://ibcdsg.zj.gov.cn:8443/restapi/prod/IC33000020220329000006/uc/sso/login',
    appId: '2002387292',
    callbackUrl: window.location.origin + '/api/sso/callback',
    useCallback: false,  // 默认使用直跳模式，避免本地回调问题
    mockEnabled: false
};