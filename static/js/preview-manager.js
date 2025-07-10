/**
 * 预审状态管理器
 * 处理预审流程的各种状态：waiting, processing, completed, failed
 */
class PreviewManager {
    constructor(containerId) {
        this.container = document.getElementById(containerId);
        this.previewId = null;
        this.previewData = null;
        this.currentState = 'waiting';
        this.progressInterval = null;
        this.statusInterval = null;
        this.progress = 0;
        
        this.init();
    }

    // 初始化
    init() {
        this.previewId = this.getPreviewId();
        if (this.previewId) {
            this.startPreviewProcess();
        } else {
            this.setState('error', { message: '未找到预审ID' });
        }
    }

    // 获取预审ID
    getPreviewId() {
        // 从URL参数或路径中获取
        const urlParams = new URLSearchParams(window.location.search);
        const fromParams = urlParams.get('previewId');
        
        if (fromParams) return fromParams;
        
        // 从路径中提取 /api/preview/view/{previewId}
        const path = window.location.pathname;
        const matches = path.match(/\/api\/preview\/view\/([^\/]+)/);
        return matches ? matches[1] : null;
    }

    // 开始预审流程
    startPreviewProcess() {
        this.setState('waiting');
        this.startProgressSimulation();
        this.startStatusPolling();
    }

    // 设置状态
    setState(state, data = {}) {
        this.currentState = state;
        this.render(state, data);
    }

    // 渲染不同状态的页面
    render(state, data = {}) {
        let content = '';
        
        switch (state) {
            case 'waiting':
                content = this.renderWaitingPage();
                break;
            case 'error':
                content = this.renderErrorPage(data.message);
                break;
            case 'result':
                content = this.renderResultPage(data);
                break;
            case 'success':
                content = this.renderSuccessPage();
                break;
            default:
                content = this.renderErrorPage('未知状态');
        }
        
        this.container.innerHTML = content;
        this.bindEvents();
    }

    // 渲染等待页面
    renderWaitingPage() {
        return `
            <div class="preview-waiting">
                <div class="scanning-animation">
                    <div class="document-icon">
                        <div class="document-lines"></div>
                    </div>
                    <div class="scanner-frame">
                        <div class="corner corner-tl"></div>
                        <div class="corner corner-tr"></div>
                        <div class="corner corner-bl"></div>
                        <div class="corner corner-br"></div>
                    </div>
                </div>
                
                <div class="progress-section">
                    <div class="progress-text">
                        正在进行智能预审，预计需要等<span id="estimated-time">3</span>分钟
                    </div>
                    <div class="progress-bar">
                        <div class="progress-fill" id="progress-fill" style="width: ${this.progress}%">
                            <div class="progress-shine"></div>
                        </div>
                    </div>
                    <div class="progress-info">
                        <span id="progress-percent">${Math.round(this.progress)}</span>% 已完成
                    </div>
                </div>
            </div>
        `;
    }

    // 渲染错误页面
    renderErrorPage(message = '智能预审开小差了') {
        return `
            <div class="preview-error">
                <div class="error-animation">
                    <div class="error-document">
                        <div class="error-icon">?</div>
                    </div>
                    <div class="sparkles">
                        <div class="sparkle"></div>
                        <div class="sparkle"></div>
                        <div class="sparkle"></div>
                        <div class="sparkle"></div>
                    </div>
                </div>
                
                <div class="error-content">
                    <h2>${message}</h2>
                    <p>点击"重试"可重新发起智能预审</p>
                    <button class="retry-btn" onclick="previewManager.retryPreview()">重试</button>
                </div>
            </div>
        `;
    }

    // 渲染结果页面
    renderResultPage(data) {
        const basicInfo = data.basicInfo || {};
        const materials = data.materials || [];
        const issues = data.issues || [];
        
        return `
            <div class="preview-result">
                <div class="warning-banner">
                    <i class="warning-icon">⚠️</i>
                    温馨提示：以下内容为系统根据您的申报信息自动出具的智能预审意见，非实际缔结，供您参考。
                </div>
                
                <div class="result-content">
                    <section class="basic-info-section">
                        <h3 class="section-title">
                            <i class="section-icon">📋</i>
                            基本信息
                        </h3>
                        <div class="info-grid">
                            <div class="info-item">
                                <span class="label">申请人：</span>
                                <span class="value">${basicInfo.applicant || '浙江XX三四科技有限责任公司'}</span>
                            </div>
                            <div class="info-item">
                                <span class="label">事项名称：</span>
                                <span class="value">${basicInfo.matterName || '内资公司变更'}</span>
                            </div>
                            <div class="info-item">
                                <span class="label">事项情形：</span>
                                <span class="value">${basicInfo.matterType || '经营范围'}</span>
                            </div>
                        </div>
                    </section>

                    <section class="materials-section">
                        <h3 class="section-title">
                            <i class="section-icon">📁</i>
                            需检查的材料
                            <span class="materials-count">${issues.length}</span>
                        </h3>
                        
                        <div class="tab-controls">
                            <button class="tab-btn active" data-tab="current">当前材料</button>
                            <button class="tab-btn" data-tab="missing">需补充材料</button>
                        </div>
                        
                        <div class="materials-list">
                            ${this.renderMaterialsList(issues)}
                        </div>
                        
                        ${this.renderDocumentTable(data.documents || [])}
                    </section>

                    <div class="actions">
                        <button class="download-btn" onclick="previewManager.downloadChecklist()">
                            <i class="download-icon">📥</i>
                            下载检查要点清单
                        </button>
                    </div>
                </div>
            </div>
        `;
    }

    // 渲染材料列表
    renderMaterialsList(issues) {
        if (!issues.length) {
            return '<div class="no-materials">暂无需要检查的材料</div>';
        }
        
        return issues.map((material, index) => `
            <div class="material-item" data-index="${index}">
                <div class="material-header" onclick="previewManager.toggleMaterial(${index})">
                    <div class="material-info">
                        <span class="material-name">${material.name}</span>
                        <span class="material-count">(${material.subItems ? material.subItems.length : 0})</span>
                    </div>
                    <div class="material-status">
                        <div class="status-indicator ${material.hasIssues ? 'error' : 'success'}"></div>
                        <span class="status-text">${material.hasIssues ? '请上传材料' : '已上传'}</span>
                        <i class="expand-icon">▼</i>
                    </div>
                </div>
                <div class="material-details" style="display: none;">
                    <ul class="sub-materials">
                        ${(material.subItems || []).map(item => `
                            <li class="sub-material">
                                <div class="status-indicator error"></div>
                                <span>${item}</span>
                            </li>
                        `).join('')}
                    </ul>
                </div>
            </div>
        `).join('');
    }

    // 渲染文档表格
    renderDocumentTable(documents) {
        if (!documents.length) {
            documents = this.generateSampleDocuments();
        }
        
        return `
            <div class="document-table-container">
                <table class="document-table">
                    <thead>
                        <tr>
                            <th>序号</th>
                            <th>文件</th>
                            <th>类型</th>
                            <th>页码</th>
                            <th>录入日期</th>
                            <th>检查日期</th>
                            <th>检查</th>
                            <th>备注</th>
                        </tr>
                    </thead>
                    <tbody>
                        ${documents.map((doc, index) => `
                            <tr>
                                <td>${index + 1}</td>
                                <td>${doc.name}</td>
                                <td>${doc.type}</td>
                                <td>${doc.pages}</td>
                                <td>${doc.inputDate}</td>
                                <td>${doc.checkDate}</td>
                                <td>${doc.status}</td>
                                <td class="error-note">${doc.note}</td>
                            </tr>
                        `).join('')}
                    </tbody>
                </table>
            </div>
        `;
    }

    // 渲染成功页面
    renderSuccessPage() {
        return `
            <div class="preview-success">
                <div class="success-animation">
                    <div class="success-document">
                        <div class="document-lines"></div>
                        <div class="check-mark">✓</div>
                    </div>
                    <div class="sparkles">
                        <div class="sparkle"></div>
                        <div class="sparkle"></div>
                        <div class="sparkle"></div>
                        <div class="sparkle"></div>
                    </div>
                </div>
                
                <div class="success-content">
                    <h2>智能预审通过</h2>
                    <p>请返回信息纳入页面继续操作</p>
                </div>
            </div>
        `;
    }

    // 开始进度模拟
    startProgressSimulation() {
        this.progress = 0;
        this.progressInterval = setInterval(() => {
            // 非线性进度增长
            if (this.progress < 30) {
                this.progress += Math.random() * 3 + 1;
            } else if (this.progress < 60) {
                this.progress += Math.random() * 2 + 0.5;
            } else if (this.progress < 85) {
                this.progress += Math.random() * 1 + 0.2;
            } else if (this.progress < 95) {
                this.progress += Math.random() * 0.5 + 0.1;
            }
            
            this.progress = Math.min(this.progress, 95);
            this.updateProgress();
        }, 800 + Math.random() * 400);
    }

    // 更新进度显示
    updateProgress() {
        const progressFill = document.getElementById('progress-fill');
        const progressPercent = document.getElementById('progress-percent');
        const estimatedTime = document.getElementById('estimated-time');
        
        if (progressFill) {
            progressFill.style.width = this.progress + '%';
        }
        if (progressPercent) {
            progressPercent.textContent = Math.round(this.progress);
        }
        if (estimatedTime) {
            const remainingTime = Math.max(1, Math.round((100 - this.progress) / 30));
            estimatedTime.textContent = remainingTime;
        }
    }

    // 开始状态轮询
    startStatusPolling() {
        if (!this.previewId) return;
        
        this.statusInterval = setInterval(async () => {
            try {
                const response = await fetch(`/api/preview/status/${this.previewId}`, {
                    credentials: 'include'
                });
                
                if (response.ok) {
                    const result = await response.json();
                    this.handleStatusUpdate(result);
                } else {
                    console.error('状态检查失败:', response.status);
                }
            } catch (error) {
                console.error('状态检查错误:', error);
            }
        }, 2000);
    }

    // 处理状态更新
    handleStatusUpdate(result) {
        if (!result.data) return;
        
        const status = result.data.status;
        
        switch (status) {
            case 'processing':
                // 继续等待
                break;
            case 'completed':
                this.stopIntervals();
                this.completeProgress();
                setTimeout(() => {
                    this.loadPreviewResult();
                }, 1000);
                break;
            case 'failed':
                this.stopIntervals();
                this.setState('error', { message: '预审处理失败，请稍后重试' });
                break;
        }
    }

    // 完成进度
    completeProgress() {
        this.progress = 100;
        this.updateProgress();
    }

    // 停止所有定时器
    stopIntervals() {
        if (this.progressInterval) {
            clearInterval(this.progressInterval);
            this.progressInterval = null;
        }
        if (this.statusInterval) {
            clearInterval(this.statusInterval);
            this.statusInterval = null;
        }
    }

    // 加载预审结果
    async loadPreviewResult() {
        try {
            const response = await fetch(`/api/preview/data/${this.previewId}`, {
                credentials: 'include'
            });
            
            if (response.ok) {
                const result = await response.json();
                this.previewData = result.data || result;
                
                // 判断是否有问题需要处理
                const hasIssues = this.previewData.issues && this.previewData.issues.length > 0;
                
                if (hasIssues) {
                    this.setState('result', this.previewData);
                } else {
                    this.setState('success');
                }
            } else {
                throw new Error('加载预审结果失败');
            }
        } catch (error) {
            console.error('加载预审结果错误:', error);
            this.setState('error', { message: '加载预审结果失败' });
        }
    }

    // 绑定事件
    bindEvents() {
        // 标签切换
        const tabBtns = this.container.querySelectorAll('.tab-btn');
        tabBtns.forEach(btn => {
            btn.addEventListener('click', (e) => {
                tabBtns.forEach(b => b.classList.remove('active'));
                e.target.classList.add('active');
                this.filterMaterials(e.target.dataset.tab);
            });
        });
    }

    // 切换材料详情
    toggleMaterial(index) {
        const materialItem = this.container.querySelector(`[data-index="${index}"]`);
        if (materialItem) {
            const details = materialItem.querySelector('.material-details');
            const icon = materialItem.querySelector('.expand-icon');
            
            if (details.style.display === 'none') {
                details.style.display = 'block';
                icon.style.transform = 'rotate(180deg)';
            } else {
                details.style.display = 'none';
                icon.style.transform = 'rotate(0deg)';
            }
        }
    }

    // 过滤材料
    filterMaterials(tab) {
        // 这里可以根据tab类型过滤材料显示
        console.log('过滤材料:', tab);
    }

    // 重试预审
    retryPreview() {
        this.startPreviewProcess();
    }

    // 下载检查清单
    downloadChecklist() {
        if (this.previewId) {
            window.open(`/api/preview/download/${this.previewId}`, '_blank');
        }
    }

    // 生成示例文档数据
    generateSampleDocuments() {
        return [
            { name: '法人证件', type: '身份证', pages: '15.37', inputDate: '2015年3月', checkDate: '2021年10月', status: '检查', note: '已录入' },
            { name: '法人证件', type: '身份证', pages: '15.37', inputDate: '2015年3月', checkDate: '2021年9月', status: '检查', note: '已录入' },
            { name: '法人证件', type: '身份证', pages: '15.37', inputDate: '2015年3月', checkDate: '2021年9月', status: '检查', note: '已录入' },
            { name: '法人证件', type: '身份证', pages: '15.37', inputDate: '2015年3月', checkDate: '2020年11月', status: '检查', note: '已录入' },
            { name: '法人证件', type: '身份证', pages: '15.37', inputDate: '2015年3月', checkDate: '2021年8月', status: '检查', note: '已录入' }
        ];
    }

    // 销毁管理器
    destroy() {
        this.stopIntervals();
    }
}

// 全局变量
let previewManager = null;

// 页面加载完成后初始化
document.addEventListener('DOMContentLoaded', function() {
    previewManager = new PreviewManager('preview-container');
});

// 页面卸载时清理
window.addEventListener('beforeunload', function() {
    if (previewManager) {
        previewManager.destroy();
    }
}); 