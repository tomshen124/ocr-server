// OCR服务测试后门脚本
const TestBackdoor = {
    selectedFiles: [],
    config: null,
    
    init() {
        // 加载测试配置
        this.loadTestConfig();
        this.bindEvents();
        this.generateRequestId();
        this.log('🔧 测试后门已初始化');
    },

    // 加载测试配置
    loadTestConfig() {
        if (window.TestConfig) {
            this.config = window.TestConfig;
            this.log('✅ 测试配置已加载');
        } else {
            console.warn('⚠️ 测试配置未找到，使用默认设置');
            this.config = {
                baseConfig: { debugMode: true },
                log: (msg, level) => console.log(`[BACKDOOR-${level?.toUpperCase()}]`, msg)
            };
        }
    },

    // 统一日志输出
    log(message, level = 'info') {
        if (this.config && this.config.log) {
            this.config.log(`[后门工具] ${message}`, level);
        } else {
            console.log(`[BACKDOOR] ${message}`);
        }
    },

    bindEvents() {
        const fileInput = document.getElementById('fileInput');
        const fileUploadArea = document.getElementById('fileUploadArea');

        // 文件选择事件
        fileInput.addEventListener('change', (e) => {
            this.handleFileSelect(e.target.files);
        });

        // 拖拽上传
        fileUploadArea.addEventListener('click', () => {
            fileInput.click();
        });

        fileUploadArea.addEventListener('dragover', (e) => {
            e.preventDefault();
            fileUploadArea.classList.add('dragover');
        });

        fileUploadArea.addEventListener('dragleave', () => {
            fileUploadArea.classList.remove('dragover');
        });

        fileUploadArea.addEventListener('drop', (e) => {
            e.preventDefault();
            fileUploadArea.classList.remove('dragover');
            this.handleFileSelect(e.dataTransfer.files);
        });
    },

    // 生成请求ID
    generateRequestId() {
        const timestamp = Date.now();
        const random = Math.random().toString(36).substr(2, 5);
        document.getElementById('requestId').value = `REQ_${timestamp}_${random}`;
    },

    // 处理文件选择
    handleFileSelect(files) {
        Array.from(files).forEach(file => {
            if (!this.selectedFiles.find(f => f.name === file.name && f.size === file.size)) {
                this.selectedFiles.push(file);
            }
        });
        this.updateFileList();
    },

    // 更新文件列表显示
    updateFileList() {
        const fileList = document.getElementById('fileList');
        fileList.innerHTML = '';

        this.selectedFiles.forEach((file, index) => {
            const fileItem = document.createElement('div');
            fileItem.className = 'file-item';
            fileItem.innerHTML = `
                <div class="file-info">
                    <div>${file.name}</div>
                    <div class="file-size">${this.formatFileSize(file.size)}</div>
                </div>
                <button class="remove-btn" onclick="TestBackdoor.removeFile(${index})">删除</button>
            `;
            fileList.appendChild(fileItem);
        });
    },

    // 移除文件
    removeFile(index) {
        this.selectedFiles.splice(index, 1);
        this.updateFileList();
    },

    // 格式化文件大小
    formatFileSize(bytes) {
        if (bytes === 0) return '0 Bytes';
        const k = 1024;
        const sizes = ['Bytes', 'KB', 'MB', 'GB'];
        const i = Math.floor(Math.log(bytes) / Math.log(k));
        return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
    },

    // 生成示例数据
    generateSampleData() {
        document.getElementById('userId').value = 'test_user_' + Date.now();
        document.getElementById('matterId').value = 'MATTER_' + Math.random().toString(36).substr(2, 8).toUpperCase();
        document.getElementById('matterName').value = '工程渣土堆运证核准';
        document.getElementById('channel').value = 'web';
        this.generateRequestId();

        // 模拟添加一些文件
        const sampleFiles = [
            { name: '营业执照.pdf', size: 1024000, type: 'application/pdf' },
            { name: '法人身份证.jpg', size: 512000, type: 'image/jpeg' },
            { name: '公司章程.pdf', size: 2048000, type: 'application/pdf' }
        ];

        // 创建模拟文件对象
        this.selectedFiles = sampleFiles.map(fileInfo => {
            const file = new File([''], fileInfo.name, { type: fileInfo.type });
            Object.defineProperty(file, 'size', { value: fileInfo.size });
            return file;
        });

        this.updateFileList();
        this.showToast('已生成示例数据', 'success');
    },

    // 清空所有数据
    clearAll() {
        document.getElementById('userId').value = '';
        document.getElementById('matterId').value = '';
        document.getElementById('matterName').value = '';
        document.getElementById('requestId').value = '';
        document.getElementById('channel').value = 'web';
        this.selectedFiles = [];
        this.updateFileList();
        this.hideResult();
        this.showToast('已清空所有数据', 'info');
    },

    // 提交预审数据（模拟第三方系统）
    async submitPreview() {
        try {
            this.showLoading(true);

            // 构建预审数据
            const previewData = this.buildPreviewData();
            
            this.showResult('发送预审请求...', JSON.stringify(previewData, null, 2));

            // 发送到预审接口
            const response = await fetch('/api/preview', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json'
                },
                body: JSON.stringify(previewData)
            });

            const result = await response.json();

            if (result.success) {
                this.showResult('✅ 预审提交成功', JSON.stringify(result, null, 2));
                this.showToast('预审数据提交成功！', 'success');
                
                // 如果有预览URL，可以打开预览
                if (result.data && result.data.previewUrl) {
                    setTimeout(() => {
                        if (confirm('是否打开预览文档？')) {
                            window.open(result.data.previewUrl, '_blank');
                        }
                    }, 1000);
                }
            } else {
                this.showResult('❌ 预审提交失败', JSON.stringify(result, null, 2));
                this.showToast('预审数据提交失败: ' + result.errorMsg, 'error');
            }

        } catch (error) {
            console.error('提交预审数据错误:', error);
            this.showResult('❌ 请求错误', error.message);
            this.showToast('提交失败: ' + error.message, 'error');
        } finally {
            this.showLoading(false);
        }
    },

    // 直接上传文件（OCR识别）
    async uploadFiles() {
        if (this.selectedFiles.length === 0) {
            this.showToast('请先选择文件', 'warning');
            return;
        }

        try {
            this.showLoading(true);
            const results = [];

            for (let file of this.selectedFiles) {
                if (file.size === 0) {
                    // 跳过模拟文件
                    results.push({
                        fileName: file.name,
                        status: 'skipped',
                        message: '模拟文件，跳过上传'
                    });
                    continue;
                }

                const formData = new FormData();
                formData.append('file', file);

                const response = await fetch('/api/upload', {
                    method: 'POST',
                    body: formData
                });

                const result = await response.json();
                results.push({
                    fileName: file.name,
                    status: result.success ? 'success' : 'error',
                    data: result.data,
                    error: result.errorMsg
                });
            }

            this.showResult('📤 文件上传结果', JSON.stringify(results, null, 2));
            this.showToast('文件上传完成', 'success');

        } catch (error) {
            console.error('上传文件错误:', error);
            this.showResult('❌ 上传错误', error.message);
            this.showToast('上传失败: ' + error.message, 'error');
        } finally {
            this.showLoading(false);
        }
    },

    // 构建预审数据
    buildPreviewData() {
        const userId = document.getElementById('userId').value;
        const matterId = document.getElementById('matterId').value;
        const matterName = document.getElementById('matterName').value;
        const requestId = document.getElementById('requestId').value;
        const channel = document.getElementById('channel').value;

        // 构建材料数据
        const materialData = this.selectedFiles.map((file, index) => ({
            code: `MATERIAL_${index + 1}`,
            attachmentList: [{
                attaName: file.name,
                attaUrl: `http://example.com/files/${file.name}`, // 模拟URL
                isCloudShare: false
            }]
        }));

        return {
            userId: userId,
            preview: {
                matterId: matterId,
                matterType: "行政许可",
                matterName: matterName,
                copy: false,
                channel: channel,
                requestId: requestId,
                sequenceNo: `SEQ_${Date.now()}`,
                formData: [],
                materialData: materialData,
                agentInfo: {
                    userId: userId,
                    certificateType: "身份证"
                },
                subjectInfo: {
                    userId: userId,
                    certificateType: "身份证"
                }
            }
        };
    },

    // 显示结果
    showResult(title, content) {
        const resultArea = document.getElementById('resultArea');
        const resultContent = document.getElementById('resultContent');
        
        resultContent.textContent = `${title}\n\n${content}`;
        resultArea.style.display = 'block';
        
        // 滚动到结果区域
        resultArea.scrollIntoView({ behavior: 'smooth' });
    },

    // 隐藏结果
    hideResult() {
        document.getElementById('resultArea').style.display = 'none';
    },

    // 显示加载状态
    showLoading(show) {
        // 简单的加载提示
        if (show) {
            this.showToast('处理中...', 'info');
        }
    },

    // 显示提示消息
    showToast(message, type = 'info') {
        // 创建toast元素
        const toast = document.createElement('div');
        toast.className = `toast toast-${type}`;
        toast.style.cssText = `
            position: fixed;
            top: 20px;
            right: 20px;
            padding: 12px 20px;
            border-radius: 5px;
            color: white;
            font-size: 14px;
            z-index: 10000;
            opacity: 0;
            transition: opacity 0.3s ease;
        `;

        // 设置背景色
        const colors = {
            success: '#27ae60',
            error: '#e74c3c',
            warning: '#f39c12',
            info: '#3498db'
        };
        toast.style.backgroundColor = colors[type] || colors.info;
        toast.textContent = message;

        document.body.appendChild(toast);

        // 显示动画
        setTimeout(() => {
            toast.style.opacity = '1';
        }, 100);

        // 自动隐藏
        setTimeout(() => {
            toast.style.opacity = '0';
            setTimeout(() => {
                document.body.removeChild(toast);
            }, 300);
        }, 3000);
    }
};

// 页面加载完成后初始化
document.addEventListener('DOMContentLoaded', () => {
    TestBackdoor.init();
});

// 全局快捷键支持
document.addEventListener('keydown', (e) => {
    if (e.ctrlKey && e.shiftKey && e.key === 'T') {
        e.preventDefault();
        window.location.href = '/static/test.html';
    }
});
