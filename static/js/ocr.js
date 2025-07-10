// 材料智能预审模块
const MaterialReview = {
    selectedFiles: [],
    ocrResults: [],
    currentStep: 'upload', // upload, progress, result
    availableThemes: [],
    selectedTheme: null,

    async init() {
        // 首先初始化配置管理器
        await ConfigManager.init();
        
        this.bindEvents();
        this.showUploadInterface();
        this.loadThemes();
    },

    bindEvents() {
        const fileInput = document.getElementById('fileInput');
        const uploadBtn = document.getElementById('uploadBtn');
        const exportBtn = document.getElementById('exportBtn');
        const copyResultBtn = document.getElementById('copyResultBtn');
        const elderModeBtn = document.querySelector('.elder-mode-btn');

        // 测试后门快捷键
        document.addEventListener('keydown', (e) => {
            if (e.ctrlKey && e.shiftKey && e.key === 'T') {
                e.preventDefault();
                if (confirm('是否进入测试模式？\n\n这将打开开发者测试后门页面。')) {
                    window.location.href = '/static/test.html';
                }
            }
        });

        // 文件选择事件
        if (fileInput) {
            fileInput.addEventListener('change', (e) => {
                this.handleFileSelect(e.target.files);
            });
        }

        // 开始预审按钮
        if (uploadBtn) {
            uploadBtn.addEventListener('click', () => this.startReview());
        }

        // 导出材料按钮
        if (exportBtn) {
            exportBtn.addEventListener('click', () => this.exportMaterials());
        }

        // 复制检查结果按钮
        if (copyResultBtn) {
            copyResultBtn.addEventListener('click', () => this.copyResults());
        }

        // 适老模式按钮
        if (elderModeBtn) {
            elderModeBtn.addEventListener('click', () => this.toggleElderMode());
        }

        // 主题选择事件
        const themeSelect = document.getElementById('themeSelect');
        if (themeSelect) {
            themeSelect.addEventListener('change', (e) => {
                this.selectedTheme = e.target.value || null;
                this.showToast(`已选择主题: ${e.target.options[e.target.selectedIndex].text}`, 'success');
            });
        }

        // 页面加载时自动触发文件选择（模拟从其他系统跳转过来）
        setTimeout(() => {
            this.simulateFileUpload();
        }, 1000);
    },

    // 显示文件上传界面（实际上是隐藏的，因为是从其他系统跳转过来）
    showUploadInterface() {
        // 在实际使用中，这里可能会接收从其他系统传递过来的文件
        console.log('材料智能预审系统已准备就绪');
    },

    // 模拟从其他系统接收文件
    simulateFileUpload() {
        // 使用配置管理器获取系统信息
        const systemInfo = ConfigManager.getSystemInfo();
        const defaultUser = ConfigManager.getDefaultUser();
        
        console.log('系统信息:', systemInfo);
        console.log('默认用户:', defaultUser);
        
        // 模拟接收到文件，直接开始预审流程
        this.selectedFiles = [
            { name: '营业执照.pdf', size: 1024000 },
            { name: '法人身份证.jpg', size: 512000 },
            { name: '公司章程.pdf', size: 2048000 }
        ];
        this.startReview();
    },

    handleFileSelect(files) {
        this.selectedFiles = Array.from(files);
        console.log('接收到文件:', this.selectedFiles.map(f => f.name));
    },

    // 开始材料预审流程
    async startReview() {
        console.log('开始材料智能预审...');
        this.currentStep = 'progress';
        this.showProgressPage();

        // 模拟预审进度
        await this.simulateProgress();

        // 执行实际的OCR识别
        await this.performOCR();

        // 显示预审结果
        this.showResults();
    },

    // 显示进度页面
    showProgressPage() {
        const progressSection = document.getElementById('progressSection');
        const resultSection = document.getElementById('resultSection');

        progressSection.style.display = 'block';
        resultSection.style.display = 'none';

        // 重置进度条
        const progressFill = document.getElementById('progressFill');
        const progressText = document.getElementById('progressText');

        progressFill.style.width = '0%';
        progressText.textContent = '正在进行材料预审，预计需要等待 3 分钟';
    },

    // 模拟预审进度
    async simulateProgress() {
        const progressFill = document.getElementById('progressFill');
        const progressText = document.getElementById('progressText');

        const steps = [
            { progress: 20, text: '正在分析材料类型...' },
            { progress: 40, text: '正在进行OCR文字识别...' },
            { progress: 60, text: '正在检查材料完整性...' },
            { progress: 80, text: '正在生成预审报告...' },
            { progress: 100, text: '预审完成，正在生成结果...' }
        ];

        for (const step of steps) {
            await new Promise(resolve => setTimeout(resolve, 800));
            progressFill.style.width = step.progress + '%';
            progressText.textContent = step.text;
        }
    },

    // 执行OCR识别
    async performOCR() {
        this.ocrResults = [];

        try {
            for (let i = 0; i < this.selectedFiles.length; i++) {
                const file = this.selectedFiles[i];

                // 如果是真实文件，调用API
                if (file instanceof File) {
                    const formData = new FormData();
                    formData.append('file', file);

                    const response = await fetch('/api/upload', {
                        method: 'POST',
                        body: formData
                    });

                    const result = await response.json();

                    if (result.success && result.data) {
                        this.ocrResults.push({
                            fileName: file.name,
                            content: result.data
                        });
                    } else {
                        this.ocrResults.push({
                            fileName: file.name,
                            content: ['识别失败: ' + (result.errorMsg || '未知错误')]
                        });
                    }
                } else {
                    // 模拟数据
                    this.ocrResults.push({
                        fileName: file.name,
                        content: this.generateMockOCRResult(file.name)
                    });
                }
            }
        } catch (error) {
            console.error('OCR识别错误:', error);
            this.showToast('材料识别失败: ' + error.message, 'error');
        }
    },

    // 生成模拟OCR结果
    generateMockOCRResult(fileName) {
        // 使用配置管理器获取默认用户信息
        const defaultUser = ConfigManager.getDefaultUser();
        const matters = ConfigManager.getEnabledMatters();
        const currentMatter = matters[0] || { matterName: "默认事项" };
        
        const mockResults = {
            '营业执照.pdf': [
                '统一社会信用代码：91330000MA28A1234X',
                `名称：${defaultUser.organizationName || '浙江一二三四五六有限公司'}`,
                '类型：有限责任公司',
                `法定代表人：${defaultUser.userName || '张三'}`,
                '注册资本：1000万元人民币',
                '成立日期：2020年01月15日',
                '营业期限：2020年01月15日至2050年01月14日',
                '经营范围：技术开发、技术服务...'
            ],
            '法人身份证.jpg': [
                `姓名：${defaultUser.userName || '张三'}`,
                '性别：男',
                '民族：汉',
                '出生：1980年05月20日',
                '住址：浙江省杭州市西湖区某某街道123号',
                `公民身份号码：${defaultUser.certificateNumber || '330106198005201234'}`
            ],
            '公司章程.pdf': [
                '第一章 总则',
                '第一条 为规范公司的组织和行为...',
                `第二条 公司名称：${defaultUser.organizationName || '浙江一二三四五六有限公司'}`,
                '第三条 公司住所：浙江省杭州市...',
                '第二章 公司经营范围和经营期限',
                '第四条 公司经营范围：技术开发...'
            ]
        };

        return mockResults[fileName] || ['文档内容识别中...'];
    },

    // 显示预审结果
    showResults() {
        const progressSection = document.getElementById('progressSection');
        const resultSection = document.getElementById('resultSection');

        progressSection.style.display = 'none';
        resultSection.style.display = 'block';

        this.currentStep = 'result';

        // 生成材料检查结果
        this.generateMaterialCheckResults();
    },

    // 生成材料检查结果
    generateMaterialCheckResults() {
        const materialList = document.getElementById('materialList');
        const passedMaterialList = document.getElementById('passedMaterialList');

        // 需检查的材料（有问题的）
        const problemMaterials = [
            {
                name: '《营业执照》副本',
                count: 2,
                status: 'error',
                details: [
                    { name: '基本工商信息表', status: 'error', action: '查看' },
                    { name: '法人身份证明', status: 'success', action: '查看' }
                ]
            },
            {
                name: '《法定代表人身份证明》',
                count: 1,
                status: 'warning',
                details: [
                    { name: '身份证正面', status: 'success', action: '查看' },
                    { name: '身份证反面', status: 'error', action: '查看' }
                ]
            }
        ];

        // 已通过的材料
        const passedMaterials = [
            {
                name: '《公司章程》',
                count: 4,
                status: 'success',
                details: [
                    { name: '公司基本信息', status: 'success', action: '查看' },
                    { name: '股东信息', status: 'success', action: '查看' },
                    { name: '经营范围', status: 'success', action: '查看' },
                    { name: '注册资本', status: 'success', action: '查看' }
                ]
            }
        ];

        // 渲染需检查的材料
        materialList.innerHTML = '';
        problemMaterials.forEach(material => {
            const materialItem = this.createMaterialItem(material);
            materialList.appendChild(materialItem);
        });

        // 渲染已通过的材料
        passedMaterialList.innerHTML = '';
        passedMaterials.forEach(material => {
            const materialItem = this.createMaterialItem(material);
            passedMaterialList.appendChild(materialItem);
        });
    },

    // 创建材料项目元素
    createMaterialItem(material) {
        const itemDiv = document.createElement('div');
        itemDiv.className = 'material-item';

        const statusIcon = material.status === 'error' ? '!' :
                          material.status === 'warning' ? '⚠' : '✓';

        itemDiv.innerHTML = `
            <div class="material-info">
                <div class="material-status ${material.status}">${statusIcon}</div>
                <div class="material-name">
                    ${material.name}
                    <span class="material-count">(${material.count})</span>
                </div>
            </div>
            <div class="material-actions">
                <button class="material-expand" onclick="MaterialReview.toggleMaterialDetails(this)">
                    ${material.status === 'success' ? '▼' : '▼'}
                </button>
            </div>
        `;

        // 添加详情区域
        const detailsDiv = document.createElement('div');
        detailsDiv.className = 'material-details';

        material.details.forEach(detail => {
            const detailItem = document.createElement('div');
            detailItem.className = 'detail-item';
            detailItem.innerHTML = `
                <div class="detail-status ${detail.status}"></div>
                <div class="detail-name">${detail.name}</div>
                <a href="#" class="detail-action" onclick="MaterialReview.viewDetail('${detail.name}')">${detail.action}</a>
            `;
            detailsDiv.appendChild(detailItem);
        });

        const containerDiv = document.createElement('div');
        containerDiv.appendChild(itemDiv);
        containerDiv.appendChild(detailsDiv);

        return containerDiv;
    },

    // 切换材料详情显示
    toggleMaterialDetails(button) {
        const materialItem = button.closest('div').parentElement;
        const details = materialItem.nextElementSibling;
        const isExpanded = details.classList.contains('expanded');

        if (isExpanded) {
            details.classList.remove('expanded');
            button.textContent = '▼';
        } else {
            details.classList.add('expanded');
            button.textContent = '▲';
        }
    },

    // 查看详情
    viewDetail(detailName) {
        console.log('查看详情:', detailName);
        // 这里可以显示具体的OCR识别结果
        const result = this.ocrResults.find(r => r.fileName.includes(detailName));
        if (result) {
            alert(`${detailName} 识别内容:\n${result.content.join('\n')}`);
        }
    },

    // 导出材料
    exportMaterials() {
        let content = '材料智能预审结果\n';
        content += '=' .repeat(50) + '\n\n';

        content += '基本信息:\n';
        content += '申请人: 浙江一二三四五六有限公司\n';
        content += '事项名称: 内资公司设立\n';
        content += '事项类型: 设立登记\n\n';

        content += '需检查的材料:\n';
        content += '1. 《营业执照》副本 - 需要完善\n';
        content += '2. 《法定代表人身份证明》 - 需要完善\n\n';

        content += '已通过材料:\n';
        content += '1. 《公司章程》 - 已通过\n\n';

        content += 'OCR识别详细内容:\n';
        this.ocrResults.forEach(result => {
            content += `\n=== ${result.fileName} ===\n`;
            if (Array.isArray(result.content)) {
                content += result.content.join('\n');
            } else {
                content += result.content;
            }
            content += '\n';
        });

        const blob = new Blob([content], { type: 'text/plain;charset=utf-8' });
        const url = window.URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = `材料预审结果_${new Date().toISOString().slice(0, 10)}.txt`;
        a.click();
        window.URL.revokeObjectURL(url);

        this.showToast('材料导出成功', 'success');
    },

    // 复制检查结果
    copyResults() {
        let text = '材料智能预审结果\n\n';
        text += '需检查的材料:\n';
        text += '• 《营业执照》副本 - 基本工商信息表需要完善\n';
        text += '• 《法定代表人身份证明》 - 身份证反面需要完善\n\n';
        text += '已通过材料:\n';
        text += '• 《公司章程》 - 所有信息完整\n';

        navigator.clipboard.writeText(text).then(() => {
            this.showToast('检查结果已复制到剪贴板', 'success');
        }).catch(err => {
            this.showToast('复制失败', 'error');
        });
    },

    // 切换适老模式
    toggleElderMode() {
        document.body.classList.toggle('elder-mode');
        const isElderMode = document.body.classList.contains('elder-mode');

        if (isElderMode) {
            // 增大字体，简化界面
            document.documentElement.style.fontSize = '18px';
            this.showToast('已开启适老模式', 'success');
        } else {
            document.documentElement.style.fontSize = '14px';
            this.showToast('已关闭适老模式', 'success');
        }
    },

    // 加载可用主题
    async loadThemes() {
        try {
            // 使用配置管理器获取主题数据
            this.availableThemes = ConfigManager.getEnabledThemes();
            this.displayThemeSelector();
            console.log('加载主题数据:', this.availableThemes);
        } catch (error) {
            console.error('加载主题失败:', error);
            // 如果配置管理器失败，尝试直接调用API
            try {
                const response = await fetch('/api/themes');
                const result = await response.json();
                
                if (result.success && result.data) {
                    this.availableThemes = result.data.themes;
                    this.displayThemeSelector();
                }
            } catch (apiError) {
                console.error('API加载主题也失败:', apiError);
            }
        }
    },

    // 显示主题选择器
    displayThemeSelector() {
        const themeSelect = document.getElementById('themeSelect');
        if (!themeSelect) return;

        themeSelect.innerHTML = '<option value="">默认规则</option>';
        
        this.availableThemes.forEach(theme => {
            const option = document.createElement('option');
            option.value = theme.id;
            option.textContent = `${theme.name} - ${theme.description}`;
            themeSelect.appendChild(option);
        });
    },

    async generatePreview() {
        if (this.ocrResults.length === 0) {
            this.showToast('请先进行OCR识别', 'error');
            return;
        }

        this.showLoading(true);

        try {
            // 构建预览数据 - 添加主题ID支持
            const previewData = {
                userId: sessionStorage.getItem('ticketId') || 'anonymous',
                preview: {
                    matterId: 'OCR_' + Date.now(),
                    matterType: 'OCR识别',
                    matterName: 'OCR识别结果预览',
                    copy: false,
                    channel: 'web',
                    requestId: 'req_' + Date.now(),
                    sequenceNo: 'seq_' + Date.now(),
                    // 将formData改为符合后端期望的Value格式
                    formData: this.ocrResults.map(result => {
                        const content = Array.isArray(result.content) ? result.content.join('\n') : result.content;
                        return {
                            fileName: result.fileName,
                            content: content
                        };
                    }),
                    materialData: [],
                    agentInfo: {
                        userId: sessionStorage.getItem('ticketId') || 'anonymous',
                        certificateType: '01'
                    },
                    subjectInfo: {
                        userId: sessionStorage.getItem('ticketId') || 'anonymous',
                        certificateType: '01'
                    },
                    // 添加主题ID
                    themeId: this.selectedTheme
                }
            };

            const response = await fetch('/api/preview', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json'
                },
                body: JSON.stringify(previewData)
            });

            const result = await response.json();
            
            if (result.success && result.data) {
                this.displayPreview(result.data.previewUrl);
                this.showToast('预览生成成功', 'success');
            } else {
                throw new Error(result.errorMsg || '预览生成失败');
            }
        } catch (error) {
            console.error('生成预览错误:', error);
            this.showToast('生成预览失败: ' + error.message, 'error');
        } finally {
            this.showLoading(false);
        }
    },

    displayPreview(previewUrl) {
        const previewSection = document.getElementById('previewSection');
        const previewFrame = document.getElementById('previewFrame');
        const downloadPreviewBtn = document.getElementById('downloadPreviewBtn');

        previewSection.style.display = 'block';
        previewFrame.src = previewUrl;
        downloadPreviewBtn.href = previewUrl;

        // 滚动到预览区域
        previewSection.scrollIntoView({ behavior: 'smooth' });
    },

    showLoading(show) {
        const loadingOverlay = document.getElementById('loadingOverlay');
        loadingOverlay.style.display = show ? 'flex' : 'none';
    },

    showToast(message, type = 'info') {
        const toast = document.getElementById('toast');
        toast.textContent = message;
        toast.className = 'toast show ' + type;

        setTimeout(() => {
            toast.classList.remove('show');
        }, 3000);
    }
};

// 初始化材料智能预审模块
document.addEventListener('DOMContentLoaded', () => {
    MaterialReview.init();
});