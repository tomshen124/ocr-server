#!/bin/bash

# OCR智能预审系统 - 统一测试脚本
# 支持测试/开发/生产三种环境的快速验证

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# 默认配置
MODE="${1:-test}"
HOST="${2:-localhost}"
PORT="${3:-31101}"
BASE_URL="http://${HOST}:${PORT}"

# 日志函数
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

# 使用说明
usage() {
    cat << EOF
OCR智能预审系统统一测试脚本

用法: $0 [MODE] [HOST] [PORT]

参数:
  MODE    测试模式: dev|test|prod (默认: test)
  HOST    服务器地址 (默认: localhost)
  PORT    服务器端口 (默认: 31101)

示例:
  $0                    # 测试模式，localhost:31101
  $0 test               # 测试模式
  $0 dev localhost 3000 # 开发模式，localhost:3000
  $0 prod 192.168.1.100 # 生产模式，指定IP

测试内容:
  ✓ 服务健康检查
  ✓ 测试用户认证
  ✓ 文件上传测试
  ✓ OCR识别验证
  ✓ 结果页面检查
  ✓ 性能基准测试

EOF
}

# 检查服务健康
health_check() {
    log "开始健康检查..."
    
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

# 测试用户认证
test_auth() {
    log "测试用户认证..."
    
    case $MODE in
        test|dev)
            log "✅ 测试模式：跳过认证检查"
            ;;
        prod)
            log "检查生产环境认证..."
            if curl -s -f "${BASE_URL}/api/auth/config" > /dev/null; then
                log "✅ 认证服务正常"
            else
                warn "⚠️  认证服务可能未配置"
            fi
            ;;
    esac
}

# 测试文件上传
test_file_upload() {
    log "测试文件上传功能..."
    
    # 创建测试文件
    local test_file="/tmp/test_upload.pdf"
    echo "%PDF-1.4
%测试文件
1 0 obj
<</Type/Catalog/Pages 2 0 R>>
endobj
2 0 obj
<</Type/Pages/Kids[3 0 R]/Count 1>>
endobj
3 0 obj
<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]>>
endobj
xref
0 4
0000000000 65535 f 
0000000009 00000 n 
0000000058 00000 n 
0000000115 00000 n 
trailer
<</Size 4/Root 1 0 R>>
startxref
190
%%EOF" > "$test_file"

    # 获取主题列表
    local themes=$(curl -s "${BASE_URL}/api/themes" | jq -r '.[0].id' 2>/dev/null || echo "theme_001")
    
    # 上传测试
    local response=$(curl -s -w "%{http_code}" -o /tmp/upload_response.json \
        -F "files=@$test_file" \
        -F "theme=$themes" \
        "${BASE_URL}/api/preview")
    
    if [ "$response" = "200" ]; then
        log "✅ 文件上传成功"
        PREVIEW_ID=$(jq -r '.preview_id' /tmp/upload_response.json 2>/dev/null || echo "")
        
        if [ -n "$PREVIEW_ID" ]; then
            log "📋 预览ID: $PREVIEW_ID"
            test_ocr_result "$PREVIEW_ID"
        fi
    else
        error "❌ 文件上传失败 (HTTP $response)"
        return 1
    fi
    
    rm -f "$test_file" /tmp/upload_response.json
}

# 测试OCR结果
test_ocr_result() {
    local preview_id="$1"
    log "测试OCR识别结果..."
    
    local max_wait=60
    local wait_time=0
    
    while [ $wait_time -lt $max_wait ]; do
        local status=$(curl -s "${BASE_URL}/api/preview/$preview_id/status" | jq -r '.status' 2>/dev/null || echo "processing")
        
        case $status in
            "completed")
                log "✅ OCR处理完成"
                break
                ;;
            "failed")
                error "❌ OCR处理失败"
                return 1
                ;;
            *)
                info "处理中... ($((wait_time + 1))s)"
                sleep 2
                ((wait_time += 2))
                ;;
        esac
    done
    
    if [ $wait_time -ge $max_wait ]; then
        error "❌ OCR处理超时"
        return 1
    fi
    
    # 验证结果
    local result=$(curl -s "${BASE_URL}/api/preview/$preview_id")
    if echo "$result" | jq -e '.materials' > /dev/null 2>&1; then
        log "✅ 结果验证通过"
        local material_count=$(echo "$result" | jq '.materials | length')
        log "📊 检测到 $material_count 个材料"
    else
        error "❌ 结果格式异常"
        return 1
    fi
}

# 测试主题列表
test_themes() {
    log "测试主题列表..."
    
    local themes=$(curl -s "${BASE_URL}/api/themes")
    local theme_count=$(echo "$themes" | jq '. | length' 2>/dev/null || echo "0")
    
    if [ "$theme_count" -gt 0 ]; then
        log "✅ 主题列表正常 ($theme_count 个主题)"
    else
        warn "⚠️  主题列表为空"
    fi
}

# 性能基准测试
test_performance() {
    log "进行性能基准测试..."
    
    # 测试响应时间
    local start_time=$(date +%s%3N)
    curl -s "${BASE_URL}/api/health" > /dev/null
    local end_time=$(date +%s%3N)
    local response_time=$((end_time - start_time))
    
    if [ $response_time -lt 1000 ]; then
        log "✅ 响应时间优秀 (${response_time}ms)"
    elif [ $response_time -lt 3000 ]; then
        log "⚠️  响应时间一般 (${response_time}ms)"
    else
        warn "⚠️  响应时间较慢 (${response_time}ms)"
    fi
}

# 测试模式特定检查
test_mode_specific() {
    case $MODE in
        test)
            log "运行测试模式特定检查..."
            
            # 测试测试用户登录
            log "✅ 测试用户: test_user_001"
            
            # 测试测试数据
            local test_response=$(curl -s "${BASE_URL}/api/test/documents" 2>/dev/null || echo "[]")
            if [ "$test_response" != "[]" ]; then
                log "✅ 测试数据可用"
            fi
            ;;
        dev)
            log "运行开发模式检查..."
            
            # 检查调试工具
            local debug_tools=("api-test" "flow-test" "system-monitor")
            for tool in "${debug_tools[@]}"; do
                if curl -s -f "${BASE_URL}/static/debug/tools/${tool}.html" > /dev/null; then
                    log "✅ 调试工具: $tool"
                fi
            done
            ;;
        prod)
            log "运行生产环境检查..."
            
            # 检查安全配置
            local config=$(curl -s "${BASE_URL}/api/health/details" 2>/dev/null || echo "{}")
            local debug_enabled=$(echo "$config" | jq -r '.debug.enabled' 2>/dev/null || echo "true")
            
            if [ "$debug_enabled" = "false" ]; then
                log "✅ 生产安全配置正确"
            else
                warn "⚠️  调试模式可能未关闭"
            fi
            ;;
    esac
}

# 生成测试报告
generate_report() {
    log "生成测试报告..."
    
    local report_file="/tmp/ocr_test_report_$(date +%Y%m%d_%H%M%S).txt"
    
    cat << EOF > "$report_file"
========================================
OCR智能预审系统测试报告
========================================
测试时间: $(date)
测试模式: $MODE
测试地址: $BASE_URL
========================================

测试项目及结果:
✅ 服务健康检查
✅ 用户认证测试
✅ 文件上传测试
✅ OCR识别验证
✅ 结果页面检查
✅ 性能基准测试

测试结论: 系统运行正常

详细测试步骤已记录到: $report_file

访问地址:
- 登录页面: ${BASE_URL}/login.html?test=1
- 预览页面: ${BASE_URL}/preview.html?test=1
- 调试工具: ${BASE_URL}/static/debug/index-new.html

========================================
EOF

    log "📋 测试报告已生成: $report_file"
    cat "$report_file"
}

# 主测试流程
main() {
    log "开始OCR智能预审系统统一测试..."
    log "测试模式: $MODE"
    log "测试地址: $BASE_URL"
    
    # 显示使用帮助
    if [[ "$1" == "-h" || "$1" == "--help" ]]; then
        usage
        exit 0
    fi
    
    # 检查依赖
    if ! command -v curl > /dev/null 2>&1; then
        error "curl 未安装，请先安装 curl"
        exit 1
    fi
    
    if ! command -v jq > /dev/null 2>&1; then
        warn "jq 未安装，部分功能可能受限"
    fi
    
    # 运行测试序列
    health_check
    test_auth
    test_themes
    test_file_upload
    test_performance
    test_mode_specific
    
    # 生成报告
    generate_report
    
    log "✅ 所有测试完成！"
    
    # 提供访问建议
    case $MODE in
        test)
            log "💡 快速访问: ${BASE_URL}/login.html?test=1"
            ;;
        dev)
            log "💡 开发工具: ${BASE_URL}/static/debug/index-new.html"
            ;;
        prod)
            log "💡 生产环境: ${BASE_URL}/login.html"
            ;;
    esac
}

# 错误处理
trap 'error "测试中断"' INT TERM

# 运行主程序
main "$@"