(() => {
class ComponentManager {
    constructor() {
        this.components = {};
    }

    register(name, component) {
        this.components[name] = component;
    }

    get(name) {
        return this.components[name];
    }
}

class BasicInfoComponent {
    constructor(container) {
        this.container = container;
    }

    render(data) {
        this.container.innerHTML = `
            <h3 class="card-title">基本信息</h3>
            <div class="info-item">
                <span class="info-label">申请人：</span>
                <span class="info-value">${data.applicant}</span>
            </div>
            <div class="info-item">
                <span class="info-label">申请类型：</span>
                <span class="info-value">${data.applicationType}</span>
            </div>
            <div class="info-item">
                <span class="info-label">审核机关：</span>
                <span class="info-value">${data.auditOrgan}</span>
            </div>
        `;
    }
}

class MaterialsListComponent {
    constructor(container) {
        this.container = container;
        this.onItemClick = null;
        this.onDocumentClick = null;
        this.materials = [];
        this.materialMap = new Map();
        this.eventsBound = false;
    }

    render(materials) {
        this.materials = Array.isArray(materials) ? materials : [];
        this.materialMap = new Map(
            this.materials.map(material => [String(material.id), material])
        );
        this.container.innerHTML = materials
            .map(material => this.createMaterialHTML(material))
            .join('');
        
        this.bindEvents();
    }

    createMaterialHTML(material) {
        const statusClass = this.getStatusClass(material.status);
        const materialId = this.escapeHtml(material.id ?? '');
        const materialName = this.escapeHtml(material.name ?? '');
        const materialCount = this.escapeHtml(
            material.count != null ? material.count : ''
        );
        const safeMaterialImage =
            this.escapeHtml(
                material.image?.status_icon ||
                    '/static/images/智能预审_审核依据材料1.3.png'
            );

        const hasPreview = Array.isArray(material.items)
            ? material.items.some(item => item.hasDocument)
            : false;
        
        return `
            <div class="material-item" data-material-id="${materialId}">
                <div class="material-header ${material.expanded ? 'expanded' : ''}" 
                     data-material-id="${materialId}"
                     data-has-preview="${hasPreview}">
                    <div class="material-title">
                        <span class="status-dot ${statusClass}"></span>
                        <img src="${safeMaterialImage}" alt="材料图标" class="material-icon" 
                             style="width: 20px; height: 20px; margin: 0 8px; vertical-align: middle;">
                        <span>${materialName}</span>
                        <span class="material-count">${materialCount}</span>
                        ${hasPreview ? '<span class="preview-icon" title="点击预览文档">👁️</span>' : ''}
                    </div>
                    <span class="expand-icon ${material.expanded ? 'expanded' : ''}">▼</span>
                </div>
                <div class="material-content ${material.expanded ? 'expanded' : ''}">
                    <div class="material-items">
                        ${material.items
                            .map(item => this.createSubItemHTML(material, item))
                            .join('')}
                    </div>
                </div>
            </div>
        `;
    }

    createSubItemHTML(material, item) {
        const statusClass = this.getStatusClass(item.status);
        const hasDocumentAttr = item.hasDocument ? 'true' : 'false';
        const documentTypeAttr = this.escapeHtml(item.documentType || '');
        const documentIdAttr = this.escapeHtml(item.documentId || '');
        const materialId = this.escapeHtml(material.id ?? '');
        const itemId = this.escapeHtml(item.id ?? '');
        const itemName = this.escapeHtml(item.name ?? '');
        const itemTitle = this.escapeHtml(item.checkPoint || '');
        const cursorStyle = item.hasDocument ? 'cursor: pointer;' : '';
        const hoverClass = item.hasDocument ? 'hover-effect' : '';

        return `
            <div class="material-sub-item ${hoverClass}" 
                 data-item-id="${itemId}"
                 data-material-id="${materialId}"
                 data-has-document="${hasDocumentAttr}" 
                 data-document-type="${documentTypeAttr}"
                 data-document-id="${documentIdAttr}"
                 style="${cursorStyle}"
                 title="${itemTitle}">
                <span class="status-dot ${statusClass}"></span>
                <span class="item-name">${itemName}</span>
                ${item.hasDocument ? '<span class="document-icon">📄</span>' : ''}
                ${
                    item.checkPoint
                        ? `<span class="check-point-icon" title="${itemTitle}">⚠️</span>`
                        : ''
                }
            </div>
        `;
    }

    escapeHtml(text) {
        if (text == null) {
            return '';
        }
        return String(text)
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#39;');
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
        if (this.eventsBound) {
            return;
        }
        this.eventsBound = true;

        this.container.addEventListener('click', (e) => {
            const header = e.target.closest('.material-header');
            if (header) {
                const previewTrigger = e.target.closest('.preview-icon');
                if (previewTrigger) {
                    const materialId = header.dataset.materialId;
                    const material = this.materialMap.get(String(materialId));
                    const firstItem = material?.items?.find(item => item.hasDocument);
                    if (material && firstItem && this.onDocumentClick) {
                        this.onDocumentClick(firstItem, material);
                    }
                    return;
                }

                const materialId = header.dataset.materialId;
                if (this.onItemClick) {
                    this.onItemClick(materialId);
                }
            }
        });

        this.container.addEventListener('click', (e) => {
            const subItem = e.target.closest('.material-sub-item');
            if (subItem && subItem.dataset.hasDocument === 'true') {
                const materialId = subItem.dataset.materialId;
                const itemId = subItem.dataset.itemId;
                const material = this.materialMap.get(String(materialId));
                const item = material?.items?.find(
                    entry => String(entry.id) === String(itemId)
                ) || null;

                if (item && this.onDocumentClick) {
                    this.onDocumentClick(item, material);
                }
            }
        });
    }

    setEventHandlers(onItemClick, onDocumentClick) {
        this.onItemClick = onItemClick;
        this.onDocumentClick = onDocumentClick;
    }
}

class StatusDisplayComponent {
    constructor(container) {
        this.container = container;
    }

    render(status, message = '') {
        let content = '';
        
        switch(status) {
            case 'summary':
                content = this.createSummaryStatus(message);
                break;
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
                            <rect x="25" y="15" width="70" height="90" fill="#E8F4FD" stroke="#4A90E2" stroke-width="2" rx="4"/>
                            <line x1="35" y1="30" x2="75" y2="30" stroke="#4A90E2" stroke-width="2" opacity="0.6"/>
                            <line x1="35" y1="40" x2="85" y2="40" stroke="#4A90E2" stroke-width="2" opacity="0.6"/>
                            <line x1="35" y1="50" x2="80" y2="50" stroke="#4A90E2" stroke-width="2" opacity="0.6"/>
                            <line x1="35" y1="60" x2="75" y2="60" stroke="#4A90E2" stroke-width="2" opacity="0.6"/>
                            <circle cx="75" cy="75" r="20" fill="#52C41A" stroke="white" stroke-width="3"/>
                            <path d="M67 75l5 5 10-10" stroke="white" stroke-width="3" fill="none" stroke-linecap="round" stroke-linejoin="round"/>
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
                            <rect x="25" y="15" width="70" height="90" fill="#FFF2F0" stroke="#F5222D" stroke-width="2" rx="4"/>
                            <line x1="35" y1="30" x2="75" y2="30" stroke="#F5222D" stroke-width="2" opacity="0.6"/>
                            <line x1="35" y1="40" x2="85" y2="40" stroke="#F5222D" stroke-width="2" opacity="0.6"/>
                            <line x1="35" y1="50" x2="80" y2="50" stroke="#F5222D" stroke-width="2" opacity="0.6"/>
                            <line x1="35" y1="60" x2="75" y2="60" stroke="#F5222D" stroke-width="2" opacity="0.6"/>
                            <circle cx="75" cy="75" r="20" fill="#1890FF" stroke="white" stroke-width="3"/>
                            <text x="75" y="85" text-anchor="middle" fill="white" font-size="24" font-weight="bold" font-family="Arial, sans-serif">?</text>
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

    createSummaryStatus(payload = {}) {
        const stats = payload.stats || {};
        const status = (payload.status || 'passed').toLowerCase();
        const message = payload.message || '';

        const total = stats.total ?? 0;
        const passed = stats.passed ?? 0;
        const warnings = stats.warnings ?? 0;
        const failed = stats.failed ?? 0;
        const pending = Math.max(stats.pending ?? (total - passed - warnings - failed), 0);
        const problemMaterials = Array.isArray(stats.problemMaterials)
            ? stats.problemMaterials.slice(0, 3)
            : [];

        const badge = this.resolveSummaryBadge(status, failed, warnings);

        const metrics = [
            this.renderSummaryMetric('总材料', total, '#4a90e2'),
            this.renderSummaryMetric('已通过', passed, '#52c41a'),
            this.renderSummaryMetric('需复核', warnings, '#fa8c16'),
            this.renderSummaryMetric('未通过', failed, '#f5222d')
        ];

        if (pending > 0) {
            metrics.push(this.renderSummaryMetric('待处理', pending, '#2f54eb'));
        }

        const issueItems = problemMaterials
            .map((material, index) => {
                const name = this.escapeHtml(material.name || `材料${index + 1}`);
                const statusLabel =
                    (material.status || '').toLowerCase() === 'error'
                        ? '<span style="color:#f5222d;font-weight:600;">未通过</span>'
                        : '<span style="color:#fa8c16;font-weight:600;">需复核</span>';
                return `<li style="margin-bottom:6px;">${statusLabel} - ${name}</li>`;
            })
            .join('');

        const issuesSection = issueItems
            ? `
            <div style="margin-top:12px;">
                <div style="font-weight:600;color:#fa8c16;font-size:13px;margin-bottom:4px;">
                    重点关注材料
                </div>
                <ul style="margin:0;padding-left:18px;font-size:13px;color:#555;">${issueItems}</ul>
            </div>`
            : '';

        return `
            <div class="status-display summary-display" style="padding:18px 22px;">
                <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:16px;flex-wrap:wrap;gap:8px;">
                    <span style="display:inline-flex;align-items:center;padding:4px 12px;border-radius:16px;font-weight:600;background:${badge.background};color:${badge.color};font-size:13px;">
                        ${badge.text}
                    </span>
                    <span style="color:#555;font-size:14px;">${this.escapeHtml(message)}</span>
                </div>
                <div style="display:flex;gap:12px;flex-wrap:wrap;margin-bottom:12px;">
                    ${metrics.join('')}
                </div>
                <div style="font-size:12px;color:#999;">
                    统计口径：系统已收录的预审材料数量，供运维人员快速掌握总体情况。
                </div>
                ${issuesSection}
            </div>
        `;
    }

    renderSummaryMetric(label, value, themeColor) {
        return `
            <div style="flex:1;min-width:120px;background:#f8f9fb;border-radius:10px;padding:12px 14px;">
                <div style="font-size:12px;color:#888;margin-bottom:4px;">${this.escapeHtml(label)}</div>
                <div style="font-size:22px;font-weight:600;color:${themeColor};line-height:1;">${value}</div>
            </div>
        `;
    }

    resolveSummaryBadge(status, failed, warnings) {
        if (failed > 0 || status === 'error') {
            return {
                text: '存在未通过材料',
                background: '#fff1f0',
                color: '#f5222d'
            };
        }

        if (warnings > 0 || status === 'hasissues') {
            return {
                text: '需人工复核',
                background: '#fff7e6',
                color: '#fa8c16'
            };
        }

        return {
            text: '预审通过',
            background: '#f6ffed',
            color: '#52c41a'
        };
    }

    escapeHtml(text) {
        if (text == null) {
            return '';
        }
        return String(text)
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#39;');
    }
}

class DocumentPreviewComponent {
    constructor(container) {
        this.container = container;
        this.currentDocument = null;
        this.currentPage = 1;
    }

    render(documentData = {}) {
        const pages = this.normalizePages(documentData.pages);
        this.currentDocument = {
            ...documentData,
            pages,
        };
        this.currentPage = 1;
        this.renderCurrentDocument();
    }

    renderCurrentDocument() {
        if (!this.currentDocument) {
            this.container.innerHTML = '';
            return;
        }

        const {
            type = 'file',
            name = '材料预览',
            content = '',
            url = '',
            mimeType = '',
            meta = {},
            message = '',
            pages = [],
        } = this.currentDocument;

        const normalizedType = (type || 'file').toLowerCase();
        const hasMultiImages =
            pages.length > 1 &&
            (normalizedType === 'image' ||
                normalizedType === 'pdf' ||
                normalizedType === 'file');

        if (hasMultiImages) {
            this.renderImageWithPagination(name, pages, meta);
            return;
        }

        switch (normalizedType) {
            case 'license':
                this.renderLicense(name, content, meta);
                break;
            case 'table':
                this.renderTable(name, content, meta);
                break;
            case 'image':
                this.renderImage(name, pages[0] || url, meta);
                break;
            case 'pdf':
                if (pages.length === 1) {
                    this.renderImage(name, pages[0], meta);
                } else {
                    this.renderPdf(name, url, meta);
                }
                break;
            case 'text':
                this.renderText(name, content, meta);
                break;
            case 'empty':
                this.renderEmpty(name, message || content, meta);
                break;
            default:
                this.renderFile(name, url, mimeType, meta);
        }
    }

    renderLicense(name, content, meta = {}) {
        const license = {
            creditCode: this.escapeHtml(content?.creditCode || '91330000XXXXXXXXXX'),
            companyName: this.escapeHtml(content?.companyName || '示例企业有限公司'),
            companyType: this.escapeHtml(content?.companyType || '有限责任公司'),
            legalPerson: this.escapeHtml(content?.legalPerson || '张三'),
            registeredCapital: this.escapeHtml(content?.registeredCapital || '1000万元人民币'),
            establishDate: this.escapeHtml(content?.establishDate || '2020年01月01日'),
            businessTerm: this.escapeHtml(content?.businessTerm || '2020年01月01日至长期'),
            businessScope: this.escapeHtml(content?.businessScope || '软件开发；技术服务；技术转让；技术咨询'),
            issueDate: this.escapeHtml(content?.issueDate || '2020年01月01日')
        };

        this.container.innerHTML = `
            <div class="document-preview">
                <h3 class="preview-title">${name}</h3>
                <div class="license-preview">
                    <div class="license-header">
                        <div class="national-emblem">🇨🇳</div>
                        <h2>营业执照</h2>
                    </div>
                    <div class="license-content">
                        <div class="license-row">
                            <span class="label">统一社会信用代码：</span>
                            <span class="value">${license.creditCode}</span>
                        </div>
                        <div class="license-row">
                            <span class="label">名称：</span>
                            <span class="value">${license.companyName}</span>
                        </div>
                        <div class="license-row">
                            <span class="label">类型：</span>
                            <span class="value">${license.companyType}</span>
                        </div>
                        <div class="license-row">
                            <span class="label">法定代表人：</span>
                            <span class="value">${license.legalPerson}</span>
                        </div>
                        <div class="license-row">
                            <span class="label">注册资本：</span>
                            <span class="value">${license.registeredCapital}</span>
                        </div>
                        <div class="license-row">
                            <span class="label">成立日期：</span>
                            <span class="value">${license.establishDate}</span>
                        </div>
                        <div class="license-row">
                            <span class="label">营业期限：</span>
                            <span class="value">${license.businessTerm}</span>
                        </div>
                        <div class="license-row">
                            <span class="label">经营范围：</span>
                            <span class="value">${license.businessScope}</span>
                        </div>
                    </div>
                    <div class="license-footer">
                        <div class="official-seal">
                            <div class="seal-circle">
                                <span>工商行政管理局</span>
                            </div>
                        </div>
                        <div class="issue-info">
                            <p>发照日期：${license.issueDate}</p>
                        </div>
                    </div>
                </div>
                ${this.renderMeta(meta)}
            </div>
        `;
    }

    renderTable(name, content, meta = {}) {
        this.container.innerHTML = `
            <div class="document-preview">
                <h3 class="preview-title">${name}</h3>
                <div class="table-preview">
                    <table class="preview-table">
                        <thead>
                            <tr>
                                <th>序号</th>
                                <th>事项</th>
                                <th>要求</th>
                                <th>份数</th>
                                <th>审核结果</th>
                                <th>备注</th>
                            </tr>
                        </thead>
                        <tbody>
                            ${(content?.rows || []).map((row, index) => `
                                <tr>
                                    <td>${index + 1}</td>
                                    <td>${this.escapeHtml(row.item || '')}</td>
                                    <td>${this.escapeHtml(row.requirement || '')}</td>
                                    <td>${row.count || '-'}</td>
                                    <td><span class="${this.getStatusClass(row.status)}">${row.status === 'passed' ? '✓' : '⚠'}</span></td>
                                    <td>${this.escapeHtml(row.note || '')}</td>
                                </tr>
                            `).join('') || this.renderSampleTableRows()}
                        </tbody>
                    </table>
                    <div class="table-summary">
                        <p><strong>总计：</strong>${content?.summary || '系统自动对关键材料进行了详细比对。'}</p>
                    </div>
                </div>
                ${this.renderMeta(meta)}
            </div>
        `;
    }

    renderImage(name, url, meta = {}) {
        const downloadUrl = meta.downloadUrl || url;
        this.container.innerHTML = `
            <div class="document-preview">
                <h3 class="preview-title">${name}</h3>
                <div class="image-preview">
                    ${url ? `<img src="${url}" alt="${name}" style="max-width: 100%; max-height: 600px; border-radius: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.1);">` : '<div class="preview-placeholder">暂无图片预览</div>'}
                </div>
                ${this.renderMeta(meta)}
                ${downloadUrl ? this.renderDownloadActions(downloadUrl) : ''}
            </div>
        `;
    }

    renderImageWithPagination(name, pages, meta = {}) {
        const total = pages.length;
        const current = Math.min(Math.max(this.currentPage, 1), total);
        this.currentPage = current;
        const currentUrl = pages[current - 1];
        const downloadUrl = (meta && meta.downloadUrl) || currentUrl;
        const mergedMeta = { ...meta, pageCount: meta.pageCount || total };

        this.container.innerHTML = `
            <div class="document-preview">
                <h3 class="preview-title">${name}</h3>
                <div class="image-preview">
                    ${currentUrl ? `<img src="${currentUrl}" alt="${name}" style="max-width: 100%; max-height: 600px; border-radius: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.1);">` : '<div class="preview-placeholder">暂无图片预览</div>'}
                </div>
                <div class="page-controls" style="display: flex; align-items: center; justify-content: center; gap: 12px; margin-top: 12px;">
                    <button class="btn btn-outline" data-action="prev-page" style="padding: 6px 12px; border: 1px solid #d1d5db; border-radius: 4px; background: white;">上一页</button>
                    <span style="font-size: 14px; color: #1f2937;">第 ${current} / ${total} 页</span>
                    <button class="btn btn-outline" data-action="next-page" style="padding: 6px 12px; border: 1px solid #d1d5db; border-radius: 4px; background: white;">下一页</button>
                </div>
                ${this.renderMeta(mergedMeta)}
                ${downloadUrl ? this.renderDownloadActions(downloadUrl) : ''}
            </div>
        `;

        this.bindPaginationEvents(total);
    }

    renderPdf(name, url, meta = {}) {
        const downloadUrl = meta.downloadUrl || url;
        this.container.innerHTML = `
            <div class="document-preview">
                <h3 class="preview-title">${name}</h3>
                <div class="pdf-preview">
                    ${url ? `<iframe src="${url}" title="${name}" style="width: 100%; min-height: 620px; border: 1px solid #e5e7eb; border-radius: 6px;"></iframe>` : '<div class="preview-placeholder">暂无PDF预览</div>'}
                </div>
                ${this.renderMeta(meta)}
                ${downloadUrl ? this.renderDownloadActions(downloadUrl) : ''}
            </div>
        `;
    }

    renderText(name, content, meta = {}) {
        this.container.innerHTML = `
            <div class="document-preview">
                <h3 class="preview-title">${name}</h3>
                <div class="text-preview">
                    <pre style="white-space: pre-wrap; line-height: 1.6; background: #f7f9fc; border: 1px solid #e5e7eb; border-radius: 6px; padding: 12px; max-height: 520px; overflow: auto;">${this.escapeHtml(content || '暂无识别文本')}</pre>
                </div>
                ${this.renderMeta(meta)}
            </div>
        `;
    }

    renderFile(name, url, mimeType, meta = {}) {
        const description = mimeType ? `文件类型：${mimeType}` : '该文件可以下载后在本地查看。';
        const downloadUrl = meta.downloadUrl || url;
        this.container.innerHTML = `
            <div class="document-preview">
                <h3 class="preview-title">${name}</h3>
                <div class="default-preview">
                    <div class="document-icon-large">📄</div>
                    <p>${this.escapeHtml(description)}</p>
                </div>
                ${this.renderMeta(meta)}
                ${downloadUrl ? this.renderDownloadActions(downloadUrl) : ''}
            </div>
        `;
    }

    renderEmpty(name, message, meta = {}) {
        this.container.innerHTML = `
            <div class="document-preview">
                <h3 class="preview-title">${name}</h3>
                <div class="default-preview">
                    <div class="document-icon-large">ℹ️</div>
                    <p>${this.escapeHtml(message || '暂无可预览内容')}</p>
                </div>
                ${this.renderMeta(meta)}
            </div>
        `;
    }

    renderMeta(meta = {}) {
        const rows = [];

        if (meta.materialName) {
            rows.push({ label: '所属材料', value: meta.materialName });
        }
        if (meta.materialCode) {
            rows.push({ label: '材料编码', value: meta.materialCode });
        }
        if (meta.fileSize != null) {
            rows.push({ label: '文件大小', value: this.formatFileSize(meta.fileSize) });
        }
        if (meta.pageCount != null) {
            rows.push({ label: '页数', value: `${meta.pageCount}` });
        }
        if (meta.ocrSuccess === false) {
            rows.push({ label: 'OCR识别', value: '识别失败' });
        } else if (meta.ocrSuccess === true) {
            rows.push({ label: 'OCR识别', value: '已完成' });
        }
        if (meta.isCloudShare) {
            rows.push({ label: '来源', value: '云共享附件' });
        }
        if (meta.note) {
            rows.push({ label: '备注提示', value: meta.note });
        }

        if (!rows.length) {
            return '';
        }

        return `
            <div class="document-meta" style="margin-top: 16px; padding: 12px; background: #f7f9fc; border: 1px solid #e5e7eb; border-radius: 6px;">
                ${rows
                    .map(
                        (row) => `
                            <div class="meta-row" style="display: flex; justify-content: space-between; margin-bottom: 6px; font-size: 14px;">
                                <span class="meta-label" style="color: #666;">${this.escapeHtml(row.label)}：</span>
                                <span class="meta-value" style="color: #1f2937;">${this.escapeHtml(row.value)}</span>
                            </div>
                        `,
                    )
                    .join('')}
            </div>
        `;
    }

    renderDownloadActions(url) {
        return `
            <div class="download-actions">
                <a class="btn btn-outline" href="${url}" target="_blank" rel="noopener noreferrer"
                   style="display: inline-block; padding: 8px 16px; border: 1px solid #4a90e2; border-radius: 4px; color: #4a90e2; text-decoration: none; margin-top: 12px;">
                    下载原件
                </a>
            </div>
        `;
    }

    renderSampleTableRows() {
        return `
            <tr>
                <td>1</td>
                <td>申请表</td>
                <td>完整填写并签字盖章</td>
                <td>1</td>
                <td><span class="status-passed">✓</span></td>
                <td>系统示例</td>
            </tr>
        `;
    }

    formatFileSize(bytes) {
        if (bytes == null) {
            return '-';
        }
        const size = Number(bytes);
        if (!Number.isFinite(size) || size <= 0) {
            return '-';
        }
        const units = ['B', 'KB', 'MB', 'GB'];
        let index = 0;
        let value = size;
        while (value >= 1024 && index < units.length - 1) {
            value /= 1024;
            index += 1;
        }
        return `${value.toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
    }

    escapeHtml(text) {
        if (text == null) {
            return '';
        }
        return String(text)
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#39;');
    }

    getStatusClass(status) {
        switch (status) {
            case 'error':
                return 'status-error';
            case 'hasIssues':
            case 'warning':
                return 'status-warning';
            default:
                return 'status-passed';
        }
    }

    normalizePages(pages) {
        if (!Array.isArray(pages)) {
            return [];
        }
        const normalized = pages
            .map((page) => {
                if (!page) return '';
                if (typeof page === 'string') {
                    return page.trim();
                }
                if (typeof page === 'object') {
                    return (
                        page.url ||
                        page.preview_url ||
                        page.previewUrl ||
                        page.src ||
                        ''
                    );
                }
                return '';
            })
            .filter(Boolean);
        return Array.from(new Set(normalized));
    }

    bindPaginationEvents(total) {
        const prevBtn = this.container.querySelector('[data-action="prev-page"]');
        const nextBtn = this.container.querySelector('[data-action="next-page"]');

        prevBtn?.addEventListener('click', () => {
            this.goToPage(this.currentPage - 1, total);
        });

        nextBtn?.addEventListener('click', () => {
            this.goToPage(this.currentPage + 1, total);
        });

        if (prevBtn) {
            prevBtn.disabled = this.currentPage <= 1;
            prevBtn.style.opacity = prevBtn.disabled ? '0.5' : '1';
            prevBtn.style.cursor = prevBtn.disabled ? 'not-allowed' : 'pointer';
        }
        if (nextBtn) {
            nextBtn.disabled = this.currentPage >= total;
            nextBtn.style.opacity = nextBtn.disabled ? '0.5' : '1';
            nextBtn.style.cursor = nextBtn.disabled ? 'not-allowed' : 'pointer';
        }
    }

    goToPage(page, total) {
        const target = Math.min(Math.max(page, 1), total);
        if (target === this.currentPage) {
            return;
        }
        this.currentPage = target;
        this.renderCurrentDocument();
    }
}

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

window.ComponentManager = ComponentManager;
window.BasicInfoComponent = BasicInfoComponent;
window.MaterialsListComponent = MaterialsListComponent;
window.StatusDisplayComponent = StatusDisplayComponent;
window.DocumentPreviewComponent = DocumentPreviewComponent;
window.PassedMaterialsComponent = PassedMaterialsComponent;
})();
