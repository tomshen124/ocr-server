document.addEventListener('DOMContentLoaded', () => {

    // --- DOM 元素获取 ---
    const screens = {
        loading: document.getElementById('loading-screen'),
        main: document.getElementById('main-screen'),
        error: document.getElementById('error-screen')
    };
    const progressBar = document.getElementById('progress-bar');
    const progressText = document.getElementById('progress-text');
    const retryButton = document.getElementById('retry-button');
    const sidebarContainer = document.getElementById('sidebar');
    const imageContainer = document.getElementById('image-container');
    const materialImage = document.getElementById('material-image');
    
    let activeMaterialId = null;

    // --- 数据获取 ---

    /**
     * 从URL参数中获取预审ID
     * 支持多种参数名：previewId, preview_id, id, request_id
     */
    function getPreviewIdFromUrl() {
        const urlParams = new URLSearchParams(window.location.search);
        const possibleParams = ['previewId', 'preview_id', 'id', 'request_id'];
        
        for (const param of possibleParams) {
            const value = urlParams.get(param);
            if (value) {
                console.log(`从URL参数 '${param}' 获取到预审ID: ${value}`);
                return value;
            }
        }
        
        // 如果URL参数中没有，尝试从路径中提取
        const pathMatch = window.location.pathname.match(/\/preview\/view\/([^/]+)/);
        if (pathMatch && pathMatch[1]) {
            console.log(`从URL路径获取到预审ID: ${pathMatch[1]}`);
            return pathMatch[1];
        }
        
        console.warn('未能从URL中获取到预审ID，将使用测试ID');
        return 'test-preview-id'; // 开发测试用的默认ID
    }

    /**
     * 将后端预审数据转换为前端需要的格式
     */
    function transformPreviewDataToFrontend(backendData) {
        console.log('转换后端数据:', backendData);
        
        // 默认占位图
        const placeholderImage = 'https://via.placeholder.com/800x1000.png?text=预审文档';
        
        // 基本信息转换
        const basicInfo = {
            applicant: backendData.applicant || '未知申请人',
            projectName: backendData.projectName || backendData.matterName || '未知项目',
            approvalItem: backendData.approvalItem || backendData.themeName || '未知审批事项'
        };
        
        // 材料列表转换
        const materialsToReview = [];
        const approvedMaterials = [];
        
        if (backendData.materials && Array.isArray(backendData.materials)) {
            backendData.materials.forEach((material, index) => {
                const materialData = {
                    id: material.id || index + 1,
                    name: material.name || `材料${index + 1}`,
                    count: material.count || 1,
                    image: material.imageUrl || placeholderImage,
                    reviewPoints: material.reviewPoints || []
                };
                
                // 如果有子项
                if (material.subItems && Array.isArray(material.subItems)) {
                    materialData.subItems = material.subItems.map((subItem, subIndex) => ({
                        id: subItem.id || `sub${index}_${subIndex}`,
                        name: subItem.name || `子项${subIndex + 1}`,
                        image: subItem.imageUrl || placeholderImage,
                        hoverText: subItem.description || subItem.hoverText
                    }));
                }
                
                // 根据状态分类
                if (material.status === 'passed' || material.status === 'approved') {
                    approvedMaterials.push(materialData);
                } else {
                    materialsToReview.push(materialData);
                }
            });
        }
        
        return {
            basicInfo,
            materialsToReview,
            approvedMaterials
        };
    }

    /**
     * 从后端API获取预审数据
     */
    async function fetchDataFromAPI() {
        console.log('开始从后端API获取预审数据...');
        
        try {
            const previewId = getPreviewIdFromUrl();
            console.log(`使用预审ID: ${previewId}`);
            
            // 首先检查预审状态
            const statusResponse = await fetch(`/api/preview/status/${previewId}`, {
                method: 'GET',
                headers: {
                    'Content-Type': 'application/json'
                },
                credentials: 'include' // 包含会话cookie
            });
            
            if (!statusResponse.ok) {
                throw new Error(`状态查询失败: ${statusResponse.status} ${statusResponse.statusText}`);
            }
            
            const statusResult = await statusResponse.json();
            console.log('预审状态:', statusResult);
            
            if (!statusResult.success) {
                throw new Error(statusResult.errorMsg || '获取预审状态失败');
            }
            
            const status = statusResult.data?.status || 'unknown';
            console.log(`当前预审状态: ${status}`);
            
            // 如果预审还在处理中，等待一段时间后重试
            if (status === 'processing' || status === 'submitted') {
                console.log('预审正在处理中，等待完成...');
                await new Promise(resolve => setTimeout(resolve, 2000));
                // 递归调用，继续检查状态
                return await fetchDataFromAPI();
            }
            
            if (status === 'failed') {
                throw new Error('预审处理失败');
            }
            
            // 获取完整的预审数据
            const dataResponse = await fetch(`/api/preview/data/${previewId}`, {
                method: 'GET',
                headers: {
                    'Content-Type': 'application/json'
                },
                credentials: 'include'
            });
            
            if (!dataResponse.ok) {
                throw new Error(`数据获取失败: ${dataResponse.status} ${dataResponse.statusText}`);
            }
            
            const dataResult = await dataResponse.json();
            console.log('预审数据:', dataResult);
            
            if (!dataResult.success) {
                throw new Error(dataResult.errorMsg || '获取预审数据失败');
            }
            
            // 转换数据格式
            const frontendData = transformPreviewDataToFrontend(dataResult.data);
            console.log('转换后的前端数据:', frontendData);
            
            return frontendData;
            
        } catch (error) {
            console.error('获取预审数据失败:', error);
            
            // 如果是开发环境或者API不可用，返回模拟数据
            if (window.location.hostname === 'localhost' || window.location.hostname === '127.0.0.1') {
                console.warn('检测到开发环境，使用模拟数据');
                return getFallbackData();
            }
            
            throw error;
        }
    }
    
    /**
     * 获取后备模拟数据（用于开发环境或API不可用时）
     */
    function getFallbackData() {
        const placeholderImage = 'https://via.placeholder.com/800x1000.png?text=示例文档';
        return {
            basicInfo: { 
                applicant: '浙江一二三四科技有限责任公司', 
                projectName: '内资公司变更', 
                approvalItem: '经营范围' 
            },
            materialsToReview: [
                { 
                    id: 1, 
                    name: '《公司变更登记申请书》', 
                    count: 2, 
                    image: placeholderImage, 
                    reviewPoints: [ 
                        { id: 'p1', top: '33%', left: '60%', width: '15%', height: '5%' }, 
                        { id: 'p2', top: '65%', left: '78%', width: '10%', height: '5%' } 
                    ]
                },
                { 
                    id: 2, 
                    name: '《质量担当合同》', 
                    count: 3, 
                    subItems: [ 
                        { 
                            id: 'sub1', 
                            name: '检查要点', 
                            image: placeholderImage, 
                            hoverText: '经营场所在中国(上海)自由贸易试验区临港新片区依法设立并登记的企业;满足新片区产业导向的企业;未被列入严重违法失信企业名单。' 
                        }, 
                        { id: 'sub2', name: '线上申办材料', image: placeholderImage }, 
                        { id: 'sub3', name: '法人信息', image: placeholderImage }
                    ] 
                },
                { id: 3, name: '《建筑执照》', count: 4, image: placeholderImage }
            ],
            approvedMaterials: [ 
                { id: 4, name: '《公司章程》', image: placeholderImage } 
            ]
        };
    }

    // --- 界面渲染和逻辑 ---

    function showScreen(screenName) {
        Object.values(screens).forEach(screen => screen.classList.add('hidden'));
        screens[screenName].classList.remove('hidden');
    }

    function renderSidebar(data) {
        sidebarContainer.innerHTML = '';
        const infoHtml = `<div class="sidebar-section"><p class="info-title">基本信息</p><div class="info-item"><span>申&nbsp;&nbsp;请&nbsp;&nbsp;人</span>: ${data.basicInfo.applicant}</div><div class="info-item"><span>项目名称</span>: ${data.basicInfo.projectName}</div><div class="info-item"><span>审批事项</span>: ${data.basicInfo.approvalItem}</div></div>`;
        const toReviewHtml = `<div class="sidebar-section"><p class="section-title">需检查的材料 <span class="count-badge">${data.materialsToReview.length}</span></p><ul>${data.materialsToReview.map(mat => `<li class="material-item-wrapper" data-id="${mat.id}"><div class="material-item">${mat.name} (${mat.count})</div>${mat.subItems ? `<ul class="sub-item-list">${mat.subItems.map(sub => `<li class="sub-item" data-id="${sub.id}">${sub.name}${sub.hoverText ? `<div class="tooltip">${sub.hoverText}</div>` : ''}</li>`).join('')}</ul>` : ''}</li>`).join('')}</ul></div>`;
        const approvedHtml = `<div class="sidebar-section"><p class="section-title">已通过的材料</p><ul>${data.approvedMaterials.map(mat => `<li class="material-item-wrapper" data-id="${mat.id}"><div class="material-item passed">${mat.name}</div></li>`).join('')}</ul></div>`;
        sidebarContainer.innerHTML = infoHtml + toReviewHtml + approvedHtml;
        addSidebarEventListeners(data);
    }
    
    function addSidebarEventListeners(data) {
        sidebarContainer.querySelectorAll('.material-item, .sub-item').forEach(item => {
            item.addEventListener('click', (e) => {
                e.stopPropagation();
                const wrapper = e.currentTarget.closest('.material-item-wrapper, .sub-item');
                const id = wrapper.dataset.id;
                selectMaterial(id, data);
            });
        });
    }

    function selectMaterial(id, data) {
        activeMaterialId = id;
        let selectedMaterial = null;
        let parentMaterialId = null;

        data.materialsToReview.forEach(mat => {
            if (String(mat.id) === id) selectedMaterial = mat;
            if (mat.subItems) {
                const sub = mat.subItems.find(s => String(s.id) === id);
                if (sub) {
                    selectedMaterial = sub;
                    parentMaterialId = mat.id;
                }
            }
        });
        if (!selectedMaterial) {
            selectedMaterial = data.approvedMaterials.find(mat => String(mat.id) === id);
        }

        if (selectedMaterial) {
            renderContentView(selectedMaterial);
        }
        
        document.querySelectorAll('.material-item, .sub-item').forEach(item => item.classList.remove('active'));
        const activeElement = sidebarContainer.querySelector(`[data-id='${id}']`);
        if (activeElement) {
            activeElement.classList.add('active');
            if (activeElement.classList.contains('sub-item')) {
                activeElement.closest('.material-item-wrapper').querySelector('.material-item').classList.add('active');
            }
        }
    }

    function renderContentView(material) {
        materialImage.src = material.image || '';
        materialImage.alt = material.name;
        imageContainer.querySelectorAll('.review-point').forEach(p => p.remove());
        if (material.reviewPoints) {
            material.reviewPoints.forEach(point => {
                const pointEl = document.createElement('div');
                pointEl.className = 'review-point';
                pointEl.style.top = point.top;
                pointEl.style.left = point.left;
                pointEl.style.width = point.width;
                pointEl.style.height = point.height;
                imageContainer.appendChild(pointEl);
            });
        }
    }

    async function initializeApp() {
        showScreen('loading');
        let progress = 0;
        const interval = setInterval(() => {
            progress = Math.min(progress + 2, 99);
            progressBar.style.width = progress + '%';
            progressText.textContent = progress + '%';
        }, 30);

        try {
            const appData = await fetchDataFromAPI();
            clearInterval(interval);
            progressBar.style.width = '100%';
            progressText.textContent = '100%';

            setTimeout(() => {
                showScreen('main');
                renderSidebar(appData); 
                const firstMaterial = appData.materialsToReview[0];
                if (firstMaterial) {
                    const defaultSelection = (firstMaterial.subItems && firstMaterial.subItems.length > 0) ? firstMaterial.subItems[0] : firstMaterial;
                    selectMaterial(String(defaultSelection.id), appData);
                }
            }, 300);

        } catch (error) {
            clearInterval(interval);
            setTimeout(() => showScreen('error'), 300);
        }
    }

    // --- 事件绑定和启动 ---
    retryButton.addEventListener('click', initializeApp);
    initializeApp();
});