/**
 * 生产环境预审管理器
 * 负责处理真实API数据和状态管理
 */
class ProductionPreviewApp {
    constructor() {
        this.previewId = this.getPreviewId();
        this.progress = 0;
        this.timer = null;
        this.statusTimer = null;
        this.init();
    }
    
    /**
     * 从URL获取预审ID
     */
    getPreviewId() {
        // 从URL参数获取
        const urlParams = new URLSearchParams(window.location.search);
        const fromParams = urlParams.get('previewId');
        
        if (fromParams) return fromParams;
        
        // 从路径中提取 /api/preview/view/{previewId}
        const path = window.location.pathname;
        const matches = path.match(/\/api\/preview\/view\/([^\/]+)/);
        return matches ? matches[1] : null;
    }
    
    /**
     * 初始化应用
     */
    async init() {
        if (!this.previewId) {
            this.showError('未找到预审ID');
            return;
        }
        
        try {
            // 检查预审状态
            const status = await this.checkStatus();
            
            switch (status) {
                case 'processing':
                    this.render('waiting');
                    this.startStatusPolling();
                    break;
                case 'completed':
                    const data = await this.loadPreviewData();
                    this.render('result', data);
                    break;
                case 'failed':
                    this.render('error');
                    break;
                default:
                    this.render('waiting');
                    this.startStatusPolling();
            }
        } catch (error) {
            console.error('初始化失败:', error);
            this.showError('加载预审信息失败');
        }
    }
    
    /**
     * 检查预审状态
     */
    async checkStatus() {
        try {
            const response = await fetch(`/api/preview/status/${this.previewId}`, {
                credentials: 'include'
            });
            
            if (!response.ok) {
                throw new Error(`状态检查失败: ${response.status}`);
            }
            
            const result = await response.json();
            return result.data?.status || 'processing';
        } catch (error) {
            console.error('状态检查错误:', error);
            throw error;
        }
    }
    
    /**
     * 加载预审数据
     */
    async loadPreviewData() {
        try {
            const response = await fetch(`/api/preview/data/${this.previewId}`, {
                credentials: 'include'
            });
            
            if (!response.ok) {
                throw new Error(`数据加载失败: ${response.status}`);
            }
            
            const result = await response.json();
            return result.data || result;
        } catch (error) {
            console.error('数据加载错误:', error);
            throw error;
        }
    }
    
    /**
     * 开始状态轮询
     */
    startStatusPolling() {
        this.statusTimer = setInterval(async () => {
            try {
                const status = await this.checkStatus();
                
                if (status === 'completed') {
                    this.stopTimers();
                    this.progress = 100;
                    this.updateProgress();
                    
                    setTimeout(async () => {
                        try {
                            const data = await this.loadPreviewData();
                            this.render('result', data);
                        } catch (error) {
                            this.showError('加载预审结果失败');
                        }
                    }, 800);
                } else if (status === 'failed') {
                    this.stopTimers();
                    this.render('error');
                }
            } catch (error) {
                console.error('状态轮询错误:', error);
                // 继续轮询，不中断
            }
        }, 2000);
    }
    
    /**
     * 渲染不同状态的页面
     */
    render(state, data = null) {
        const app = document.getElementById('app');
        let html = '';
        
        if (state === 'waiting') {
            html = this.renderWaitingState();
            this.startProgress();
        } else if (state === 'error') {
            html = this.renderErrorState();
        } else if (state === 'success') {
            html = this.renderSuccessState();
        } else if (state === 'result' && data) {
            html = this.renderResultState(data);
        }
        
        app.innerHTML = html;
    }
    
    /**
     * 渲染等待状态
     */
    renderWaitingState() {
        return `
            <div class="waiting-card waiting">
                <div class="doc-animation">
                    <div class="document">
                        <div class="doc-lines"></div>
                    </div>
                    <div class="scan-overlay">
                        <div class="scan-corner corner-tl"></div>
                        <div class="scan-corner corner-tr"></div>
                        <div class="scan-corner corner-bl"></div>
                        <div class="scan-corner corner-br"></div>
                    </div>
                </div>
                <div class="progress-section">
                    <div class="progress-title">
                        正在进行智能预审，预计需要等<span id="time">3</span>分钟
                    </div>
                    <div class="progress-bar-container">
                        <div class="progress-bar-fill" id="fill" style="width: ${this.progress}%"></div>
                    </div>
                    <div class="progress-stats">
                        <span>进度：<span id="percent">${Math.round(this.progress)}</span>% 已完成</span>
                        <span id="status-text">正在处理中...</span>
                    </div>
                </div>
            </div>
        `;
    }
    
    /**
     * 渲染错误状态
     */
    renderErrorState() {
        return `
            <div class="error-card error">
                <div class="doc-animation">
                    <div class="document">
                        <div class="error-icon">⚠</div>
                    </div>
                </div>
                <h2 class="state-title error-title">智能预审开小差了</h2>
                <p class="state-description">系统暂时无法完成预审，请稍后重试</p>
                <button class="action-btn" onclick="productionApp.retryPreview()">重试</button>
            </div>
        `;
    }
    
    /**
     * 渲染成功状态
     */
    renderSuccessState() {
        return `
            <div class="success-card success">
                <div class="doc-animation">
                    <div class="document">
                        <div class="doc-lines"></div>
                        <div class="success-icon">✓</div>
                    </div>
                </div>
                <h2 class="state-title success-title">智能预审通过</h2>
                <p class="state-description">
                    恭喜您！您的材料已通过智能预审<br>
                    请返回信息纳入页面继续后续操作
                </p>
            </div>
        `;
    }
    
    /**
     * 渲染结果状态
     */
    renderResultState(data) {
        const basicInfo = data.basicInfo || {};
        const issues = data.issues || [];
        const documents = data.documents || [];
        
        // 判断是否有问题需要处理
        const hasIssues = issues.length > 0;
        
        if (!hasIssues) {
            // 没有问题，显示成功页面
            this.render('success');
            return;
        }
        
        return `
            <div class="result-container">
                ${this.renderWarningBanner()}
                <div style="padding:32px;">
                    ${this.renderBasicInfo(basicInfo)}
                    ${this.renderMaterialsSection(issues)}
                    ${documents.length > 0 ? this.renderDocumentsSection(documents) : ''}
                    ${this.renderActionButtons()}
                </div>
            </div>
        `;
    }
    
    /**
     * 渲染警告横幅
     */
    renderWarningBanner() {
        return `
            <div style="background:linear-gradient(135deg,#fef3c7,#fbbf24);padding:16px 24px;color:#92400e;font-size:14px;border-radius:12px 12px 0 0;border-bottom:1px solid #f59e0b;">
                <div style="display:flex;align-items:center;">
                    <span style="font-size:16px;margin-right:8px;">⚠️</span>
                    <span style="font-weight:500;">温馨提示：</span>
                    <span style="margin-left:4px;">以下内容为系统根据您的申报信息自动出具的智能预审意见，非实际缔结，供您参考。</span>
                </div>
            </div>
        `;
    }
    
    /**
     * 渲染基本信息
     */
    renderBasicInfo(basicInfo) {
        return `
            <div style="margin-bottom:40px;">
                <h3 class="section-header">
                    <span class="section-icon basic-info">📋</span>
                    基本信息
                </h3>
                <div style="background:#f8fafc;border-radius:8px;padding:20px;border:1px solid #e2e8f0;">
                    <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(300px,1fr));gap:16px;">
                        <div style="display:flex;">
                            <span style="color:#6b7280;width:80px;flex-shrink:0;">申请人：</span>
                            <span style="color:#1f2937;font-weight:500;">${basicInfo.applicant || '未提供'}</span>
                        </div>
                        <div style="display:flex;">
                            <span style="color:#6b7280;width:80px;flex-shrink:0;">事项名称：</span>
                            <span style="color:#1f2937;font-weight:500;">${basicInfo.matterName || '未提供'}</span>
                        </div>
                        <div style="display:flex;">
                            <span style="color:#6b7280;width:80px;flex-shrink:0;">事项情形：</span>
                            <span style="color:#1f2937;font-weight:500;">${basicInfo.matterType || '未提供'}</span>
                        </div>
                    </div>
                </div>
            </div>
        `;
    }
    
    /**
     * 渲染材料检查部分
     */
    renderMaterialsSection(issues) {
        return `
            <div style="margin-bottom:32px;">
                <h3 class="section-header">
                    <span class="section-icon materials">📁</span>
                    需检查的材料
                    <span style="background:linear-gradient(135deg,#dc2626,#ef4444);color:white;padding:4px 8px;border-radius:12px;font-size:12px;margin-left:12px;font-weight:600;">${issues.length}</span>
                </h3>
                <div class="materials-list">
                    ${this.renderMaterialsList(issues)}
                </div>
            </div>
        `;
    }
    
    /**
     * 渲染材料列表
     */
    renderMaterialsList(issues) {
        if (!issues.length) {
            return '<div style="text-align:center;color:#6b7280;padding:40px;background:#f8fafc;border-radius:8px;border:1px solid #e2e8f0;">暂无需要检查的材料</div>';
        }
        
        return issues.map((issue, index) => `
            <div class="material-item">
                <div class="material-header" onclick="productionApp.toggleMaterial(${index})">
                    <div class="material-info">
                        <div class="material-name-section">
                            <div class="material-status-dot ${issue.hasIssues ? 'has-issues' : 'completed'}"></div>
                            <div>
                                <span class="material-name">${issue.name || '未知材料'}</span>
                                ${issue.subItems ? `<span class="material-count">${issue.subItems.length}项</span>` : ''}
                            </div>
                        </div>
                        <div class="material-status">
                            <span class="material-status-text ${issue.hasIssues ? 'needs-supplement' : 'completed'}">
                                ${issue.hasIssues ? '需补充' : '已完成'}
                            </span>
                            <i class="expand-icon" id="expand-${index}">▼</i>
                        </div>
                    </div>
                </div>
                <div class="material-details" id="material-${index}">
                    ${issue.subItems ? `
                        <div class="sub-materials">
                            ${issue.subItems.map(item => `
                                <div class="sub-material">
                                    <div class="sub-material-dot"></div>
                                    <span class="sub-material-text">${item}</span>
                                </div>
                            `).join('')}
                        </div>
                    ` : ''}
                </div>
            </div>
        `).join('');
    }
    
    /**
     * 渲染文档部分
     */
    renderDocumentsSection(documents) {
        return `
            <div class="documents-section">
                <h3 class="section-header">
                    <span class="section-icon documents">📊</span>
                    文档清单
                </h3>
                <div class="document-table-container">
                    <table class="document-table">
                        <thead>
                            <tr>
                                <th>序号</th>
                                <th>文件名称</th>
                                <th>文件类型</th>
                                <th>页数</th>
                                <th>录入日期</th>
                                <th>检查日期</th>
                                <th>状态</th>
                                <th>备注</th>
                            </tr>
                        </thead>
                        <tbody>
                            ${documents.map((doc, index) => `
                                <tr>
                                    <td class="doc-index">${index + 1}</td>
                                    <td class="doc-name">${doc.name || '-'}</td>
                                    <td class="doc-type">
                                        <span class="doc-type-badge">${doc.type || '-'}</span>
                                    </td>
                                    <td class="doc-pages">${doc.pages || '-'}</td>
                                    <td class="doc-date">${doc.inputDate || '-'}</td>
                                    <td class="doc-date">${doc.checkDate || '-'}</td>
                                    <td>
                                        <span class="doc-status-badge ${doc.status === '通过' ? 'passed' : 'pending'}">
                                            ${doc.status || '待检查'}
                                        </span>
                                    </td>
                                    <td class="doc-note">${doc.note || '-'}</td>
                                </tr>
                            `).join('')}
                        </tbody>
                    </table>
                </div>
            </div>
        `;
    }
    
    /**
     * 渲染操作按钮
     */
    renderActionButtons() {
        return `
            <div style="text-align:center;margin-top:32px;padding-top:24px;border-top:1px solid #e5e7eb;">
                <button class="action-btn" onclick="productionApp.downloadChecklist()" style="background:linear-gradient(135deg,#10b981,#059669);box-shadow:0 4px 12px rgba(16,185,129,0.3);">
                    📥 下载检查要点清单
                </button>
            </div>
        `;
    }
    
    /**
     * 开始进度动画
     */
    startProgress() {
        this.progress = 0;
        this.timer = setInterval(() => {
            this.progress += Math.random() * 2 + 0.5;
            this.progress = Math.min(this.progress, 95);
            this.updateProgress();
        }, 1000);
    }
    
    /**
     * 更新进度显示
     */
    updateProgress() {
        const fill = document.getElementById('fill');
        const percent = document.getElementById('percent');
        const time = document.getElementById('time');
        const statusText = document.getElementById('status-text');
        
        if (fill) fill.style.width = this.progress + '%';
        if (percent) percent.textContent = Math.round(this.progress);
        if (time) time.textContent = Math.max(1, Math.round((100 - this.progress) / 35));
        
        // 更新状态文本
        if (statusText) {
            if (this.progress < 30) {
                statusText.textContent = '正在分析材料...';
            } else if (this.progress < 60) {
                statusText.textContent = '正在进行智能检查...';
            } else if (this.progress < 90) {
                statusText.textContent = '正在生成预审报告...';
            } else {
                statusText.textContent = '即将完成...';
            }
        }
    }
    
    /**
     * 停止所有定时器
     */
    stopTimers() {
        if (this.timer) {
            clearInterval(this.timer);
            this.timer = null;
        }
        if (this.statusTimer) {
            clearInterval(this.statusTimer);
            this.statusTimer = null;
        }
    }
    
    /**
     * 切换材料详情显示
     */
    toggleMaterial(index) {
        const details = document.getElementById(`material-${index}`);
        const expand = document.getElementById(`expand-${index}`);
        
        if (details && expand) {
            if (details.classList.contains('expanded')) {
                details.classList.remove('expanded');
                expand.classList.remove('expanded');
            } else {
                details.classList.add('expanded');
                expand.classList.add('expanded');
            }
        }
    }
    
    /**
     * 重试预审
     */
    async retryPreview() {
        try {
            const response = await fetch('/api/preview', {
                method: 'POST',
                credentials: 'include',
                headers: {
                    'Content-Type': 'application/json'
                },
                body: JSON.stringify({
                    previewId: this.previewId
                })
            });
            
            if (response.ok) {
                // 重新初始化
                this.init();
            } else {
                this.showError('重试失败，请稍后再试');
            }
        } catch (error) {
            console.error('重试错误:', error);
            this.showError('重试失败，请检查网络连接');
        }
    }
    
    /**
     * 下载检查清单
     */
    async downloadChecklist() {
        try {
            const response = await fetch(`/api/preview/download/${this.previewId}`, {
                credentials: 'include'
            });
            
            if (response.ok) {
                const blob = await response.blob();
                const url = window.URL.createObjectURL(blob);
                const a = document.createElement('a');
                a.href = url;
                a.download = `预审检查清单_${this.previewId}.pdf`;
                document.body.appendChild(a);
                a.click();
                document.body.removeChild(a);
                window.URL.revokeObjectURL(url);
            } else {
                this.showError('下载失败');
            }
        } catch (error) {
            console.error('下载错误:', error);
            this.showError('下载失败，请稍后重试');
        }
    }
    
    /**
     * 显示错误信息
     */
    showError(message) {
        const app = document.getElementById('app');
        app.innerHTML = `
            <div class="error-card error">
                <div class="error-message">${message}</div>
                <div class="doc-animation">
                    <div class="document">
                        <div class="error-icon">!</div>
                    </div>
                </div>
                <h2 class="state-title error-title">出现错误</h2>
                <p class="state-description">${message}</p>
                <button class="action-btn" onclick="location.reload()">刷新页面</button>
            </div>
        `;
    }
}

// 全局实例
let productionApp;

// 初始化
document.addEventListener('DOMContentLoaded', () => {
    productionApp = new ProductionPreviewApp();
});

// 页面卸载时清理
window.addEventListener('beforeunload', () => {
    if (productionApp) {
        productionApp.stopTimers();
    }
});