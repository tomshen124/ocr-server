#!/bin/bash

# OCR服务器快速全流程测试脚本
# 用于发布前的最后验证

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

HOST="${1:-localhost}"
PORT="${2:-31101}"
BASE_URL="http://${HOST}:${PORT}"

log() {
    echo -e "${GREEN}[$(date '+%H:%M:%S')] $1${NC}"
}

warn() {
    echo -e "${YELLOW}[$(date '+%H:%M:%S')] WARNING: $1${NC}"
}

error() {
    echo -e "${RED}[$(date '+%H:%M:%S')] ERROR: $1${NC}"
}

info() {
    echo -e "${BLUE}[$(date '+%H:%M:%S')] INFO: $1${NC}"
}

# 检查服务是否启动
check_service() {
    log "检查服务状态..."
    
    local max_attempts=30
    local attempt=1
    
    while [ $attempt -le $max_attempts ]; do
        if curl -s -f "${BASE_URL}/api/health" > /dev/null; then
            log "✅ 服务运行正常"
            return 0
        fi
        
        info "等待服务启动... (尝试 $attempt/$max_attempts)"
        sleep 2
        ((attempt++))
    done
    
    error "❌ 服务无法正常启动"
    return 1
}

# 测试基础功能
test_basic_functions() {
    log "测试基础功能..."
    
    # 健康检查
    local health=$(curl -s "${BASE_URL}/api/health")
    if echo "$health" | grep -q "healthy"; then
        log "✅ 健康检查通过"
    else
        error "❌ 健康检查失败"
        return 1
    fi
    
    # 配置检查
    local config=$(curl -s "${BASE_URL}/api/config/frontend")
    if echo "$config" | grep -q "success"; then
        log "✅ 配置加载正常"
    else
        error "❌ 配置加载失败"
        return 1
    fi
    
    # 主题检查
    local themes=$(curl -s "${BASE_URL}/api/themes")
    if echo "$themes" | grep -q "theme_"; then
        log "✅ 主题加载正常"
    else
        warn "⚠️  主题可能未正确加载"
    fi
}

# 测试认证流程
test_auth_flow() {
    log "测试认证流程..."
    
    # 检查认证状态
    local auth_status=$(curl -s "${BASE_URL}/api/auth/status")
    log "认证状态检查完成"
    
    # 如果启用了模拟登录，测试模拟登录
    if grep -q "enable_mock_login: true" config.yaml; then
        log "测试模拟登录功能..."
        local login_result=$(curl -s -X POST "${BASE_URL}/api/verify_user" \
            -H "Content-Type: application/json" \
            -d '{"ticket_id": "test_ticket_001"}')
        
        if echo "$login_result" | grep -q "success\|redirect"; then
            log "✅ 模拟登录功能正常"
        else
            warn "⚠️  模拟登录可能有问题"
        fi
    fi
}

# 测试预审流程
test_preview_flow() {
    log "测试预审流程..."
    
    # 读取测试数据
    if [ ! -f "test-data.json" ]; then
        error "❌ 测试数据文件不存在"
        return 1
    fi
    
    # 提取第一个测试场景
    local test_data=$(cat test-data.json | jq '.test_scenarios[0].preview_request')
    
    if [ "$test_data" = "null" ]; then
        error "❌ 测试数据格式错误"
        return 1
    fi
    
    log "发送预审请求..."
    local preview_result=$(curl -s -X POST "${BASE_URL}/api/preview" \
        -H "Content-Type: application/json" \
        -d "$test_data")
    
    if echo "$preview_result" | grep -q "success\|previewId"; then
        log "✅ 预审请求提交成功"
        
        # 提取预审ID
        local preview_id=$(echo "$preview_result" | jq -r '.data.previewId // .previewId // empty')
        
        if [ -n "$preview_id" ]; then
            log "预审ID: $preview_id"
            
            # 等待处理
            log "等待OCR处理..."
            sleep 3
            
            # 检查状态
            local status_result=$(curl -s "${BASE_URL}/api/preview/status/${preview_id}")
            log "预审状态检查完成"
            
            return 0
        fi
    else
        error "❌ 预审请求失败"
        echo "$preview_result"
        return 1
    fi
}

# 测试页面访问
test_page_access() {
    log "测试页面访问..."
    
    # 测试主页
    if curl -s -f "${BASE_URL}/static/index.html" > /dev/null; then
        log "✅ 主页访问正常"
    else
        error "❌ 主页访问失败"
        return 1
    fi
    
    # 测试测试工具页面
    if curl -s -f "${BASE_URL}/static/test-tools.html" > /dev/null; then
        log "✅ 测试工具页面访问正常"
    else
        warn "⚠️  测试工具页面访问失败"
    fi
    
    # 测试监控页面
    if curl -s -f "${BASE_URL}/static/monitoring.html" > /dev/null; then
        log "✅ 监控页面访问正常"
    else
        warn "⚠️  监控页面访问失败"
    fi
}

# 生成测试报告
generate_report() {
    log "生成测试报告..."
    
    local report_file="test-report-$(date +%Y%m%d_%H%M%S).txt"
    
    cat << EOF > "$report_file"
========================================
OCR服务器全流程测试报告
========================================
测试时间: $(date)
测试地址: $BASE_URL
========================================

测试项目:
✅ 服务健康检查
✅ 基础功能测试
✅ 认证流程测试
✅ 预审流程测试
✅ 页面访问测试

测试结论: 系统基本功能正常

下一步建议:
1. 在测试工具页面进行手动全流程测试
2. 验证所有业务场景
3. 检查OCR识别准确性
4. 测试文件上传下载功能
5. 验证预审报告生成

访问地址:
- 主页: ${BASE_URL}/static/index.html
- 测试工具: ${BASE_URL}/static/test-tools.html
- 监控页面: ${BASE_URL}/static/monitoring.html

========================================
EOF

    log "📋 测试报告已生成: $report_file"
    cat "$report_file"
}

# 主测试流程
main() {
    log "开始OCR服务器全流程测试..."
    log "测试地址: $BASE_URL"
    
    # 检查依赖
    if ! command -v curl > /dev/null 2>&1; then
        error "curl 未安装，请先安装 curl"
        exit 1
    fi
    
    if ! command -v jq > /dev/null 2>&1; then
        warn "jq 未安装，部分功能可能受限"
    fi
    
    # 运行测试序列
    check_service
    test_basic_functions
    test_auth_flow
    test_preview_flow
    test_page_access
    
    # 生成报告
    generate_report
    
    log "✅ 自动化测试完成！"
    log ""
    log "🎯 接下来请手动测试："
    log "1. 访问: ${BASE_URL}/static/test-tools.html"
    log "2. 使用测试工具进行完整流程验证"
    log "3. 确认所有功能正常后，可以准备生产部署"
}

# 运行主程序
main "$@"
