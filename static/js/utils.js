// 工具函數模塊
(() => {
// 格式化日期
function formatDate(date) {
    if (!date) return '';
    
    if (typeof date === 'string') {
        date = new Date(date);
    }
    
    const year = date.getFullYear();
    const month = String(date.getMonth() + 1).padStart(2, '0');
    const day = String(date.getDate()).padStart(2, '0');
    
    return `${year}年${month}月${day}日`;
}

// 格式化數字
function formatNumber(num) {
    if (num === undefined || num === null) return '';
    
    return num.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ',');
}

// 生成唯一ID
function generateUniqueId() {
    return Date.now().toString(36) + Math.random().toString(36).substr(2, 5);
}

// 防抖函數
function debounce(func, wait) {
    let timeout;
    return function(...args) {
        const context = this;
        clearTimeout(timeout);
        timeout = setTimeout(() => func.apply(context, args), wait);
    };
}

// 節流函數
function throttle(func, limit) {
    let inThrottle;
    return function(...args) {
        const context = this;
        if (!inThrottle) {
            func.apply(context, args);
            inThrottle = true;
            setTimeout(() => inThrottle = false, limit);
        }
    };
}

// 深拷貝對象
function deepClone(obj) {
    if (obj === null || typeof obj !== 'object') {
        return obj;
    }
    
    if (obj instanceof Date) {
        return new Date(obj);
    }
    
    if (obj instanceof Array) {
        return obj.map(item => deepClone(item));
    }
    
    if (obj instanceof Object) {
        const copy = {};
        Object.keys(obj).forEach(key => {
            copy[key] = deepClone(obj[key]);
        });
        return copy;
    }
}

// 獲取狀態顏色
function getStatusColor(status) {
    switch(status) {
        case 'passed': return '#52c41a';
        case 'warning': return '#fa8c16';
        case 'error': return '#f5222d';
        case 'hasIssues': return '#fa8c16';
        default: return '#52c41a';
    }
}

// 獲取狀態圖標
function getStatusIcon(status) {
    switch(status) {
        case 'passed': return '✓';
        case 'warning': return '⚠';
        case 'error': return '✗';
        case 'hasIssues': return '⚠';
        default: return '✓';
    }
}

// 獲取狀態文本
function getStatusText(status) {
    switch(status) {
        case 'passed': return '通過';
        case 'warning': return '需注意';
        case 'error': return '異常';
        case 'hasIssues': return '需注意';
        default: return '通過';
    }
}

// 創建SVG圖標
function createSvgIcon(type) {
    const icons = {
        success: `
            <svg width="80" height="80" viewBox="0 0 80 80">
                <circle cx="40" cy="40" r="35" fill="#e8f4fd" stroke="#4a90e2" stroke-width="2"/>
                <path d="M25 40l10 10 20-20" stroke="#4a90e2" stroke-width="3" fill="none"/>
            </svg>
        `,
        warning: `
            <svg width="80" height="80" viewBox="0 0 80 80">
                <circle cx="40" cy="40" r="35" fill="#fff7e6" stroke="#fa8c16" stroke-width="2"/>
                <path d="M40 20l-3 25h6l-3-25z" fill="#fa8c16"/>
                <circle cx="40" cy="55" r="3" fill="#fa8c16"/>
            </svg>
        `,
        error: `
            <svg width="80" height="80" viewBox="0 0 80 80">
                <circle cx="40" cy="40" r="35" fill="#fff2f0" stroke="#f5222d" stroke-width="2"/>
                <path d="M25 25l30 30M55 25l-30 30" stroke="#f5222d" stroke-width="3"/>
            </svg>
        `,
        loading: `
            <svg width="80" height="80" viewBox="0 0 80 80">
                <circle cx="40" cy="40" r="35" fill="none" stroke="#e8e8e8" stroke-width="4"/>
                <circle cx="40" cy="40" r="35" fill="none" stroke="#4a90e2" stroke-width="4" 
                        stroke-dasharray="164" stroke-dashoffset="41" stroke-linecap="round">
                    <animateTransform attributeName="transform" type="rotate" 
                                    dur="1s" repeatCount="indefinite" values="0 40 40;360 40 40"/>
                </circle>
            </svg>
        `
    };
    
    return icons[type] || icons.success;
}

// 創建模擬文檔圖像
function createDocumentImage(type, data = {}) {
    if (type === 'license') {
        return createLicenseImage(data);
    } else if (type === 'table') {
        return createTableImage(data);
    } else {
        return createDefaultDocumentImage();
    }
}

// 創建營業執照圖像
function createLicenseImage(data = {}) {
    const {
        companyName = '示例企业有限公司',
        creditCode = '91330000XXXXXXXXXX',
        legalPerson = '張三',
        registeredCapital = '1000萬元人民幣',
        establishDate = '2020年01月01日'
    } = data;
    
    return `data:image/svg+xml;base64,${btoa(`
        <svg width="600" height="800" xmlns="http://www.w3.org/2000/svg">
            <rect width="600" height="800" fill="#f8f9fa" stroke="#ddd" stroke-width="2"/>
            <rect x="50" y="50" width="500" height="700" fill="white" stroke="#ccc"/>
            
            <text x="300" y="100" text-anchor="middle" font-size="28" font-weight="bold" fill="#333">營業執照</text>
            <text x="300" y="130" text-anchor="middle" font-size="16" fill="#666">(副本)</text>
            
            <text x="100" y="180" font-size="14" fill="#666">統一社會信用代碼：${creditCode}</text>
            <text x="100" y="220" font-size="14" fill="#666">名稱：${companyName}</text>
            <text x="100" y="260" font-size="14" fill="#666">類型：有限責任公司</text>
            <text x="100" y="300" font-size="14" fill="#666">法定代表人：${legalPerson}</text>
            <text x="100" y="340" font-size="14" fill="#666">註冊資本：${registeredCapital}</text>
            <text x="100" y="380" font-size="14" fill="#666">成立日期：${establishDate}</text>
            <text x="100" y="420" font-size="14" fill="#666">營業期限：${establishDate}至長期</text>
            <text x="100" y="460" font-size="14" fill="#666">經營範圍：軟件開發；技術服務；技術轉讓；技術諮詢</text>
            
            <circle cx="450" cy="600" r="60" fill="none" stroke="#e74c3c" stroke-width="2"/>
            <text x="450" y="605" text-anchor="middle" font-size="14" fill="#e74c3c">工商行政管理局</text>
            <text x="450" y="625" text-anchor="middle" font-size="14" fill="#e74c3c">公章</text>
            
            <text x="100" y="700" font-size="14" fill="#666">發照日期：${establishDate}</text>
        </svg>
    `)}`;
}

// 創建表格圖像
function createTableImage(data = {}) {
    return `data:image/svg+xml;base64,${btoa(`
        <svg width="600" height="400" xmlns="http://www.w3.org/2000/svg">
            <rect width="600" height="400" fill="#f8f9fa" stroke="#ddd" stroke-width="2"/>
            <rect x="50" y="50" width="500" height="300" fill="white" stroke="#ccc"/>
            <text x="300" y="80" text-anchor="middle" font-size="18" font-weight="bold" fill="#333">杭州市工商主管部門設立登記申請清單</text>
            
            <!-- 表格頭部 -->
            <rect x="80" y="100" width="440" height="30" fill="#f0f0f0" stroke="#ccc"/>
            <text x="100" y="120" font-size="12" fill="#333">序號</text>
            <text x="150" y="120" font-size="12" fill="#333">事項</text>
            <text x="250" y="120" font-size="12" fill="#333">要求</text>
            <text x="350" y="120" font-size="12" fill="#333">份數</text>
            <text x="400" y="120" font-size="12" fill="#333">審核結果</text>
            
            <!-- 表格內容 -->
            <rect x="80" y="130" width="440" height="25" fill="white" stroke="#ccc"/>
            <text x="100" y="147" font-size="11" fill="#333">1</text>
            <text x="150" y="147" font-size="11" fill="#333">法人代表</text>
            <text x="250" y="147" font-size="11" fill="#333">簽字</text>
            <text x="350" y="147" font-size="11" fill="#333">1</text>
            <circle cx="420" cy="142" r="6" fill="#52c41a"/>
            
            <rect x="80" y="155" width="440" height="25" fill="#fff2e8" stroke="#ccc"/>
            <text x="100" y="172" font-size="11" fill="#333">2</text>
            <text x="150" y="172" font-size="11" fill="#333">法人簽章</text>
            <text x="250" y="172" font-size="11" fill="#333">蓋章</text>
            <text x="350" y="172" font-size="11" fill="#333">1</text>
            <circle cx="420" cy="167" r="6" fill="#fa8c16"/>
            
            <rect x="80" y="180" width="440" height="25" fill="white" stroke="#ccc"/>
            <text x="100" y="197" font-size="11" fill="#333">3</text>
            <text x="150" y="197" font-size="11" fill="#333">企業法人簽章</text>
            <text x="250" y="197" font-size="11" fill="#333">蓋章</text>
            <text x="350" y="197" font-size="11" fill="#333">1</text>
            <circle cx="420" cy="192" r="6" fill="#52c41a"/>
        </svg>
    `)}`;
}

// 創建默認文檔圖像
function createDefaultDocumentImage() {
    return `data:image/svg+xml;base64,${btoa(`
        <svg width="400" height="500" xmlns="http://www.w3.org/2000/svg">
            <rect width="400" height="500" fill="#f8f9fa" stroke="#ddd" stroke-width="2"/>
            <rect x="50" y="50" width="300" height="400" fill="white" stroke="#ccc"/>
            <text x="200" y="250" text-anchor="middle" font-size="24" fill="#ccc">文檔預覽</text>
        </svg>
    `)}`;
}

// 導出工具函數
window.utils = {
    formatDate,
    formatNumber,
    generateUniqueId,
    debounce,
    throttle,
    deepClone,
    getStatusColor,
    getStatusIcon,
    getStatusText,
    createSvgIcon,
    createDocumentImage
};
})();
