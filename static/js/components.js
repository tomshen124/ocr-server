// 组件管理模块
class ComponentManager {
    constructor() {
        this.components = {};
    }

    // 注册组件
    register(name, component) {
        this.components[name] = component;
    }

    // 获取组件
    get(name) {
        return this.components[name];
    }
}

// 基本信息组件
class BasicInfoComponent {
    constructor(container) {
        this.container = container;
    }

    render(data) {
        this.container.innerHTML = `
            <h3 class="card-title">基本信息</h3>
            <div class="info-item">
                <span class="info-label">申請人：</span>
                <span class="info-value">${data.applicant}</span>
            </div>
            <div class="info-item">
                <span class="info-label">申請類型：</span>
                <span class="info-value">${data.applicationType}</span>
            </div>
            <div class="info-item">
                <span class="info-label">審核機關：</span>
                <span class="info-value">${data.auditOrgan}</span>
            </div>
        `;
    }
}

// 材料列表组件
class MaterialsListComponent {
    constructor(container) {
        this.container = container;
        this.onItemClick = null;
        this.onDocumentClick = null;
    }

    render(materials) {
        this.container.innerHTML = materials.map(material => 
            this.createMaterialHTML(material)
        ).join('');
        
        this.bindEvents();
    }

    createMaterialHTML(material) {
        const statusClass = this.getStatusClass(material.status);
        
        return `
            <div class="material-item" data-material-id="${material.id}">
                <div class="material-header ${material.expanded ? 'expanded' : ''}" data-material-id="${material.id}">
                    <div class="material-title">
                        <span class="status-dot ${statusClass}"></span>
                        <span>${material.name}</span>
                        <span class="material-count">${material.count}</span>
                    </div>
                    <span class="expand-icon ${material.expanded ? 'expanded' : ''}">▼</span>
                </div>
                <div class="material-content ${material.expanded ? 'expanded' : ''}">
                    <div class="material-items">
                        ${material.items.map(item => this.createSubItemHTML(item)).join('')}
                    </div>
                </div>
            </div>
        `;
    }

    createSubItemHTML(item) {
        const statusClass = this.getStatusClass(item.status);
        const hasDocumentAttr = item.hasDocument ? 'true' : 'false';
        const documentTypeAttr = item.documentType || '';
        const documentIdAttr = item.documentId || '';
        const cursorStyle = item.hasDocument ? 'cursor: pointer;' : '';
        const hoverClass = item.hasDocument ? 'hover-effect' : '';

        return `
            <div class="material-sub-item ${hoverClass}" 
                 data-item-id="${item.id}"
                 data-has-document="${hasDocumentAttr}" 
                 data-document-type="${documentTypeAttr}"
                 data-document-id="${documentIdAttr}"
                 style="${cursorStyle}"
                 title="${item.checkPoint || ''}">
                <span class="status-dot ${statusClass}"></span>
                <span class="item-name">${item.name}</span>
                ${item.hasDocument ? '<span class="document-icon">📄</span>' : ''}
                ${item.checkPoint ? '<span class="check-point-icon" title="' + item.checkPoint + '">⚠️</span>' : ''}
            </div>
        `;
    }

    getStatusClass(status) {
        switch(status) {
            case 'passed': return 'passed';
            case 'warning': return 'warning';
            case 'error': return 'error';
            case 'hasIssues': return 'warning';
            default: return 'passed';
        }
    }

    bindEvents() {
        // 材料展开/收起
        this.container.addEventListener('click', (e) => {
            const header = e.target.closest('.material-header');
            if (header) {
                const materialId = parseInt(header.dataset.materialId);
                if (this.onItemClick) {
                    this.onItemClick(materialId);
                }
            }
        });

        // 文档点击
        this.container.addEventListener('click', (e) => {
            const subItem = e.target.closest('.material-sub-item');
            if (subItem && subItem.dataset.hasDocument === 'true') {
                const itemData = {
                    id: subItem.dataset.itemId,
                    documentId: subItem.dataset.documentId,
                    documentType: subItem.dataset.documentType,
                    name: subItem.querySelector('.item-name').textContent
                };
                if (this.onDocumentClick) {
                    this.onDocumentClick(itemData);
                }
            }
        });
    }

    // 设置事件回调
    setEventHandlers(onItemClick, onDocumentClick) {
        this.onItemClick = onItemClick;
        this.onDocumentClick = onDocumentClick;
    }
}

// 状态显示组件
class StatusDisplayComponent {
    constructor(container) {
        this.container = container;
    }

    render(status, message = '') {
        let content = '';
        
        switch(status) {
            case 'passed':
                content = this.createPassedStatus();
                break;
            case 'hasIssues':
                content = this.createIssuesStatus(message);
                break;
            case 'error':
                content = this.createErrorStatus(message);
                break;
            case 'loading':
                content = this.createLoadingStatus();
                break;
            default:
                content = this.createPassedStatus();
        }
        
        this.container.innerHTML = content;
    }

    createPassedStatus() {
        return `
            <div class="status-display">
                <div class="status-icon">
                    <div class="success-icon">
                        <svg width="120" height="120" viewBox="0 0 120 120">
                            <!-- 文档背景 -->
                            <rect x="25" y="15" width="70" height="90" fill="#E8F4FD" stroke="#4A90E2" stroke-width="2" rx="4"/>
                            <!-- 文档线条 -->
                            <line x1="35" y1="30" x2="75" y2="30" stroke="#4A90E2" stroke-width="2" opacity="0.6"/>
                            <line x1="35" y1="40" x2="85" y2="40" stroke="#4A90E2" stroke-width="2" opacity="0.6"/>
                            <line x1="35" y1="50" x2="80" y2="50" stroke="#4A90E2" stroke-width="2" opacity="0.6"/>
                            <line x1="35" y1="60" x2="75" y2="60" stroke="#4A90E2" stroke-width="2" opacity="0.6"/>
                            <!-- 勾号圆圈背景 -->
                            <circle cx="75" cy="75" r="20" fill="#52C41A" stroke="white" stroke-width="3"/>
                            <!-- 勾号 -->
                            <path d="M67 75l5 5 10-10" stroke="white" stroke-width="3" fill="none" stroke-linecap="round" stroke-linejoin="round"/>
                            <!-- 装饰元素 -->
                            <circle cx="35" cy="25" r="2" fill="#4A90E2" opacity="0.3"/>
                            <circle cx="45" cy="20" r="1.5" fill="#4A90E2" opacity="0.4"/>
                            <circle cx="90" cy="30" r="1" fill="#4A90E2" opacity="0.3"/>
                            <path d="M25 35l3 3 6-6" stroke="#4A90E2" stroke-width="1" fill="none" opacity="0.2"/>
                        </svg>
                    </div>
                </div>
                <div class="status-text">
                    <h3>智能预审通过，</h3>
                    <p>请适回信息确认人员继续操作。</p>
                </div>
            </div>
        `;
    }

    createIssuesStatus(message) {
        return `
            <div class="status-display">
                <div class="status-icon">
                    <div class="warning-icon">
                        <svg width="80" height="80" viewBox="0 0 80 80">
                            <circle cx="40" cy="40" r="35" fill="#fff7e6" stroke="#fa8c16" stroke-width="2"/>
                            <path d="M40 20l-3 25h6l-3-25z" fill="#fa8c16"/>
                            <circle cx="40" cy="55" r="3" fill="#fa8c16"/>
                        </svg>
                    </div>
                </div>
                <div class="status-text">
                    <h3>发现需要注意的问题</h3>
                    <p>${message || '请检查左侧标记的材料项目。'}</p>
                </div>
            </div>
        `;
    }

    createErrorStatus(message) {
        return `
            <div class="status-display">
                <div class="status-icon">
                    <div class="error-icon">
                        <svg width="120" height="120" viewBox="0 0 120 120">
                            <!-- 文档背景 -->
                            <rect x="25" y="15" width="70" height="90" fill="#FFF2F0" stroke="#F5222D" stroke-width="2" rx="4"/>
                            <!-- 文档线条 -->
                            <line x1="35" y1="30" x2="75" y2="30" stroke="#F5222D" stroke-width="2" opacity="0.6"/>
                            <line x1="35" y1="40" x2="85" y2="40" stroke="#F5222D" stroke-width="2" opacity="0.6"/>
                            <line x1="35" y1="50" x2="80" y2="50" stroke="#F5222D" stroke-width="2" opacity="0.6"/>
                            <line x1="35" y1="60" x2="75" y2="60" stroke="#F5222D" stroke-width="2" opacity="0.6"/>
                            <!-- 问号圆圈背景 -->
                            <circle cx="75" cy="75" r="20" fill="#1890FF" stroke="white" stroke-width="3"/>
                            <!-- 问号 -->
                            <text x="75" y="85" text-anchor="middle" fill="white" font-size="24" font-weight="bold" font-family="Arial, sans-serif">?</text>
                            <!-- 装饰元素 -->
                            <circle cx="35" cy="25" r="2" fill="#F5222D" opacity="0.3"/>
                            <circle cx="45" cy="20" r="1.5" fill="#F5222D" opacity="0.4"/>
                            <circle cx="90" cy="30" r="1" fill="#F5222D" opacity="0.3"/>
                            <path d="M25 35l3 3 6-6" stroke="#F5222D" stroke-width="1" fill="none" opacity="0.2"/>
                        </svg>
                    </div>
                </div>
                <div class="status-text">
                    <h3>智能预审开小差了</h3>
                    <p>点击“重试”可重新发起智能预审</p>
                    <button class="retry-btn" onclick="auditApp.setStatus('loading')">重试</button>
                </div>
            </div>
        `;
    }

    createLoadingStatus() {
        return `
            <div class="status-display">
                <div class="status-icon">
                    <div class="loading-spinner">
                        <svg width="80" height="80" viewBox="0 0 80 80">
                            <circle cx="40" cy="40" r="35" fill="none" stroke="#e8e8e8" stroke-width="4"/>
                            <circle cx="40" cy="40" r="35" fill="none" stroke="#4a90e2" stroke-width="4" 
                                    stroke-dasharray="164" stroke-dashoffset="41" stroke-linecap="round">
                                <animateTransform attributeName="transform" type="rotate" 
                                                dur="1s" repeatCount="indefinite" values="0 40 40;360 40 40"/>
                            </circle>
                        </svg>
                    </div>
                </div>
                <div class="status-text">
                    <h3>正在进行智能预审...</h3>
                    <p>请耐心等待，预计需要2-3分钟。</p>
                </div>
            </div>
        `;
    }
}

// 文档预览组件
class DocumentPreviewComponent {
    constructor(container) {
        this.container = container;
    }

    render(documentData) {
        const { type, name, content, url } = documentData;
        
        if (type === 'license') {
            this.renderLicense(name, content);
        } else if (type === 'table') {
            this.renderTable(name, content);
        } else if (type === 'image') {
            this.renderImage(name, url);
        } else {
            this.renderDefault(name);
        }
    }

    renderLicense(name, content) {
        this.container.innerHTML = `
            <div class="document-preview">
                <h3 class="preview-title">${name}</h3>
                <div class="license-preview">
                    <div class="license-header">
                        <div class="national-emblem">🇨🇳</div>
                        <h2>營業執照</h2>
                    </div>
                    <div class="license-content">
                        <div class="license-row">
                            <span class="label">统一社会信用代码：</span>
                            <span class="value">${content?.creditCode || '91330000XXXXXXXXXX'}</span>
                        </div>
                        <div class="license-row">
                            <span class="label">名稱：</span>
                            <span class="value">${content?.companyName || '浙江一二三科技有限公司'}</span>
                        </div>
                        <div class="license-row">
                            <span class="label">類型：</span>
                            <span class="value">${content?.companyType || '有限責任公司'}</span>
                        </div>
                        <div class="license-row">
                            <span class="label">法定代表人：</span>
                            <span class="value">${content?.legalPerson || '张三'}</span>
                        </div>
                        <div class="license-row">
                            <span class="label">注册资本：</span>
                            <span class="value">${content?.registeredCapital || '1000万元人民币'}</span>
                        </div>
                        <div class="license-row">
                            <span class="label">成立日期：</span>
                            <span class="value">${content?.establishDate || '2020年01月01日'}</span>
                        </div>
                        <div class="license-row">
                            <span class="label">營業期限：</span>
                            <span class="value">${content?.businessTerm || '2020年01月01日至長期'}</span>
                        </div>
                        <div class="license-row">
                            <span class="label">经营范围：</span>
                            <span class="value">${content?.businessScope || '软件开发；技术服务；技术转让；技术咨询'}</span>
                        </div>
                    </div>
                    <div class="license-footer">
                        <div class="official-seal">
                            <div class="seal-circle">
                                <span>工商行政管理局</span>
                            </div>
                        </div>
                        <div class="issue-info">
                            <p>發照日期：${content?.issueDate || '2020年01月01日'}</p>
                        </div>
                    </div>
                </div>
            </div>
        `;
    }

    renderTable(name, content) {
        this.container.innerHTML = `
            <div class="document-preview">
                <h3 class="preview-title">${name}</h3>
                <div class="table-preview">
                    <table class="preview-table">
                        <thead>
                            <tr>
                                <th>序號</th>
                                <th>事項</th>
                                <th>要求</th>
                                <th>份數</th>
                                <th>審核結果</th>
                                <th>備註</th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr>
                                <td>1</td>
                                <td>法人代表</td>
                                <td>签字</td>
                                <td>1</td>
                                <td><span class="status-passed">✓</span></td>
                                <td>已完成</td>
                            </tr>
                            <tr class="highlight-row">
                                <td>2</td>
                                <td>法人簽章</td>
                                <td>蓋章</td>
                                <td>1</td>
                                <td><span class="status-warning">⚠</span></td>
                                <td class="highlight-cell">簽章不清晰</td>
                            </tr>
                            <tr>
                                <td>3</td>
                                <td>企業法人簽章</td>
                                <td>蓋章</td>
                                <td>1</td>
                                <td><span class="status-passed">✓</span></td>
                                <td>已完成</td>
                            </tr>
                            <tr>
                                <td>4</td>
                                <td>企业法人签章副本</td>
                                <td>盖章</td>
                                <td>1</td>
                                <td><span class="status-passed">✓</span></td>
                                <td>已完成</td>
                            </tr>
                        </tbody>
                    </table>
                    <div class="table-summary">
                        <p><strong>总计：</strong>4项，通过3项，需要注意1项</p>
                        <p><strong>建议：</strong>请重新提供清晰的法人签章</p>
                    </div>
                </div>
            </div>
        `;
    }

    renderImage(name, url) {
        this.container.innerHTML = `
            <div class="document-preview">
                <h3 class="preview-title">${name}</h3>
                <div class="image-preview">
                    <img src="${url}" alt="${name}" style="max-width: 100%; max-height: 600px; border-radius: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.1);">
                </div>
            </div>
        `;
    }

    renderDefault(name) {
        this.container.innerHTML = `
            <div class="document-preview">
                <h3 class="preview-title">${name}</h3>
                <div class="default-preview">
                    <div class="document-icon-large">📄</div>
                    <p>文档预览功能开发中...</p>
                </div>
            </div>
        `;
    }
}

// 已通过材料组件
class PassedMaterialsComponent {
    constructor(container) {
        this.container = container;
    }

    render(materials) {
        this.container.innerHTML = `
            <h3 class="card-title">已通过材料</h3>
            ${materials.map((material, index) => `
                <div class="passed-item">
                    <span class="passed-number">${index + 1}.</span>
                    <span class="passed-name">${material.name}</span>
                </div>
            `).join('')}
        `;
    }
}

// 导出组件管理器和组件类
window.ComponentManager = ComponentManager;
window.BasicInfoComponent = BasicInfoComponent;
window.MaterialsListComponent = MaterialsListComponent;
window.StatusDisplayComponent = StatusDisplayComponent;
window.DocumentPreviewComponent = DocumentPreviewComponent;
window.PassedMaterialsComponent = PassedMaterialsComponent;