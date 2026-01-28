// 智能预审系统主应用
(() => {
class IntelligentAuditApp {
    constructor() {
        this.apiService = window.apiService;
        this.componentManager = new ComponentManager();
        this.currentStatus = 'loading'; // passed, loading, error, hasIssues
        this.previewId = this.getPreviewIdFromUrl(); // 从URL获取预审ID
        this.auditId = null;
        this.dataLoaded = false; // 数据是否加载成功
        this.dataPending = false; // 数据是否等待落库/生成
        this.pendingTimer = null;
        this.loadingInterval = null;
        this.loadingProgress = 0;
        this.loadingActive = false;
        this.files = {};
        this.reportDownloadUrl = null;
        this.reportFallbackUrl = null;
        this.reportPdfUrl = null;
        this.reportHtmlUrl = null;
        this.reportPdfSource = null;
        this.reportHtmlSource = null;
        this.monitorSessionId = this.resolveMonitorSessionId();
        this.init();
    }

    // 从URL获取预审ID
    getPreviewIdFromUrl() {
        const urlParams = new URLSearchParams(window.location.search);
        return urlParams.get('previewId') || urlParams.get('requestId') || urlParams.get('preview_id') || urlParams.get('request_id');
    }

    // 读取监控系统会话ID（用于绕过SSO）
    resolveMonitorSessionId() {
        let sessionId = null;
        try {
            const urlParams = new URLSearchParams(window.location.search);
            const queryValue = urlParams.get('monitor_session_id');
            if (queryValue) {
                sessionId = queryValue;
                window.localStorage?.setItem('monitor_session_id', queryValue);
            }
        } catch (error) {
            console.warn('解析monitor_session_id失败:', error);
        }

        if (!sessionId) {
            try {
                sessionId = window.localStorage?.getItem('monitor_session_id') || null;
            } catch (error) {
                console.warn('读取monitor_session_id失败:', error);
            }
        }

        return sessionId;
    }

    // 初始化应用
    async init() {
        this.initComponents();
        this.bindEvents();

        // 先显示loading状态
        this.currentStatus = 'loading';
        this.showLoading();
        this.updateDownloadButtonState();

        // 启动系统状态监控
        this.startSystemStatusMonitoring();

        // 异步加载数据
        this.loadDataAsync();
    }

    // 初始化组件
    initComponents() {
        // 注册基本信息组件
        const basicInfoContainer = document.querySelector('.basic-info-card');
        this.componentManager.register('basicInfo', new BasicInfoComponent(basicInfoContainer));

        // 注册材料列表组件
        const materialsListContainer = document.getElementById('materials-list');
        const materialsListComponent = new MaterialsListComponent(materialsListContainer);
        materialsListComponent.setEventHandlers(
            this.handleMaterialToggle.bind(this),
            this.handleDocumentClick.bind(this)
        );
        this.componentManager.register('materialsList', materialsListComponent);

        // 注册状态显示组件
        const statusDisplayContainer = document.getElementById('right-panel');
        this.componentManager.register('statusDisplay', new StatusDisplayComponent(statusDisplayContainer));

        // 注册文档预览组件
        const documentPreviewContainer = document.getElementById('right-panel');
        this.componentManager.register('documentPreview', new DocumentPreviewComponent(documentPreviewContainer));

        // 注册已通过材料组件
        const passedMaterialsContainer = document.querySelector('.passed-materials-card');
        this.componentManager.register('passedMaterials', new PassedMaterialsComponent(passedMaterialsContainer));
    }

    // 绑定事件
    bindEvents() {
        // 关闭文档预览
        document.getElementById('close-document').addEventListener('click', () => {
            this.hideDocumentPreview();
        });

        // 点击遮罩关闭预览
        document.getElementById('document-modal').addEventListener('click', (e) => {
            if (e.target.id === 'document-modal') {
                this.hideDocumentPreview();
            }
        });

        // 重试按钮
        document.getElementById('retry-btn').addEventListener('click', () => {
            this.hideErrorModal();
            this.startAudit();
        });

        // 导出材料按钮
        document.getElementById('export-materials').addEventListener('click', () => {
            this.exportMaterials();
        });

        // 审核依据材料按钮
        document.getElementById('view-basis').addEventListener('click', () => {
            this.viewAuditBasis();
        });

        // 下载报告按钮
        const downloadPdfBtn = document.getElementById('download-report-pdf');
        const downloadHtmlBtn = document.getElementById('download-report-html');

        downloadPdfBtn?.addEventListener('click', () => {
            this.downloadReport('pdf');
        });

        downloadHtmlBtn?.addEventListener('click', () => {
            this.downloadReport('html');
        });

        // 🖼️ 文档预览模态框关闭事件
        this.bindDocumentPreviewEvents();

        // 添加测试按钮（开发用）
        // this.addTestButtons(); // 注释掉调试按钮
    }

    // 🖼️ 绑定文档预览相关事件
    bindDocumentPreviewEvents() {
        // 动态创建文档预览模态框（如果不存在）
        if (!document.getElementById('document-preview-modal')) {
            const modalHTML = `
                <div id="document-preview-modal" class="document-preview-modal" style="display: none;">
                    <div class="modal-overlay">
                        <div class="modal-content">
                            <div class="modal-header">
                                <h3 id="document-title">文档预览</h3>
                                <button class="close-btn" id="close-document-preview">×</button>
                            </div>
                            <div class="modal-body" id="document-content">
                                <img id="document-image" style="max-width: 100%; height: auto;" alt="文档预览">
                            </div>
                        </div>
                    </div>
                </div>
            `;
            document.body.insertAdjacentHTML('beforeend', modalHTML);
        }

        // 绑定关闭事件
        const modal = document.getElementById('document-preview-modal');
        const closeBtn = document.getElementById('close-document-preview');
        
        closeBtn?.addEventListener('click', () => {
            this.hideDocumentPreview();
        });

        modal?.addEventListener('click', (e) => {
            if (e.target === modal || e.target.className === 'modal-overlay') {
                this.hideDocumentPreview();
            }
        });
    }

    // 🖼️ 显示文档预览弹窗（缩略图使用）
    showPreviewModal(imageUrl, title = '文档预览') {
        const modal = document.getElementById('document-preview-modal');
        const titleElement = document.getElementById('document-title');
        const imageElement = document.getElementById('document-image');
        
        if (modal && titleElement && imageElement) {
            titleElement.textContent = title;
            imageElement.src = imageUrl;
            imageElement.alt = title;
            modal.style.display = 'block';
            
            console.log('显示文档预览:', title, imageUrl);
        }
    }

    // 🖼️ 隐藏文档预览
    hideDocumentPreview() {
        const modal = document.getElementById('document-preview-modal');
        if (modal) {
            modal.style.display = 'none';
        }
    }

    // 添加测试按钮（开发用）
    addTestButtons() {
        // 判断是否为开发环境，只在开发环境中显示测试按钮
        const isDevelopment = window.location.hostname === 'localhost' || 
                             window.location.hostname === '127.0.0.1' ||
                             window.location.hostname.includes('myide.io');
        
        if (isDevelopment) {
            const testPanel = document.createElement('div');
            testPanel.style.cssText = `
                position: fixed;
                top: 10px;
                right: 10px;
                background: white;
                padding: 10px;
                border-radius: 6px;
                box-shadow: 0 2px 8px rgba(0,0,0,0.1);
                z-index: 1000;
                font-size: 12px;
            `;
            testPanel.innerHTML = `
                <div style="margin-bottom: 8px; font-weight: bold;">测试状态：</div>
                <button onclick="auditApp.setStatus('passed')" style="margin: 2px; padding: 4px 8px; font-size: 11px;">通过</button>
                <button onclick="auditApp.setStatus('loading')" style="margin: 2px; padding: 4px 8px; font-size: 11px;">加载中</button>
                <button onclick="auditApp.setStatus('error')" style="margin: 2px; padding: 4px 8px; font-size: 11px;">异常</button>
                <button onclick="auditApp.setStatus('hasIssues')" style="margin: 2px; padding: 4px 8px; font-size: 11px;">有问题</button>
            `;
            document.body.appendChild(testPanel);
        }
    }

    // 异步加载数据
    async loadDataAsync() {
        if (this.pendingTimer) {
            clearTimeout(this.pendingTimer);
            this.pendingTimer = null;
        }
        try {
            const loaded = await this.loadData();
            this.finishLoading();
            if (!loaded) {
                return;
            }
            this.renderUI();
            if (this.dataPending) {
                // 结果未落库，延迟重试
                this.currentStatus = 'loading';
                this.showLoading();
                this.pendingTimer = setTimeout(() => this.loadDataAsync(), 3000);
            }
        } catch (error) {
            console.error('数据加载失败:', error);
            this.handleLoadError(error);
        }
    }

    // 加载数据
    async loadData() {
        if (!this.previewId) {
            throw new Error('缺少预审ID参数');
        }

        console.log('正在加载预审数据，预审ID:', this.previewId);

        // 获取预审结果数据
        const resultResponse = await this.apiService.getMaterialsList(this.previewId);
        
        // 🎯 处理后端免登录检测响应
        if (resultResponse.need_auth) {
            console.log('🔐 后端检测需要用户认证，数据加载暂停');
            // API层已经处理跳转，这里只需要停止加载流程
            return false;
        }
        
        if (!resultResponse.success) {
            throw new Error('获取预审数据失败: ' + (resultResponse.errorMsg || resultResponse.message));
        }

        const transformedData = this.apiService.transformPreviewData(resultResponse);
        if (!transformedData) {
            throw new Error('数据转换失败');
        }

        this.dataPending = transformedData.auditStatus.result === 'loading';
        this.basicInfo = transformedData.basicInfo;
        this.materials = transformedData.materials;
        this.passedMaterials = transformedData.passedMaterials;
        this.auditStatus = transformedData.auditStatus;
        this.currentStatus = transformedData.auditStatus.result;
        this.dataLoaded = true;
        this.files = transformedData.files || {};

        const reportUrls = this.resolveReportUrls(this.files);
        this.reportDownloadUrl = reportUrls.primary;
        this.reportFallbackUrl = reportUrls.fallback;
        this.reportPdfUrl = reportUrls.pdf;
        this.reportHtmlUrl = reportUrls.html;
        this.reportPdfSource = reportUrls.pdfSource;
        this.reportHtmlSource = reportUrls.htmlSource;
        this.updateDownloadButtonState();

        console.log('数据加载完成:', {
            basicInfo: this.basicInfo,
            materialsCount: this.materials.length,
            currentStatus: this.currentStatus
        });

        return true;
    }

    // 处理加载错误
    handleLoadError(error) {
        console.error('数据加载失败:', error);
        this.finishLoading();
        
        this.currentStatus = 'error';
        this.dataLoaded = false;
        this.files = {};
        this.reportDownloadUrl = null;
        this.reportFallbackUrl = null;
        this.reportPdfUrl = null;
        this.reportHtmlUrl = null;
        this.reportPdfSource = null;
        this.reportHtmlSource = null;
        this.updateDownloadButtonState();
        this.renderErrorState('数据加载失败: ' + error.message);
    }

    // 渲染错误状态（生产环境专用）
    renderErrorState(message) {
        const statusDisplayComponent = this.componentManager.get('statusDisplay');
        statusDisplayComponent.render('error', message);
        
        // 清空左侧面板内容，显示错误信息
        const basicInfoCard = document.querySelector('.basic-info-card');
        const materialsCard = document.querySelector('.materials-card');
        const passedMaterialsCard = document.querySelector('.passed-materials-card');
        
        if (basicInfoCard) {
            basicInfoCard.innerHTML = `
                <h3 class="card-title">系统错误</h3>
                <div class="info-item">
                    <span class="info-label">错误信息：</span>
                    <span class="info-value">${message}</span>
                </div>
                <div class="info-item">
                    <span class="info-label">解决方案：</span>
                    <span class="info-value">请联系系统管理员或稍后重试</span>
                </div>
            `;
        }
        
        if (materialsCard) {
            materialsCard.innerHTML = `
                <h3 class="card-title">无法加载材料数据</h3>
                <p style="color: #666; font-size: 14px; margin: 16px 0;">
                    由于系统错误，无法显示材料列表。请检查网络连接或联系技术支持。
                </p>
            `;
        }
        
        if (passedMaterialsCard) {
            passedMaterialsCard.style.display = 'none';
        }
    }

    // 渲染UI
    renderUI() {
        // 渲染基本信息
        this.componentManager.get('basicInfo').render(this.basicInfo);

        // 渲染材料列表
        this.componentManager.get('materialsList').render(this.materials);

        // 渲染已通过材料
        this.componentManager.get('passedMaterials').render(this.passedMaterials);

        // 渲染状态显示
        this.updateStatusDisplay();

        // 更新检查项目数量
        this.updateCheckCount();

        // 根据可下载性刷新按钮状态
        this.updateDownloadButtonState();
    }

    // 更新状态显示
    updateStatusDisplay() {
        const statusDisplay = this.componentManager.get('statusDisplay');

        if (!statusDisplay) {
            return;
        }

        if (!this.dataLoaded) {
            statusDisplay.render(this.currentStatus || 'loading');
            return;
        }

        if (this.currentStatus === 'loading') {
            statusDisplay.render('loading');
            return;
        }

        const stats = this.calculateMaterialStats();
        const summaryMessage = this.buildSummaryMessage(this.currentStatus, stats);

        statusDisplay.render('summary', {
            status: this.currentStatus,
            stats,
            message: summaryMessage
        });
    }

    // 更新检查项目数量
    updateCheckCount() {
        const issueCount = this.materials.filter(m => m.status !== 'passed').length;
        document.getElementById('check-count').textContent = `${issueCount}项`;
    }

    calculateMaterialStats() {
        const stats = {
            total: this.materials.length,
            passed: 0,
            warnings: 0,
            failed: 0,
            pending: 0,
            problemMaterials: []
        };

        this.materials.forEach((material) => {
            const status = (material.status || '').toLowerCase();
            switch (status) {
                case 'passed':
                    stats.passed += 1;
                    break;
                case 'error':
                    stats.failed += 1;
                    stats.problemMaterials.push(material);
                    break;
                case 'warning':
                case 'hasissues':
                case 'has_issues':
                    stats.warnings += 1;
                    stats.problemMaterials.push(material);
                    break;
                default:
                    stats.pending += 1;
                    break;
            }
        });

        stats.issues = stats.warnings + stats.failed;
        return stats;
    }

    buildSummaryMessage(status, stats) {
        if (!stats) {
            return '';
        }

        if (stats.issues === 0 && stats.failed === 0) {
            return `全部${stats.total || 0}项材料已通过系统预审。`;
        }

        if (stats.failed > 0) {
            return `共有${stats.failed}项材料未通过，请立即核对问题附件。`;
        }

        if (stats.warnings > 0) {
            return `有${stats.warnings}项材料需要人工确认，请优先处理。`;
        }

        if (status === 'error') {
            return '部分材料未能生成结果，请稍后重试或联系技术支持。';
        }

        return '预审结果已生成，请按提示完成后续流程。';
    }

    resolveReportUrls(files = {}) {
        const pdfInfo = files?.pdf || {};
        const htmlInfo = files?.html || {};

        const pdfUrl = pdfInfo.downloadUrl || pdfInfo.legacyDownloadUrl || null;
        const htmlUrl = htmlInfo.downloadUrl || htmlInfo.legacyDownloadUrl || null;

        const pdfSource =
            pdfInfo.source ||
            (pdfInfo.downloadUrl
                ? 'remote'
                : pdfInfo.exists
                ? 'local'
                : null);
        const htmlSource =
            htmlInfo.source ||
            (htmlInfo.downloadUrl
                ? 'remote'
                : htmlInfo.exists
                ? 'local'
                : null);

        const primary = pdfUrl || htmlUrl || null;
        const fallback = htmlUrl || pdfUrl || null;

        return {
            primary,
            fallback,
            pdf: pdfUrl,
            html: htmlUrl,
            pdfSource,
            htmlSource,
        };
    }

    getReportDownloadUrl() {
        return this.reportDownloadUrl || this.reportFallbackUrl || null;
    }

    updateDownloadButtonState() {
        const pdfButton = document.getElementById('download-report-pdf');
        const htmlButton = document.getElementById('download-report-html');

        const pdfInfo = this.files?.pdf || {};
        const htmlInfo = this.files?.html || {};

        if (pdfButton) {
            const hasPdf =
                !!this.reportPdfUrl && (pdfInfo.exists === undefined || pdfInfo.exists);
            pdfButton.style.display = hasPdf ? 'inline-flex' : 'none';
            pdfButton.disabled = !hasPdf;
            pdfButton.title = hasPdf
                ? (this.reportPdfSource === 'remote'
                    ? '从存储下载PDF报告'
                    : '下载PDF报告')
                : '暂无PDF报告';
        }

        if (htmlButton) {
            const hasHtml =
                !!this.reportHtmlUrl && (htmlInfo.exists === undefined || htmlInfo.exists);
            htmlButton.style.display = hasHtml ? 'inline-flex' : 'none';
            htmlButton.disabled = !hasHtml;
            htmlButton.title = hasHtml
                ? (this.reportHtmlSource === 'remote'
                    ? '从存储下载HTML报告'
                    : '下载HTML报告')
                : '暂无HTML报告';
        }
    }

    normalizeInternalUrl(url) {
        if (!url) return '';
        try {
            const parsed = new URL(url, window.location.origin);
            const placeholderHosts = new Set(['0.0.0.0', '127.0.0.1', 'localhost']);
            if (placeholderHosts.has(parsed.hostname)) {
                parsed.protocol = window.location.protocol;
                parsed.host = window.location.host;
            }
            return parsed.origin === window.location.origin
                ? `${parsed.pathname}${parsed.search}${parsed.hash}`
                : parsed.toString();
        } catch (error) {
            // URL 解析失败时直接返回原值
            return url;
        }
    }

    ensureAbsoluteUrl(url) {
        if (!url) return url;
        if (/^https?:\/\//i.test(url)) {
            return url;
        }
        try {
            return new URL(url, window.location.origin).toString();
        } catch (error) {
            return url;
        }
    }

    ensureApiPrefixedUrl(url) {
        if (!url) {
            return url;
        }

        const absolutePattern = /^https?:\/\//i;
        if (absolutePattern.test(url)) {
            return url;
        }

        const baseUrl = this.apiService?.baseUrl || '/api';
        const normalizedTarget = url.startsWith('/') ? url : `/${url}`;

        const normalizePath = (path) => {
            if (!path) {
                return '/';
            }
            let result = path.startsWith('/') ? path : `/${path}`;
            if (result.length > 1 && result.endsWith('/')) {
                result = result.slice(0, -1);
            }
            return result || '/';
        };

        const joinPath = (basePath, relativePath) => {
            const sanitizedBase = basePath === '/' ? '' : basePath;
            const sanitizedRelative = relativePath.startsWith('/')
                ? relativePath
                : `/${relativePath}`;
            return `${sanitizedBase}${sanitizedRelative}`;
        };

        if (!absolutePattern.test(baseUrl)) {
            const basePath = normalizePath(baseUrl);
            if (
                basePath === '/' ||
                normalizedTarget === basePath ||
                normalizedTarget.startsWith(`${basePath}/`)
            ) {
                return normalizedTarget;
            }
            return joinPath(basePath, normalizedTarget);
        }

        try {
            const parsedBase = new URL(baseUrl);
            const baseOrigin = `${parsedBase.protocol}//${parsedBase.host}`;
            const basePath = normalizePath(parsedBase.pathname || '/');
            const alreadyPrefixed =
                basePath !== '/' &&
                (normalizedTarget === basePath ||
                    normalizedTarget.startsWith(`${basePath}/`));
            const finalPath = alreadyPrefixed
                ? normalizedTarget
                : joinPath(basePath, normalizedTarget);
            return `${baseOrigin}${finalPath}`;
        } catch (error) {
            console.warn('解析API基础路径失败，回退到默认下载路径:', error);
            return normalizedTarget;
        }
    }

    appendMonitorSession(url) {
        if (!url) {
            return url;
        }

        let normalized = this.normalizeInternalUrl(url);

        // 尝试实时获取最新的监控会话ID
        if (!this.monitorSessionId) {
            this.monitorSessionId = this.resolveMonitorSessionId();
        }

        if (!this.monitorSessionId) {
            return normalized;
        }

        try {
            const targetUrl = new URL(normalized, window.location.origin);
            if (targetUrl.origin !== window.location.origin) {
                return normalized;
            }
            targetUrl.searchParams.set('monitor_session_id', this.monitorSessionId);
            return targetUrl.origin === window.location.origin
                ? `${targetUrl.pathname}${targetUrl.search}${targetUrl.hash}`
                : targetUrl.toString();
        } catch (error) {
            console.warn('附加monitor_session_id失败:', error);
            if (/^https?:\/\//i.test(normalized)) {
                return normalized;
            }
            // 退回到简单字符串拼接
            const hasQuery = normalized.includes('?');
            const connector = hasQuery ? '&' : '?';
            return `${normalized}${connector}monitor_session_id=${encodeURIComponent(this.monitorSessionId)}`;
        }
    }

    // 處理材料展開/收起
    handleMaterialToggle(materialId) {
        const material = this.materials.find(
            m => String(m.id) === String(materialId)
        );
        if (material) {
            material.expanded = !material.expanded;
            this.componentManager.get('materialsList').render(this.materials);
        }
    }

    // 处理文档点击
    handleDocumentClick(itemData, material) {
        if (!itemData) {
            console.warn('未找到附件信息');
            return;
        }

        const documentMeta = this.buildDocumentMeta(itemData, material);

        if (itemData.documentType === 'text') {
            this.componentManager.get('documentPreview').render({
                type: 'text',
                name: itemData.name,
                content: itemData.documentContent || '暂无内容',
                meta: documentMeta
            });
            return;
        }

        const primaryUrl = itemData.documentUrl || itemData.documentThumbnail || '';

        if (
            !primaryUrl &&
            !itemData.documentContent &&
            itemData.documentType !== 'image'
        ) {
            this.componentManager.get('documentPreview').render({
                type: 'empty',
                name: itemData.name,
                message: itemData.checkPoint || '该附件暂无可预览内容',
                meta: documentMeta
            });
            return;
        }

        const pages = Array.isArray(itemData.documentPages) ? itemData.documentPages : [];

        this.componentManager.get('documentPreview').render({
            type: itemData.documentType || 'file',
            name: itemData.name,
            url: primaryUrl,
            mimeType: itemData.documentMime || '',
            content: itemData.documentContent,
            pages,
            downloadUrl: itemData.downloadUrl || primaryUrl,
            meta: documentMeta
        });
    }

    // 显示文档预览
    showDocumentPreview(documentData) {
        // 在右侧面板显示文档预览
        this.componentManager.get('documentPreview').render(documentData);
    }

    buildDocumentMeta(itemData, material) {
        const pageCount = Array.isArray(itemData.documentPages) && itemData.documentPages.length
            ? itemData.documentPages.length
            : (itemData.pageCount ?? null);

        const downloadUrl = itemData.downloadUrl || itemData.documentUrl || '';

        return {
            fileSize: itemData.fileSize ?? null,
            pageCount,
            isCloudShare: itemData.isCloudShare ?? false,
            ocrSuccess: itemData.ocrSuccess !== false,
            materialName: material?.name || '',
            materialCode: material?.materialCode || material?.id || '',
            downloadUrl,
            note: itemData.checkPoint || ''
        };
    }

    // 显示文档预览弹窗
    showDocumentModal(documentType, documentName) {
        const modal = document.getElementById('document-modal');
        const title = document.getElementById('document-title');
        const image = document.getElementById('document-image');

        title.textContent = documentName;

        // 根据文档类型生成不同的预览图像
        if (documentType === 'license') {
            image.src = utils.createDocumentImage('license');
        } else if (documentType === 'table') {
            image.src = utils.createDocumentImage('table');
        } else {
            image.src = utils.createDocumentImage('default');
        }

        modal.classList.add('show');
    }

    // 隐藏文档预览弹窗
    hideDocumentPreview() {
        document.getElementById('document-modal').classList.remove('show');

        // 恢复状态显示
        this.updateStatusDisplay();
    }

    // 設置狀態（測試用）
    setStatus(status) {
        this.currentStatus = status;
        this.updateStatusDisplay();
        
        if (status === 'loading') {
            this.showLoading();
        } else if (status === 'error') {
            this.finishLoading();
            this.showErrorModal();
        } else {
            this.finishLoading();
            this.hideErrorModal();
        }
    }

    // 顯示加載狀態
    showLoading() {
        const overlay = document.getElementById('loading-overlay');
        const progressFill = document.getElementById('progress-fill');
        const progressPercent = document.getElementById('progress-percent');
        const estimatedTime = document.getElementById('estimated-time');

        this.loadingActive = true;
        this.loadingProgress = 0;

        overlay.classList.add('show');
        progressFill.style.width = '0%';
        progressPercent.textContent = '0%';
        estimatedTime.textContent = '3';

        if (this.loadingInterval) {
            clearInterval(this.loadingInterval);
        }

        this.loadingInterval = setInterval(() => {
            if (!this.loadingActive) {
                clearInterval(this.loadingInterval);
                this.loadingInterval = null;
                return;
            }

            if (this.loadingProgress >= 95) {
                this.loadingProgress = 95;
            } else {
                const increment = this.loadingProgress < 30 ? 1.8 :
                                 this.loadingProgress < 60 ? 0.9 :
                                 this.loadingProgress < 85 ? 0.35 : 0.15;
                this.loadingProgress = Math.min(this.loadingProgress + increment, 95);
            }

            progressFill.style.width = `${this.loadingProgress}%`;
            progressPercent.textContent = `${Math.round(this.loadingProgress)}%`;

            const timeLeft = Math.max(1, Math.ceil((95 - this.loadingProgress) / 18));
            estimatedTime.textContent = timeLeft;
        }, 180);
    }

    // 隱藏加載狀態
    hideLoading() {
        const overlay = document.getElementById('loading-overlay');
        overlay.classList.remove('show');
        this.loadingActive = false;
        if (this.loadingInterval) {
            clearInterval(this.loadingInterval);
            this.loadingInterval = null;
        }
    }

    finishLoading() {
        if (!this.loadingActive) {
            return;
        }

        if (this.loadingInterval) {
            clearInterval(this.loadingInterval);
            this.loadingInterval = null;
        }

        const progressFill = document.getElementById('progress-fill');
        const progressPercent = document.getElementById('progress-percent');
        const estimatedTime = document.getElementById('estimated-time');

        this.loadingProgress = 100;
        progressFill.style.width = '100%';
        progressPercent.textContent = '100%';
        estimatedTime.textContent = '0';

        setTimeout(() => {
            this.hideLoading();
        }, 250);
    }

    // 顯示錯誤彈窗
    showErrorModal() {
        document.getElementById('error-modal').classList.add('show');
    }

    // 隱藏錯誤彈窗
    hideErrorModal() {
        document.getElementById('error-modal').classList.remove('show');
    }

    // 開始審核
    startAudit() {
        this.currentStatus = 'loading';
        this.showLoading();
        
        // 在實際應用中，這裡會調用API開始審核
        // const response = await this.apiService.startAudit(this.applicationId);
        // this.auditId = response.data.auditId;
        // this.monitorAuditProgress();
    }

    // 監控審核進度
    async monitorAuditProgress() {
        if (!this.auditId) return;
        
        const checkProgress = async () => {
            try {
                const response = await this.apiService.getAuditProgress(this.auditId);
                const { progress, status, estimatedTime } = response.data;
                
                // 更新進度條
                document.getElementById('progress-fill').style.width = `${progress}%`;
                document.getElementById('progress-percent').textContent = `${progress}%`;
                document.getElementById('estimated-time').textContent = estimatedTime;
                
                if (status === 'completed') {
                    // 審核完成
                    this.finishLoading();
                    await this.loadData(); // 重新加載數據
                    this.renderUI();
                } else if (status === 'error') {
                    // 審核出錯
                    this.finishLoading();
                    this.currentStatus = 'error';
                    this.updateStatusDisplay();
                    this.showErrorModal();
                } else {
                    // 繼續監控進度
                    setTimeout(checkProgress, 1000);
                }
            } catch (error) {
                console.error('Failed to monitor audit progress:', error);
                this.finishLoading();
                this.currentStatus = 'error';
                this.updateStatusDisplay();
                this.showErrorModal();
            }
        };
        
        // 開始監控進度
        checkProgress();
    }

    // 导出材料
    exportMaterials() {
        alert('导出材料功能');

        // 在实际应用中，这里会调用API导出材料
        // const response = await this.apiService.exportMaterials(this.applicationId);
        // window.open(response.data.url, '_blank');
    }

    // 查看审核依据
    viewAuditBasis() {
        this.showDocumentModal('license', '审核依据材料');

        // 在实际应用中，这里会从API获取审核依据材料
        // const response = await this.apiService.getAuditBasis(this.applicationId);
        // this.showDocumentPreview({
        //     type: response.data.type,
        //     name: response.data.name,
        //     content: response.data.content
        // });
    }

    // 下载报告
    downloadReport(format = 'pdf') {
        let downloadUrl = null;

        if (format === 'html') {
            downloadUrl =
                this.reportHtmlUrl ||
                this.reportPdfUrl ||
                this.reportFallbackUrl ||
                this.getReportDownloadUrl();
        } else {
            downloadUrl =
                this.reportPdfUrl ||
                this.reportHtmlUrl ||
                this.reportFallbackUrl ||
                this.getReportDownloadUrl();
        }

        if (!downloadUrl) {
            alert('暂无可下载的预审报告，请稍后重试。');
            return;
        }

        const withApiPrefix = this.ensureApiPrefixedUrl(downloadUrl);
        const withSession = this.appendMonitorSession(withApiPrefix);
        const normalized = this.normalizeInternalUrl(withSession);
        const finalUrl = this.ensureAbsoluteUrl(normalized);
        if (!finalUrl) {
            alert('无法构建下载链接，请稍后重试。');
            return;
        }
        window.open(finalUrl, '_blank');
    }

    // 系统状态监控
    startSystemStatusMonitoring() {
        // 立即执行一次状态检查
        this.updateSystemStatus();
        
        // 每30秒检查一次系统状态
        setInterval(() => {
            this.updateSystemStatus();
        }, 30000);
    }

    // 更新系统状态
    async updateSystemStatus() {
        try {
            const queueStatus = await this.apiService.getQueueStatus();
            if (queueStatus && queueStatus.success) {
                this.displaySystemStatus(queueStatus.data);
            }
        } catch (error) {
            console.warn('获取系统状态失败:', error);
            this.displaySystemStatus(null);
        }
    }

    // 显示系统状态
    displaySystemStatus(statusData) {
        const statusElement = document.getElementById('system-status');
        const indicatorElement = document.getElementById('status-indicator');
        const textElement = document.getElementById('status-text');

        if (!statusData || !statusData.queue) {
            statusElement.style.display = 'none';
            return;
        }

        const { queue, performance } = statusData;
        const loadPercent = queue.system_load_percent || 0;

        statusElement.style.display = 'block';

        // 根据系统负载设置状态颜色和文本
        if (loadPercent < 70) {
            indicatorElement.style.color = '#00cc66';
            textElement.textContent = '系统正常';
        } else if (loadPercent < 90) {
            indicatorElement.style.color = '#ffaa00';
            textElement.textContent = '系统繁忙';
        } else {
            indicatorElement.style.color = '#ff4444';
            textElement.textContent = '系统过载';
        }

        // 设置tooltip显示详细信息
        const tooltip = `处理槽位: ${queue.processing_tasks}/${queue.max_concurrent_tasks}\n系统负载: ${loadPercent}%\n可用槽位: ${queue.available_slots}`;
        statusElement.title = tooltip;
    }
}

// 初始化应用
let auditApp;
document.addEventListener('DOMContentLoaded', () => {
    auditApp = new IntelligentAuditApp();
    
    // 🖼️ 确保全局可访问，供前端组件调用
window.auditApp = auditApp;
});
})();
