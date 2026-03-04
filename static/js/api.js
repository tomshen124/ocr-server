(() => {
class ApiService {
    constructor() {
        this.config = window.CONFIG || {};
        this.baseUrl = this.config.API_CONFIG?.baseUrl || '/api';
        this.timeout = this.config.API_CONFIG?.timeout || 30000;
        this.utils = this.config.ConfigUtils || {};
    }

    getMonitorSessionId() {
        try {
            return (
                window.sessionStorage?.getItem('monitor_session_id') ||
                window.localStorage?.getItem('monitor_session_id') ||
                null
            );
        } catch (error) {
            console.warn('读取监控会话ID失败:', error);
            return null;
        }
    }

    appendMonitorSessionParam(url) {
        const sessionId = this.getMonitorSessionId();
        if (!sessionId || !url) {
            return url;
        }

        const isAbsolute = /^https?:\/\//i.test(url);

        try {
            const parsed = new URL(url, window.location.origin);
            const sameOrigin = parsed.origin === window.location.origin;
            const targetMatchesBase =
                !isAbsolute &&
                (this.baseUrl.startsWith('/') ||
                    parsed.origin === window.location.origin);

            if (sameOrigin || targetMatchesBase) {
                if (!parsed.searchParams.has('monitor_session_id')) {
                    parsed.searchParams.set(
                        'monitor_session_id',
                        sessionId,
                    );
                }
                if (parsed.origin === window.location.origin) {
                    return `${parsed.pathname}${parsed.search}${parsed.hash}`;
                }
                return parsed.toString();
            }
        } catch (error) {
            console.warn('附加监控会话参数失败:', error);
            if (!isAbsolute) {
                const connector = url.includes('?') ? '&' : '?';
                return `${url}${connector}monitor_session_id=${encodeURIComponent(sessionId)}`;
            }
        }

        return url;
    }

    async request(url, options = {}) {
        const config = {
            method: 'GET',
            headers: {
                'Content-Type': 'application/json',
                ...options.headers
            },
            credentials: 'include',
            timeout: this.timeout,
            ...options
        };

        try {
            const needPrefix = !(url.startsWith(this.baseUrl) || url.startsWith('http://') || url.startsWith('https://'));
            const requestUrl = needPrefix ? `${this.baseUrl}${url}` : url;
            const finalUrl = this.appendMonitorSessionParam(requestUrl);
            const response = await fetch(finalUrl, config);

            const contentType = response.headers.get('content-type') || '';
            const isJson = contentType.includes('application/json');
            const parsed = isJson ? await response.json().catch(() => null) : null;

            if (!response.ok) {
                if (response.status === 401) {
                    const redirectUrl = parsed?.redirect || parsed?.sso_url || '/api/sso/login';
                    this.showBackendAuthPrompt();
                    setTimeout(() => {
                        window.location.href = redirectUrl;
                    }, 1000);
                    return { success: false, need_auth: true, data: null, message: '正在跳转到用户认证...' };
                }
                return { success: false, data: parsed, message: `HTTP ${response.status}` };
            }

            if (parsed && parsed.need_auth && parsed.sso_url) {
                console.log('🔐 后端检测到需要用户认证，准备跳转认证页面');
                this.showBackendAuthPrompt();
                setTimeout(() => {
                    window.location.href = parsed.sso_url;
                }, 1000);
                return { success: false, need_auth: true, data: null, message: '正在跳转到用户认证...' };
            }

            return { success: true, data: parsed ?? null, message: 'success' };
        } catch (error) {
            console.error('API request failed:', error);
            return { success: false, data: null, message: error.message };
        }
    }

    showBackendAuthPrompt() {
        const existingPrompt = document.getElementById('backend-auth-prompt');
        if (existingPrompt) {
            existingPrompt.remove();
        }
        
        const promptDiv = document.createElement('div');
        promptDiv.id = 'backend-auth-prompt';
        promptDiv.style.cssText = `
            position: fixed;
            top: 50%;
            left: 50%;
            transform: translate(-50%, -50%);
            background: rgba(255, 255, 255, 0.95);
            padding: 30px 40px;
            border-radius: 8px;
            box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
            z-index: 10002;
            text-align: center;
            font-family: 'Microsoft YaHei', sans-serif;
            min-width: 320px;
            border-left: 4px solid #4a90e2;
        `;
        
        promptDiv.innerHTML = `
            <div style="font-size: 18px; color: #333; margin-bottom: 15px;">
                🔐 系统安全验证
            </div>
            <div style="font-size: 14px; color: #666; margin-bottom: 20px;">
                系统检测到您需要进行身份验证才能继续操作
            </div>
            <div style="font-size: 12px; color: #999; margin-bottom: 15px;">
                系统正在完成身份验证，请稍候...
            </div>
            <div style="margin-top: 15px;">
                <div style="width: 30px; height: 30px; border: 3px solid #f3f3f3; border-top: 3px solid #4a90e2; border-radius: 50%; animation: spin 1s linear infinite; margin: 0 auto;"></div>
            </div>
        `;
        
        if (!document.getElementById('backend-auth-spinner-style')) {
            const style = document.createElement('style');
            style.id = 'backend-auth-spinner-style';
            style.textContent = `
                @keyframes spin {
                    0% { transform: rotate(0deg); }
                    100% { transform: rotate(360deg); }
                }
            `;
            document.head.appendChild(style);
        }
        
        document.body.appendChild(promptDiv);
        console.log('📢 显示后端认证提示');
    }

    async getBasicInfo(previewId) {
        const url = this.utils.getApiUrl ? 
            this.utils.getApiUrl('previewData', { previewId }) : 
            `/preview/data/${previewId}`;
        return await this.request(url);
    }

    async getMaterialsList(previewId) {
        const url = this.utils.getApiUrl ? 
            this.utils.getApiUrl('previewData', { previewId }) : 
            `/preview/data/${previewId}`;
        return await this.request(url);
    }

    async getAuditStatus(previewId) {
        const url = this.utils.getApiUrl ? 
            this.utils.getApiUrl('previewData', { previewId }) : 
            `/preview/data/${previewId}`;
        return await this.request(url);
    }

    async startAudit(requestData) {
        return await this.request(`/preview`, {
            method: 'POST',
            body: JSON.stringify(requestData)
        });
    }

    async getAuditProgress(previewId) {
        return await this.request(`/preview/data/${previewId}`);
    }

    async getDocumentPreview(previewId, options = {}) {
        if (typeof options === 'number') {
            return this.getDocumentPreviewUrl(previewId, options);
        }
        if (
            options &&
            typeof options === 'object' &&
            options.pageIndex !== undefined
        ) {
            return this.getDocumentPreviewUrl(previewId, options.pageIndex);
        }
        return await this.request(`/preview/data/${previewId}`);
    }

    async exportMaterials(previewId) {
        const url = `${this.baseUrl}/preview/download/${encodeURIComponent(previewId)}?format=pdf`;
        window.open(url, '_blank');
        return { success: true, message: '下载已开始' };
    }

    async downloadCheckList(previewId) {
        const url = `${this.baseUrl}/preview/download/${encodeURIComponent(previewId)}?format=pdf`;
        window.open(url, '_blank');
        return { success: true, message: '下载已开始' };
    }

    async getFrontendConfig() {
        return await this.request('/config/frontend');
    }

    async checkAuthStatus() {
        return await this.request('/auth/status');
    }

    async debugTicketAuth(ticketId = 'debug_tk_e4a0dc3fcc8d464ba336b9bcb1ba2072') {
        return await this.request('/verify_user', {
            method: 'POST',
            body: JSON.stringify({ ticketId })
        });
    }

    async getQueueStatus() {
        return await this.request('/queue/status');
    }

    getMaterialImage(previewId, materialName) {
        const url = this.utils.getApiUrl ? 
            this.utils.getApiUrl('materialPreview', { previewId, materialName }) : 
            `/files/material-preview/${previewId}/${materialName}`;
        return url;
    }

    getOcrImage(pdfName, pageIndex) {
        const url = this.utils.getApiUrl ? 
            this.utils.getApiUrl('ocrImage', { pdfName, pageIndex }) : 
            `/files/ocr-image/${pdfName}/${pageIndex}`;
        return url;
    }

    getPreviewThumbnail(previewId, pageIndex) {
        const url = this.utils.getApiUrl ? 
            this.utils.getApiUrl('previewThumbnail', { previewId, pageIndex }) : 
            `/files/preview-thumbnail/${previewId}/${pageIndex}`;
        return url;
    }

    getDocumentPreviewUrl(previewId, pageIndex = null) {
        if (pageIndex !== null) {
            return this.getPreviewThumbnail(previewId, pageIndex);
        }
        const encodedId = encodeURIComponent(previewId ?? '');
        return `/preview/document/${encodedId}`;
    }

    transformPreviewData(backendData) {
        if (!backendData || !backendData.success) {
            return null;
        }

        const payload = backendData.data;
        if (!payload) {
            console.warn('预审接口返回空数据结构:', backendData);
            return null;
        }

        const data =
            typeof payload === 'object' && payload !== null
                ? payload.data || payload.record || payload
                : payload;

        if (!data || (payload.success === false && !payload.data)) {
            const message =
                payload.errorMsg ||
                payload.message ||
                '预审数据载荷为空或解析失败';
            console.warn('预审数据载荷异常:', payload);
            return {
                basicInfo: {
                    applicant: '申请人',
                    applicationType: '业务类型',
                    auditOrgan: '智能预审系统',
                },
                materials: [],
                passedMaterials: [],
                auditStatus: {
                    status: 'error',
                    result: 'error',
                    progress: 0,
                    estimatedTime: 0,
                    message,
                },
                files: {},
            };
        }

        console.log('转换后端数据:', { payload, data });

        let evaluationResult =
            data.evaluation_result || data.evaluationResult || null;
        evaluationResult = this.normalizeEvaluationResult(evaluationResult);

        const basicInfoSource = evaluationResult?.basic_info || {};
        const basicInfo = {
            applicant:
                basicInfoSource.applicant_name ||
                data.applicant_name ||
                data.applicant ||
                this.extractFormValue(data, 'legalRep.FDDBR') ||
                this.extractFormValue(data, 'self.DWMC') ||
                '申请人',
            applicationType:
                basicInfoSource.matter_name || data.matter_name || '业务类型',
            auditOrgan:
                basicInfoSource.theme_name || data.theme_name || '智能预审系统',
        };

        const materials = [];

        if (Array.isArray(data.materials) && data.materials.length) {
            data.materials.forEach((material, index) => {
                const normalizedImage = this.normalizeMaterialImage(
                    material.image || {},
                );
                materials.push({
                    id: material.id || `material_${index + 1}`,
                    name: material.name || `材料${index + 1}`,
                    count: material.count || material.pages || 1,
                    status: this.mapStatus(material.status),
                    expanded: false,
                    image: normalizedImage,
                    items: material.items || this.buildMaterialItems(material),
                });
            });
        } else if (
            Array.isArray(evaluationResult?.material_results) &&
            evaluationResult.material_results.length
        ) {
            evaluationResult.material_results.forEach((material, index) => {
                const materialId =
                    material.material_code || `material_${index + 1}`;
                const materialStatus = this.mapEvaluationStatus(
                    material.rule_evaluation,
                    material.processing_status,
                    material.evaluation_status,
                );

                const attachmentItems = (material.attachments || []).map(
                    (attachment, attachmentIndex) =>
                        this.buildAttachmentItem(
                            material,
                            attachment,
                            attachmentIndex,
                        ),
                );

                if (material.ocr_content) {
                    attachmentItems.push(
                        this.buildOcrTextItem(material, attachmentItems.length),
                    );
                }

                const items =
                    attachmentItems.length > 0
                        ? attachmentItems
                        : [
                              this.buildPlaceholderItem(
                                  material,
                                  materialStatus,
                              ),
                          ];

                materials.push({
                    id: materialId,
                    name:
                        material.material_name ||
                        material.material_code ||
                        `材料${index + 1}`,
                    materialCode: material.material_code,
                    count: items.length,
                    status: materialStatus,
                    expanded: false,
                    image: this.buildMaterialImage(material, items),
                    items,
                });
            });
        } else if (Array.isArray(evaluationResult?.rules)) {
            evaluationResult.rules.forEach((rule, index) => {
                const ruleStatus = this.mapLegacyRuleStatus(rule.result);
                materials.push({
                    id: `legacy_rule_${index + 1}`,
                    name: rule.description || rule.field || `检查项${index + 1}`,
                    count: 1,
                    status: ruleStatus,
                    expanded: false,
                    items: [
                        {
                            id: `legacy_rule_item_${index + 1}`,
                            name: rule.field || rule.description,
                            status: ruleStatus,
                            hasDocument: !!rule.ocr_text,
                            documentType: 'text',
                            documentContent: rule.ocr_text,
                            checkPoint:
                                rule.message || rule.details || '检查完成',
                        },
                    ],
                });
            });
        }

        const passedMaterials = materials
            .filter((material) => material.status === 'passed')
            .map((material, index) => ({
                id: index + 1,
                name: material.name,
            }));

        const overallResult = this.determineOverallResult(
            evaluationResult,
            materials,
        );

        const issuesCount = materials.filter((material) => material.status !== 'passed').length;

        const auditStatus = {
            status: 'completed',
            result: overallResult.result,
            progress: overallResult.progress,
            estimatedTime: 0,
            message: this.generateStatusMessage(
                overallResult.result,
                issuesCount,
            ),
        };

        const statusRaw = (data.status || data.latest_status || '').toString().toLowerCase();
        const evaluationMissing =
            !evaluationResult ||
            (!materials.length &&
                !(Array.isArray(data.materials) && data.materials.length));
        if (evaluationMissing && statusRaw === 'completed') {
            auditStatus.status = 'completed';
            auditStatus.result = overallResult.result || 'completed';
            auditStatus.progress = auditStatus.progress || 100;
            auditStatus.message = '预审已完成但报告尚未生成，稍后刷新或联系管理员';
        }

        return {
            basicInfo,
            materials,
            passedMaterials,
            auditStatus,
            files: data.files || {}
        };
    }

    buildMaterialItems(material) {
        if (material.items && Array.isArray(material.items)) {
            return material.items.map((item, index) => {
                const documentUrl = this.normalizeUrl(
                    item.documentUrl || item.document_url || '',
                );
                const documentThumbnail = this.normalizeUrl(
                    item.documentThumbnail ||
                        item.document_thumbnail ||
                        documentUrl,
                );
                const downloadUrl = this.normalizeUrl(
                    item.downloadUrl ||
                        item.download_url ||
                        item.documentUrl ||
                        item.document_url ||
                        documentUrl,
                );
                const documentPages = (
                    Array.isArray(item.documentPages)
                        ? item.documentPages
                        : Array.isArray(item.document_pages)
                        ? item.document_pages
                        : []
                ).map((page) => this.normalizeUrl(page));

                return {
                    id: item.id || index + 1,
                    name: item.name || `检查项${index + 1}`,
                    status: this.mapStatus(item.status),
                    hasDocument: item.hasDocument || !!documentUrl,
                    documentType: item.documentType || item.document_type || 'file',
                    documentUrl,
                    documentThumbnail,
                    documentMime: item.documentMime || item.document_mime || '',
                    documentId: item.documentId || item.document_id || '',
                    documentPages,
                    downloadUrl,
                    pageCount: item.pageCount ?? item.page_count ?? null,
                    checkPoint: item.checkPoint || item.message,
                };
            });
        }
        
        return [{
            id: 1,
            name: material.name || '检查项',
            status: this.mapStatus(material.status),
            hasDocument: material.hasDocument || false,
            documentPages: [],
            checkPoint: material.review_notes || '检查完成'
        }];
    }

    buildAttachmentItem(material, attachment, attachmentIndex) {
        const materialId = material.material_code || `material_${attachmentIndex + 1}`;
        const pages = this.extractPageImages(attachment);
        let documentType = this.detectDocumentType(attachment);
        if (pages.length > 0 && documentType !== 'image') {
            documentType = 'image';
        }
        const documentUrl = this.normalizeUrl(
            attachment.preview_url || attachment.file_url || pages[0] || '',
        );
        const thumbnailUrl = this.normalizeUrl(
            attachment.thumbnail_url || documentUrl,
        );
        const status = this.mapAttachmentStatus(attachment, material.rule_evaluation);

        return {
            id: `${materialId}_att_${attachmentIndex + 1}`,
            name: attachment.file_name || `附件${attachmentIndex + 1}`,
            status,
            hasDocument: !!documentUrl,
            documentType,
            documentUrl,
            documentThumbnail: thumbnailUrl,
            documentMime: attachment.mime_type || attachment.mimeType || '',
            documentId: `${materialId}_att_${attachmentIndex + 1}`,
            fileSize: attachment.file_size ?? null,
            pageCount:
                attachment.page_count ??
                (pages.length ? pages.length : null),
            downloadUrl: this.normalizeUrl(
                attachment.download_url || attachment.file_url || documentUrl,
            ),
            documentPages: pages.map((page) => this.normalizeUrl(page)),
            isCloudShare: !!attachment.is_cloud_share,
            ocrSuccess: attachment.ocr_success !== false,
            checkPoint: material.rule_evaluation?.message || '',
            rawAttachment: attachment
        };
    }

    buildOcrTextItem(material, index) {
        const materialId = material.material_code || `material_${index + 1}`;
        return {
            id: `${materialId}_ocr_${index + 1}`,
            name: 'OCR识别结果',
            status: 'passed',
            hasDocument: true,
            documentType: 'text',
            documentContent: material.ocr_content,
            checkPoint: 'OCR识别的文本内容'
        };
    }

    buildPlaceholderItem(material, status) {
        const materialId = material.material_code || 'material_placeholder';
        return {
            id: `${materialId}_placeholder`,
            name: material.material_name || material.material_code || '材料详情',
            status,
            hasDocument: false,
            documentType: 'none',
            checkPoint: material.rule_evaluation?.message || '暂无可预览附件'
        };
    }

    buildMaterialImage(material, items) {
        const previewItem =
            items.find(
                (item) =>
                    item.documentType === 'image' && item.documentUrl,
            ) || items.find((item) => item.documentThumbnail);

        if (!previewItem) {
            return this.normalizeMaterialImage(material.image || {});
        }

        const previewUrl = this.normalizeUrl(previewItem.documentUrl || '');
        const thumbnail = this.normalizeUrl(
            previewItem.documentThumbnail || previewUrl,
        );

        return {
            status_icon:
                this.normalizeUrl(
                    material.image?.status_icon ||
                        '/static/images/智能预审_审核依据材料1.3.png',
                ) || '/static/images/智能预审_审核依据材料1.3.png',
            has_ocr_image: !!(previewUrl || thumbnail),
            ocr_image: previewUrl,
            preview_url: thumbnail || previewUrl,
        };
    }

    mapEvaluationStatus(ruleEvaluation = {}, processingStatus = null, evaluationStatus = '') {
        if (typeof evaluationStatus === 'string') {
            const normalized = evaluationStatus.toLowerCase();
            if (normalized.includes('error') || normalized.includes('failed')) {
                return 'error';
            }
            if (normalized.includes('warning')) {
                return 'hasIssues';
            }
        }

        const statusCode = ruleEvaluation?.status_code;
        if (statusCode === 200) {
            if (this.isPartialSuccess(processingStatus)) {
                return 'hasIssues';
            }
            return 'passed';
        }

        if (typeof statusCode === 'number') {
            if (statusCode >= 400) {
                return 'error';
            }
            if (statusCode >= 300) {
                return 'hasIssues';
            }
        }

        return this.isPartialSuccess(processingStatus) ? 'hasIssues' : 'passed';
    }

    mapAttachmentStatus(attachment, ruleEvaluation = {}) {
        if (attachment?.ocr_success === false) {
            return 'hasIssues';
        }
        const statusCode = ruleEvaluation?.status_code;
        if (typeof statusCode === 'number') {
            if (statusCode >= 500) {
                return 'error';
            }
            if (statusCode >= 300) {
                return 'hasIssues';
            }
            return 'passed';
        }
        return 'passed';
    }

    isPartialSuccess(processingStatus) {
        if (!processingStatus) {
            return false;
        }

        if (typeof processingStatus === 'string') {
            return processingStatus.toLowerCase().includes('partial');
        }

        if (typeof processingStatus === 'object') {
            return Object.keys(processingStatus).some(key =>
                key.toLowerCase().includes('partial')
            );
        }

        return false;
    }

    detectDocumentType(attachment = {}) {
        const mime = (attachment.mime_type || attachment.mimeType || '').toLowerCase();
        if (mime.startsWith('image/')) {
            return 'image';
        }
        if (mime === 'application/pdf') {
            return 'pdf';
        }
        if (mime.startsWith('text/')) {
            return 'text';
        }

        const ext =
            (attachment.file_type || attachment.fileType || '').toLowerCase() ||
            this.extractExtension(attachment.preview_url || attachment.file_url);

        if (['png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp'].includes(ext)) {
            return 'image';
        }
        if (ext === 'pdf') {
            return 'pdf';
        }
        if (['txt', 'text', 'md', 'log'].includes(ext)) {
            return 'text';
        }

        return 'image';
    }

    extractExtension(path = '') {
        if (!path) {
            return '';
        }
        const cleaned = path.split('#')[0].split('?')[0];
        const segment = cleaned.split('/').pop() || '';
        const dotIndex = segment.lastIndexOf('.');
        if (dotIndex > -1 && dotIndex < segment.length - 1) {
            return segment.substring(dotIndex + 1).toLowerCase();
        }
        return '';
    }

    extractPageImages(attachment = {}) {
        const directKeys = [
            'preview_pages',
            'previewPages',
            'page_images',
            'pageImages',
            'images',
            'pages'
        ];

        for (const key of directKeys) {
            if (attachment[key]) {
                const normalized = this.normalizePageImages(attachment[key]);
                if (normalized.length) {
                    return this.uniqueStrings(normalized);
                }
            }
        }

        const extra = attachment.extra;
        if (typeof extra === 'string') {
            const normalized = this.normalizePageImages(extra);
            if (normalized.length) {
                return this.uniqueStrings(normalized);
            }
        } else if (extra && typeof extra === 'object') {
            if (Array.isArray(extra)) {
                const normalized = this.normalizePageImages(extra);
                if (normalized.length) {
                    return this.uniqueStrings(normalized);
                }
            } else {
                const candidateKeys = [
                    'preview_pages',
                    'previewPages',
                    'page_images',
                    'pageImages',
                    'pages',
                    'images'
                ];
                for (const key of candidateKeys) {
                    if (extra[key]) {
                        const normalized = this.normalizePageImages(extra[key]);
                        if (normalized.length) {
                            return this.uniqueStrings(normalized);
                        }
                    }
                }

                if (extra.url || extra.preview_url || extra.src) {
                    const normalized = this.normalizePageImages([extra]);
                    if (normalized.length) {
                        return this.uniqueStrings(normalized);
                    }
                }
            }
        }

        return [];
    }

    normalizePageImages(value) {
        if (!value) {
            return [];
        }

        if (Array.isArray(value)) {
            return this.uniqueStrings(
                value
                    .map((entry) => this.normalizePageEntry(entry))
                    .filter(Boolean)
            );
        }

        if (typeof value === 'string') {
            const trimmed = value.trim();
            if (!trimmed) {
                return [];
            }

            try {
                const parsed = JSON.parse(trimmed);
                return this.normalizePageImages(parsed);
            } catch (error) {
                if (trimmed.includes(',')) {
                    return trimmed
                        .split(',')
                        .map((part) => part.trim())
                        .filter(Boolean);
                }
                return [trimmed];
            }
        }

        if (typeof value === 'object') {
            const nestedKeys = [
                'pages',
                'preview_pages',
                'previewPages',
                'page_images',
                'pageImages',
                'images'
            ];

            const collected = [];

            for (const key of nestedKeys) {
                if (value[key]) {
                    collected.push(
                        ...this.normalizePageImages(value[key])
                    );
                }
            }

            const direct = this.normalizePageEntry(value);
            if (direct) {
                collected.push(direct);
            }

            return this.uniqueStrings(collected.filter(Boolean));
        }

        return [];
    }

    normalizePageEntry(entry) {
        if (!entry) {
            return '';
        }

        if (typeof entry === 'string') {
            const trimmed = entry.trim();
            return trimmed ? this.normalizeUrl(trimmed) : '';
        }

        if (typeof entry === 'object') {
            const value =
                entry.url ||
                entry.preview_url ||
                entry.previewUrl ||
                entry.src ||
                entry.image ||
                ''
            ;
            return this.normalizeUrl(value);
        }

        return '';
    }

    uniqueStrings(values) {
        return Array.from(new Set((values || []).filter(Boolean)));
    }

    normalizeEvaluationResult(rawResult) {
        if (!rawResult) {
            return null;
        }

        if (typeof rawResult === 'string') {
            const trimmed = rawResult.trim();
            if (!trimmed) {
                return null;
            }
            try {
                const parsed = JSON.parse(trimmed);
                if (parsed && typeof parsed === 'object') {
                    return parsed;
                }
            } catch (error) {
                console.warn('evaluation_result 字段解析失败，忽略该内容', error);
            }
            return null;
        }

        if (typeof rawResult === 'object') {
            return rawResult;
        }

        return null;
    }

    mapLegacyRuleStatus(result) {
        if (!result) {
            return 'passed';
        }
        const normalized = String(result).toLowerCase();
        if (normalized.includes('fail') || normalized.includes('error')) {
            return 'error';
        }
        if (normalized.includes('warn')) {
            return 'hasIssues';
        }
        return 'passed';
    }

    determineOverallResult(evaluationResult, materials) {
        if (evaluationResult?.evaluation_summary) {
            const summary = evaluationResult.evaluation_summary;
            const result = this.mapOverallResult(summary.overall_result);
            const total =
                summary.total_materials ||
                materials.length ||
                (Array.isArray(evaluationResult.material_results)
                    ? evaluationResult.material_results.length
                    : 0) ||
                1;
            const progress =
                summary.passed_materials !== undefined
                    ? Math.round(
                          (summary.passed_materials / Math.max(total, 1)) *
                              100,
                      )
                    : 100;
            return {
                result,
                progress: Number.isFinite(progress) ? progress : 100,
            };
        }

        const fallbackResult = this.calculateOverallResult(evaluationResult);
        const totalMaterials = materials.length || 1;
        const passedCount = materials.filter(
            (material) => material.status === 'passed',
        ).length;
        const progress = Math.round((passedCount / totalMaterials) * 100);

        return {
            result: fallbackResult,
            progress: Number.isFinite(progress) ? progress : 100,
        };
    }

    mapOverallResult(overallResult) {
        if (!overallResult) {
            return 'passed';
        }
        const normalized = String(overallResult).toLowerCase();
        if (normalized.includes('fail')) {
            return 'error';
        }
        if (
            normalized.includes('suggest') ||
            normalized.includes('require') ||
            normalized.includes('partial')
        ) {
            return 'hasIssues';
        }
        return 'passed';
    }

    normalizeMaterialImage(image = {}) {
        if (!image || typeof image !== 'object') {
            return {};
        }
        const previewUrl = this.normalizeUrl(
            image.preview_url || image.ocr_image || image.previewUrl || '',
        );
        const statusIcon = this.normalizeUrl(
            image.status_icon || image.thumbnail || '/static/images/智能预审_审核依据材料1.3.png',
        );
        return {
            ...image,
            preview_url: previewUrl,
            ocr_image: previewUrl,
            status_icon: statusIcon,
        };
    }

    normalizeUrl(url) {
        if (!url || typeof url !== 'string') {
            return '';
        }
        let trimmed = url.trim();
        if (!trimmed) {
            return '';
        }
        if (trimmed.startsWith('data:') || trimmed.startsWith('blob:')) {
            return trimmed;
        }
        const customPrefixRegex = /^zhzwdxt[:\.\/]+/i;
        if (customPrefixRegex.test(trimmed)) {
            trimmed = trimmed.replace(customPrefixRegex, '');
        }
        const httpIndex = trimmed.toLowerCase().indexOf('http://');
        const httpsIndex = trimmed.toLowerCase().indexOf('https://');
        const firstIndex = httpIndex >= 0 && httpsIndex >= 0
            ? Math.min(httpIndex, httpsIndex)
            : (httpIndex >= 0 ? httpIndex : httpsIndex);
        if (firstIndex > 0) {
            trimmed = trimmed.slice(firstIndex);
        }
        if (trimmed.startsWith('//')) {
            trimmed = `${window.location.protocol}${trimmed}`;
        }
        if (
            trimmed.startsWith('http://') &&
            window.location.protocol === 'https:'
        ) {
            try {
                const parsed = new URL(trimmed);
                parsed.protocol = 'https:';
                trimmed = parsed.toString();
            } catch (error) {
                console.warn('URL 规范化失败', trimmed, error);
            }
        }
        if (
            !trimmed.startsWith('http://') &&
            !trimmed.startsWith('https://')
        ) {
            if (!trimmed.startsWith('/')) {
                trimmed = `/${trimmed}`;
            }
            return `${window.location.origin}${trimmed}`;
        }
        return trimmed;
    }

    extractFormValue(data, fieldCode) {
        if (data.form_data && Array.isArray(data.form_data)) {
            const field = data.form_data.find(f => f.code === fieldCode);
            return field ? field.value : null;
        }
        return null;
    }

    calculateOverallResult(evaluation) {
        if (!evaluation) {
            return 'passed';
        }

        if (
            evaluation.evaluation_summary &&
            evaluation.evaluation_summary.overall_result
        ) {
            return this.mapOverallResult(
                evaluation.evaluation_summary.overall_result,
            );
        }

        if (Array.isArray(evaluation.material_results)) {
            let failedCount = 0;
            let warningCount = 0;
            evaluation.material_results.forEach((material) => {
                const status = this.mapEvaluationStatus(
                    material.rule_evaluation,
                    material.processing_status,
                    material.evaluation_status,
                );
                if (status === 'error') {
                    failedCount += 1;
                } else if (status === 'hasIssues') {
                    warningCount += 1;
                }
            });

            if (failedCount > 0) return 'error';
            if (warningCount > 0) return 'hasIssues';
            return 'passed';
        }

        if (Array.isArray(evaluation.rules)) {
            const failedCount = evaluation.rules.filter(
                (rule) =>
                    this.mapLegacyRuleStatus(rule.result) === 'error',
            ).length;
            const warningCount = evaluation.rules.filter(
                (rule) =>
                    this.mapLegacyRuleStatus(rule.result) === 'hasIssues',
            ).length;

            if (failedCount > 0) return 'error';
            if (warningCount > 0) return 'hasIssues';
        }

        return 'passed';
    }

    generateStatusMessage(result, materialsCount) {
        switch (result) {
            case 'passed': return '智能预审通过，所有材料符合要求';
            case 'hasIssues':
                return materialsCount > 0
                    ? `发现${materialsCount}个需要注意的问题，请查看左侧标记的材料`
                    : '部分材料存在需要注意的问题，请查看明细';
            case 'error': return '发现重要问题，请检查相关材料';
            default: return '智能预审完成';
        }
    }

    mapStatus(backendStatus) {
        const statusMap = this.config.DATA_MAPPING?.preview?.statusMapping || {
            'success': 'passed',
            'passed': 'passed',
            'warning': 'hasIssues',
            'error': 'error',
            'failed': 'error',
            'pending': 'loading',
            'processing': 'loading'
        };
        return this.utils.mapStatus ? 
            this.utils.mapStatus(backendStatus, statusMap) :
            statusMap[backendStatus] || 'passed';
    }

    mapAuditStatus(backendStatus) {
        const statusMap = this.config.DATA_MAPPING?.preview?.auditStatusMapping || {
            'completed': 'completed',
            'processing': 'processing',
            'pending': 'pending',
            'error': 'error',
            'failed': 'error'
        };
        return this.utils.mapStatus ? 
            this.utils.mapStatus(backendStatus, statusMap) :
            statusMap[backendStatus] || 'completed';
    }

}

window.apiService = new ApiService();
})();
