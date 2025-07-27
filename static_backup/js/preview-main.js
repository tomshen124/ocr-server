/**
 * OCR智能预审系统前端主文件 - 政务风格版本
 * 实现：用户访问 → SSO认证 → loading页面 → 数据加载 → 政务风格展示
 */
class PreviewApp {
    constructor() {
        this.previewId = null;
        this.config = null;
        this.retryCount = 0;
        this.maxRetries = 3;
        this.progressInterval = null;
        this.currentMaterial = null;
        this.previewData = null;
        this.currentView = 'image'; // image, ocr, result

        this.init();
    }

    async init() {
        try {
            console.log('🚀 政务预审应用初始化开始');

            // 1. 获取URL参数
            this.previewId = this.getUrlParameter('previewId');
            const verified = this.getUrlParameter('verified');

            console.log('📋 URL参数:', { previewId: this.previewId, verified });

            if (!this.previewId) {
                throw new Error('缺少预审ID参数');
            }

            if (verified !== 'true') {
                throw new Error('未通过身份验证，请重新登录');
            }

            // 2. 显示loading（这时用户已经通过SSO认证）
            this.showLoading();

            // 3. 获取配置
            console.log('⚙️ 获取系统配置...');
            this.config = await this.getConfig();

            // 4. 再次检查认证状态（确保会话有效）
            console.log('🔐 验证用户会话...');
            const authStatus = await this.checkAuth();
            if (!authStatus.authenticated) {
                throw new Error('用户会话已过期，请重新登录');
            }

            // 5. 获取预审数据
            console.log('📊 获取预审数据...');
            this.previewData = await this.getPreviewData();

            // 6. 渲染政务风格界面
            console.log('🎨 渲染政务风格界面...');
            this.completeProgress();
            setTimeout(() => {
                this.hideLoading();
                this.renderGovernmentUI(this.previewData);
                this.bindEvents();
            }, 500);

        } catch (error) {
            console.error('❌ 初始化失败:', error);
            this.hideLoading();
            this.showError(error.message);
        }
    }

    // 获取URL参数
    getUrlParameter(name) {
        const urlParams = new URLSearchParams(window.location.search);
        return urlParams.get(name);
    }

    // 显示loading
    showLoading() {
        console.log('⏳ 显示政务风格loading界面');
        document.getElementById('loading-screen').classList.remove('hidden');
        document.getElementById('main-screen').classList.add('hidden');
        document.getElementById('error-screen').classList.add('hidden');

        // 启动进度条动画
        this.startProgressAnimation();
    }

    // 隐藏loading
    hideLoading() {
        console.log('✅ 隐藏loading界面，显示主界面');
        document.getElementById('loading-screen').classList.add('hidden');
        document.getElementById('main-screen').classList.remove('hidden');
    }

    // 显示错误
    showError(message) {
        console.log('❌ 显示错误界面:', message);
        document.getElementById('loading-screen').classList.add('hidden');
        document.getElementById('main-screen').classList.add('hidden');
        document.getElementById('error-screen').classList.remove('hidden');
        document.getElementById('error-message').textContent = message;
    }

    // 进度条动画
    startProgressAnimation() {
        const progressBar = document.getElementById('progress-bar');
        const progressText = document.getElementById('progress-text');
        let progress = 0;

        this.progressInterval = setInterval(() => {
            progress += Math.random() * 10;
            if (progress > 85) progress = 85; // 不要到100%，等数据加载完成

            progressBar.style.width = progress + '%';
            progressText.textContent = Math.round(progress) + '%';
        }, 200);
    }

    // 完成进度条
    completeProgress() {
        if (this.progressInterval) {
            clearInterval(this.progressInterval);
        }

        const progressBar = document.getElementById('progress-bar');
        const progressText = document.getElementById('progress-text');
        progressBar.style.width = '100%';
        progressText.textContent = '100%';
    }

    // 获取配置
    async getConfig() {
        try {
            const response = await fetch('/api/config/frontend', {
                credentials: 'include'
            });
            
            if (response.ok) {
                const result = await response.json();
                console.log('✅ 配置获取成功:', result.data);
                return result.data;
            }
            console.warn('⚠️ 配置获取失败，使用默认配置');
            return {};
        } catch (error) {
            console.warn('⚠️ 配置获取异常:', error);
            return {};
        }
    }

    // 检查认证状态
    async checkAuth() {
        try {
            const response = await fetch('/api/auth/status', {
                credentials: 'include'
            });
            
            if (response.ok) {
                const result = await response.json();
                console.log('✅ 认证状态检查成功:', result);
                return result;
            }
            console.warn('⚠️ 认证状态检查失败');
            return { authenticated: false };
        } catch (error) {
            console.error('❌ 认证状态检查异常:', error);
            return { authenticated: false };
        }
    }

    // 获取预审数据
    async getPreviewData() {
        try {
            console.log(`📡 请求预审数据: /api/preview/result/${this.previewId}`);
            
            const response = await fetch(`/api/preview/result/${this.previewId}`, {
                credentials: 'include',
                headers: {
                    'Content-Type': 'application/json'
                }
            });

            if (!response.ok) {
                if (response.status === 401) {
                    throw new Error('认证已过期，请重新登录');
                } else if (response.status === 403) {
                    throw new Error('无权限访问该预审记录');
                } else if (response.status === 404) {
                    throw new Error('预审记录不存在');
                } else {
                    throw new Error(`获取预审数据失败: HTTP ${response.status}`);
                }
            }

            const result = await response.json();
            console.log('📊 预审数据响应:', result);
            
            if (!result.success) {
                throw new Error(result.errorMsg || '获取预审数据失败');
            }

            return result.data;
        } catch (error) {
            // 重试机制
            if (this.retryCount < this.maxRetries) {
                this.retryCount++;
                console.warn(`⚠️ 获取数据失败，第${this.retryCount}次重试...`);
                await this.sleep(1000 * this.retryCount); // 递增延迟
                return this.getPreviewData();
            }
            
            throw error;
        }
    }

    // 渲染政务风格UI
    renderGovernmentUI(data) {
        console.log('🎨 开始渲染政务风格界面:', data);

        // 渲染申请信息
        this.renderApplicantInfo(data);

        // 渲染材料列表
        this.renderMaterialsList(data.materials || []);

        // 渲染审核摘要
        this.renderSummary(data.summary || {});

        console.log('✅ 政务风格界面渲染完成');
    }

    // 渲染申请信息
    renderApplicantInfo(data) {
        document.getElementById('applicant-name').textContent = data.applicant_name || '申请人';
        document.getElementById('matter-name').textContent = data.matter_name || '事项名称';
        document.getElementById('apply-time').textContent = this.formatDate(data.created_at) || '-';
        document.getElementById('preview-status').textContent = this.getStatusText(data.status) || '处理中';
    }

    // 渲染材料列表
    renderMaterialsList(materials) {
        const container = document.getElementById('materials-list');
        container.innerHTML = '';

        if (!materials || materials.length === 0) {
            container.innerHTML = '<li style="text-align: center; color: #999; padding: 20px;">暂无材料信息</li>';
            return;
        }

        materials.forEach((material, index) => {
            const li = document.createElement('li');
            li.className = 'material-item-wrapper';

            const statusClass = this.getStatusClass(material.status);
            const statusIcon = this.getStatusIcon(material.status);

            li.innerHTML = `
                <div class="material-item ${statusClass}" data-material-index="${index}">
                    <div class="material-content">
                        <span class="material-name">${material.name || '未知材料'}</span>
                        <span class="material-status">${material.status_text || '未知状态'}</span>
                    </div>
                </div>
            `;

            // 添加点击事件
            li.querySelector('.material-item').addEventListener('click', () => {
                this.selectMaterial(index);
            });

            container.appendChild(li);
        });

        // 默认选择第一个材料
        if (materials.length > 0) {
            this.selectMaterial(0);
        }
    }

    // 渲染审核摘要
    renderSummary(summary) {
        const totalCount = summary.total_materials || 0;
        const passedCount = summary.passed_materials || 0;
        const failedCount = summary.failed_materials || 0;
        const warningCount = summary.warning_materials || 0;

        document.getElementById('total-count').textContent = totalCount;
        document.getElementById('passed-count').textContent = passedCount;
        document.getElementById('failed-count').textContent = failedCount;
        document.getElementById('warning-count').textContent = warningCount;

        // 更新失败计数徽章
        const failedBadge = document.getElementById('failed-count');
        failedBadge.textContent = failedCount;
        failedBadge.style.display = failedCount > 0 ? 'inline-flex' : 'none';

        // 渲染建议
        const suggestionsList = document.getElementById('suggestions-list');
        suggestionsList.innerHTML = '';

        if (summary.suggestions && summary.suggestions.length > 0) {
            summary.suggestions.forEach(suggestion => {
                const li = document.createElement('li');
                li.className = 'suggestion-item';
                li.textContent = suggestion;
                suggestionsList.appendChild(li);
            });
        } else {
            const li = document.createElement('li');
            li.className = 'suggestion-item';
            li.textContent = '暂无特殊建议';
            li.style.color = '#999';
            suggestionsList.appendChild(li);
        }

        // 设置摘要卡片样式
        const summaryCard = document.getElementById('summary-card');
        const overallResult = summary.overall_result || 'Pending';

        if (overallResult === 'Approved') {
            summaryCard.style.borderColor = '#b7eb8f';
            summaryCard.style.backgroundColor = '#f6ffed';
            document.getElementById('summary-icon').textContent = '✅';
            document.getElementById('summary-text').textContent = '审核通过';
        } else if (overallResult === 'RequiresCorrection' || overallResult === 'RequiresAdditionalMaterials') {
            summaryCard.style.borderColor = '#ffccc7';
            summaryCard.style.backgroundColor = '#fff2f0';
            document.getElementById('summary-icon').textContent = '⚠️';
            document.getElementById('summary-text').textContent = '需要修正';
        } else {
            summaryCard.style.borderColor = '#adc6ff';
            summaryCard.style.backgroundColor = '#f0f5ff';
            document.getElementById('summary-icon').textContent = '⏳';
            document.getElementById('summary-text').textContent = '处理中';
        }
    }

    // 选择材料
    selectMaterial(index) {
        // 移除之前的选中状态
        document.querySelectorAll('.material-item').forEach(item => {
            item.classList.remove('active');
        });

        // 设置当前选中状态
        const materialItems = document.querySelectorAll('.material-item');
        if (materialItems[index]) {
            materialItems[index].classList.add('active');
            this.currentMaterial = this.previewData.materials[index];
            this.updateContentView();
        }
    }

    // 更新内容视图
    updateContentView() {
        if (!this.currentMaterial) return;

        const container = document.getElementById('image-container');

        if (this.currentView === 'image') {
            this.showMaterialImage();
        } else if (this.currentView === 'ocr') {
            this.showOCRContent();
        } else if (this.currentView === 'result') {
            this.showEvaluationResult();
        }
    }

    // 显示材料图片
    showMaterialImage() {
        const container = document.getElementById('image-container');
        const imageUrl = this.currentMaterial.image_url || this.getStatusIcon(this.currentMaterial.status);

        container.innerHTML = `
            <img src="${imageUrl}" alt="${this.currentMaterial.name}" class="material-image">
        `;
    }

    // 显示OCR内容
    showOCRContent() {
        const container = document.getElementById('image-container');
        const ocrContent = this.currentMaterial.ocr_content || '暂无识别内容';

        container.innerHTML = `
            <div style="padding: 20px; background: white; border-radius: 8px; margin: 20px; line-height: 1.6;">
                <h3 style="margin-bottom: 16px; color: #333;">OCR识别内容</h3>
                <div style="background: #f8f9fa; padding: 16px; border-radius: 4px; border: 1px solid #e9ecef;">
                    <pre style="white-space: pre-wrap; font-family: inherit; margin: 0;">${ocrContent}</pre>
                </div>
            </div>
        `;
    }

    // 显示评估结果
    showEvaluationResult() {
        const container = document.getElementById('image-container');
        const evaluation = this.currentMaterial.evaluation_result || {};

        let html = `
            <div style="padding: 20px; background: white; border-radius: 8px; margin: 20px;">
                <h3 style="margin-bottom: 16px; color: #333;">审核结果</h3>
                <div style="background: #f8f9fa; padding: 16px; border-radius: 4px; border: 1px solid #e9ecef;">
        `;

        if (evaluation.score !== undefined) {
            html += `<div style="margin-bottom: 12px;"><strong>评分：</strong> ${evaluation.score}/100</div>`;
        }

        if (evaluation.issues && evaluation.issues.length > 0) {
            html += `<div style="margin-bottom: 12px;"><strong>发现问题：</strong><ul style="margin: 8px 0; padding-left: 20px;">`;
            evaluation.issues.forEach(issue => {
                html += `<li style="color: #ff4d4f; margin-bottom: 4px;">${issue}</li>`;
            });
            html += `</ul></div>`;
        }

        if (evaluation.suggestions && evaluation.suggestions.length > 0) {
            html += `<div style="margin-bottom: 12px;"><strong>改进建议：</strong><ul style="margin: 8px 0; padding-left: 20px;">`;
            evaluation.suggestions.forEach(suggestion => {
                html += `<li style="color: #faad14; margin-bottom: 4px;">${suggestion}</li>`;
            });
            html += `</ul></div>`;
        }

        if (evaluation.details) {
            html += `<div><strong>详细说明：</strong><p style="margin: 8px 0; line-height: 1.6;">${evaluation.details}</p></div>`;
        }

        html += `</div></div>`;
        container.innerHTML = html;
    }

    // 绑定事件
    bindEvents() {
        // 视图切换按钮
        document.getElementById('view-image-btn').addEventListener('click', () => {
            this.switchView('image');
        });

        document.getElementById('view-ocr-btn').addEventListener('click', () => {
            this.switchView('ocr');
        });

        document.getElementById('view-result-btn').addEventListener('click', () => {
            this.switchView('result');
        });
    }

    // 切换视图
    switchView(view) {
        this.currentView = view;

        // 更新按钮状态
        document.querySelectorAll('.content-btn').forEach(btn => {
            btn.classList.remove('primary');
        });

        if (view === 'image') {
            document.getElementById('view-image-btn').classList.add('primary');
        } else if (view === 'ocr') {
            document.getElementById('view-ocr-btn').classList.add('primary');
        } else if (view === 'result') {
            document.getElementById('view-result-btn').classList.add('primary');
        }

        // 更新内容视图
        this.updateContentView();
    }

    // 工具方法：获取状态样式类
    getStatusClass(status) {
        const statusMap = {
            'passed': 'status-passed',
            'failed': 'status-failed',
            'warning': 'status-warning',
            'pending': 'status-pending'
        };
        return statusMap[status] || 'status-pending';
    }

    // 工具方法：获取状态图标
    getStatusIcon(status) {
        const iconMap = {
            'passed': '/images/智能预审_已通过材料1.3.png',
            'failed': '/images/智能预审_有审查点1.3.png',
            'warning': '/images/智能预审_无审核依据材料1.3.png',
            'pending': '/images/智能预审loading1.3.png'
        };
        return iconMap[status] || iconMap['pending'];
    }

    // 工具方法：获取状态文本
    getStatusText(status) {
        const textMap = {
            'completed': '已完成',
            'processing': '处理中',
            'pending': '待处理',
            'failed': '处理失败'
        };
        return textMap[status] || '未知状态';
    }

    // 工具方法：格式化日期
    formatDate(dateString) {
        if (!dateString) return '';

        try {
            const date = new Date(dateString);
            return date.toLocaleString('zh-CN', {
                year: 'numeric',
                month: '2-digit',
                day: '2-digit',
                hour: '2-digit',
                minute: '2-digit'
            });
        } catch (error) {
            return dateString;
        }
    }

    // 工具函数：延迟
    sleep(ms) {
        return new Promise(resolve => setTimeout(resolve, ms));
    }
}

// 页面加载完成后初始化
document.addEventListener('DOMContentLoaded', () => {
    console.log('📄 页面加载完成，初始化预审应用');
    new PreviewApp();
});

// 全局错误处理
window.addEventListener('error', (event) => {
    console.error('🚨 全局错误:', event.error);
});

window.addEventListener('unhandledrejection', (event) => {
    console.error('🚨 未处理的Promise拒绝:', event.reason);
});
