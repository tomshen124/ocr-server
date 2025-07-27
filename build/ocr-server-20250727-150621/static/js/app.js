// 智能预审系统主应用
class IntelligentAuditApp {
    constructor() {
        this.apiService = window.apiService;
        this.componentManager = new ComponentManager();
        this.currentStatus = 'loading'; // passed, loading, error, hasIssues
        this.previewId = this.getPreviewIdFromUrl(); // 从URL获取预审ID
        this.auditId = null;
        this.defaultData = this.apiService.getDefaultData();
        this.dataLoaded = false; // 数据是否加载成功
        this.init();
    }

    // 从URL获取预审ID
    getPreviewIdFromUrl() {
        const urlParams = new URLSearchParams(window.location.search);
        return urlParams.get('previewId') || urlParams.get('requestId') || urlParams.get('preview_id') || urlParams.get('request_id');
    }

    // 初始化应用
    async init() {
        this.initComponents();
        this.bindEvents();

        // 先显示loading状态
        this.currentStatus = 'loading';
        this.showLoading();

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
        document.getElementById('download-report').addEventListener('click', () => {
            this.downloadReport();
        });

        // 添加测试按钮（开发用）
        // this.addTestButtons(); // 注释掉调试按钮
    }

    // 添加測試按鈕（開發用）
    addTestButtons() {
        // 判斷是否為開發環境，只在開發環境中顯示測試按鈕
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
                <div style="margin-bottom: 8px; font-weight: bold;">測試狀態：</div>
                <button onclick="auditApp.setStatus('passed')" style="margin: 2px; padding: 4px 8px; font-size: 11px;">通過</button>
                <button onclick="auditApp.setStatus('loading')" style="margin: 2px; padding: 4px 8px; font-size: 11px;">加載中</button>
                <button onclick="auditApp.setStatus('error')" style="margin: 2px; padding: 4px 8px; font-size: 11px;">異常</button>
                <button onclick="auditApp.setStatus('hasIssues')" style="margin: 2px; padding: 4px 8px; font-size: 11px;">有問題</button>
            `;
            document.body.appendChild(testPanel);
        }
    }

    // 异步加载数据
    async loadDataAsync() {
        try {
            await this.loadData();
            this.hideLoading();
            this.renderUI();
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
        
        if (!resultResponse.success) {
            throw new Error('获取预审数据失败: ' + (resultResponse.errorMsg || resultResponse.message));
        }

        const transformedData = this.apiService.transformPreviewData(resultResponse);
        if (!transformedData) {
            throw new Error('数据转换失败');
        }

        this.basicInfo = transformedData.basicInfo;
        this.materials = transformedData.materials;
        this.passedMaterials = transformedData.passedMaterials;
        this.auditStatus = transformedData.auditStatus;
        this.currentStatus = transformedData.auditStatus.result;
        this.dataLoaded = true;

        console.log('数据加载完成:', {
            basicInfo: this.basicInfo,
            materialsCount: this.materials.length,
            currentStatus: this.currentStatus
        });
    }

    // 处理加载错误
    handleLoadError(error) {
        console.error('数据加载失败:', error);
        
        // 使用默认数据
        this.basicInfo = this.defaultData.basicInfo;
        this.materials = this.defaultData.materials;
        this.passedMaterials = this.defaultData.passedMaterials;
        this.auditStatus = this.defaultData.auditStatus;
        this.currentStatus = 'error';
        this.dataLoaded = false;

        this.hideLoading();
        this.renderUI();
        this.showErrorMessage(error.message);
    }

    // 显示错误消息
    showErrorMessage(message) {
        // 在页面上显示错误消息
        const errorDiv = document.createElement('div');
        errorDiv.style.cssText = `
            position: fixed;
            top: 20px;
            left: 50%;
            transform: translateX(-50%);
            background: #ff4444;
            color: white;
            padding: 12px 24px;
            border-radius: 4px;
            z-index: 9999;
            font-size: 14px;
        `;
        errorDiv.textContent = `错误: ${message}`;
        document.body.appendChild(errorDiv);

        // 5秒后自动移除
        setTimeout(() => {
            if (errorDiv.parentNode) {
                errorDiv.parentNode.removeChild(errorDiv);
            }
        }, 5000);
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
    }

    // 更新状态显示
    updateStatusDisplay() {
        const statusDisplay = this.componentManager.get('statusDisplay');

        switch (this.currentStatus) {
            case 'passed':
                statusDisplay.render('passed');
                break;
            case 'hasIssues':
                const issueCount = this.materials.filter(m => m.status !== 'passed').length;
                const message = `发现${issueCount}个需要注意的问题，请检查左侧标记的材料项目。`;
                statusDisplay.render('hasIssues', message);
                break;
            case 'error':
                statusDisplay.render('error', '系统暂时无法完成预审，请稍后重试。');
                break;
            case 'loading':
                statusDisplay.render('loading');
                break;
        }
    }

    // 更新检查项目数量
    updateCheckCount() {
        const issueCount = this.materials.filter(m => m.status !== 'passed').length;
        document.getElementById('check-count').textContent = `${issueCount}项`;
    }

    // 處理材料展開/收起
    handleMaterialToggle(materialId) {
        const material = this.materials.find(m => m.id === materialId);
        if (material) {
            material.expanded = !material.expanded;
            this.componentManager.get('materialsList').render(this.materials);
        }
    }

    // 处理文档点击
    handleDocumentClick(itemData) {
        // 在实际应用中，这里会从API获取文档数据
        // const documentResponse = await this.apiService.getDocumentPreview(itemData.documentId);

        // 使用模拟数据
        let documentData = {
            type: itemData.documentType,
            name: itemData.name,
            content: {}
        };

        // 根据文档类型显示不同的预览
        if (itemData.documentType === 'license') {
            documentData.content = {
                creditCode: '91330000XXXXXXXXXX',
                companyName: '浙江一二三科技有限公司',
                legalPerson: '张三',
                registeredCapital: '1000万元人民币',
                establishDate: '2020年01月01日'
            };
        }

        // 显示文档预览
        this.showDocumentPreview(documentData);
    }

    // 显示文档预览
    showDocumentPreview(documentData) {
        // 在右侧面板显示文档预览
        this.componentManager.get('documentPreview').render(documentData);
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
            this.showErrorModal();
        }
    }

    // 顯示加載狀態
    showLoading() {
        const overlay = document.getElementById('loading-overlay');
        overlay.classList.add('show');
        
        // 重置進度
        let progress = 0;
        const progressFill = document.getElementById('progress-fill');
        const progressPercent = document.getElementById('progress-percent');
        const estimatedTime = document.getElementById('estimated-time');
        
        progressFill.style.width = '0%';
        progressPercent.textContent = '0%';
        
        // 模擬進度增長
        const interval = setInterval(() => {
            // 進度增長速度隨進度增加而減慢
            const increment = progress < 30 ? 1.5 : 
                             progress < 60 ? 0.8 : 
                             progress < 90 ? 0.3 : 0.1;
            
            progress += increment;
            
            if (progress >= 100) {
                progress = 100;
                clearInterval(interval);
                setTimeout(() => {
                    this.hideLoading();
                    this.currentStatus = 'passed';
                    this.updateStatusDisplay();
                }, 1000);
            }
            
            progressFill.style.width = `${progress}%`;
            progressPercent.textContent = `${Math.round(progress)}%`;
            
            const timeLeft = Math.max(1, Math.round(3 * (100 - progress) / 100));
            estimatedTime.textContent = timeLeft;
        }, 100); // 更新頻率更高，使動畫更流暢
    }

    // 隱藏加載狀態
    hideLoading() {
        document.getElementById('loading-overlay').classList.remove('show');
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
                    this.hideLoading();
                    await this.loadData(); // 重新加載數據
                    this.renderUI();
                } else if (status === 'error') {
                    // 審核出錯
                    this.hideLoading();
                    this.currentStatus = 'error';
                    this.updateStatusDisplay();
                    this.showErrorModal();
                } else {
                    // 繼續監控進度
                    setTimeout(checkProgress, 1000);
                }
            } catch (error) {
                console.error('Failed to monitor audit progress:', error);
                this.hideLoading();
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
    downloadReport() {
        alert('下载检查要素清单');

        // 在实际应用中，这里会调用API下载报告
        // const response = await this.apiService.downloadCheckList(this.applicationId);
        // window.open(response.data.url, '_blank');
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

        if (!statusData) {
            // 状态获取失败
            statusElement.style.display = 'block';
            indicatorElement.style.color = '#ff4444';
            textElement.textContent = '系统状态未知';
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
});