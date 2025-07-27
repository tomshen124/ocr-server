#!/bin/bash

# 生产环境部署验证脚本
# 专门针对64GB/32核服务器的快速验证

echo "🚀 OCR智能预审系统 - 生产环境验证"
echo "================================================"
echo "服务器配置: 64GB内存 + 32核CPU"
echo "并发优化: 12个OCR任务并发限制"
echo "================================================"

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 检查函数
check_status() {
    if [ $? -eq 0 ]; then
        echo -e "${GREEN}✅ $1${NC}"
        return 0
    else
        echo -e "${RED}❌ $1${NC}"
        return 1
    fi
}

warn_status() {
    echo -e "${YELLOW}⚠️ $1${NC}"
}

info_status() {
    echo -e "${GREEN}ℹ️ $1${NC}"
}

# 1. 基础服务检查
echo ""
echo "1. 🔍 基础服务检查"
echo "--------------------"

# 检查端口占用
if netstat -ln | grep :31101 > /dev/null 2>&1; then
    check_status "端口31101已监听"
else
    warn_status "端口31101未监听，服务可能未启动"
fi

# 检查健康状态
HEALTH_RESPONSE=$(curl -s -w "%{http_code}" -o /tmp/health_response http://localhost:31101/api/health)
HTTP_CODE=${HEALTH_RESPONSE: -3}

if [ "$HTTP_CODE" = "200" ]; then
    check_status "健康检查API响应正常 (HTTP 200)"
    HEALTH_DATA=$(cat /tmp/health_response)
    echo "健康状态详情:"
    echo "$HEALTH_DATA" | jq . 2>/dev/null || echo "$HEALTH_DATA"
else
    warn_status "健康检查API异常 (HTTP $HTTP_CODE)"
fi

# 2. 并发控制验证
echo ""
echo "2. ⚡ 并发控制验证"
echo "--------------------"

QUEUE_RESPONSE=$(curl -s http://localhost:31101/api/queue/status)
if [ $? -eq 0 ]; then
    check_status "队列状态API响应正常"
    
    # 解析队列状态
    MAX_CONCURRENT=$(echo "$QUEUE_RESPONSE" | jq -r '.data.queue.max_concurrent_tasks' 2>/dev/null)
    AVAILABLE_SLOTS=$(echo "$QUEUE_RESPONSE" | jq -r '.data.queue.available_slots' 2>/dev/null)
    SYSTEM_LOAD=$(echo "$QUEUE_RESPONSE" | jq -r '.data.queue.system_load_percent' 2>/dev/null)
    
    if [ "$MAX_CONCURRENT" = "12" ]; then
        check_status "并发限制配置正确 (12个任务)"
    else
        warn_status "并发限制配置异常: $MAX_CONCURRENT (期望: 12)"
    fi
    
    info_status "当前可用处理槽位: $AVAILABLE_SLOTS"
    info_status "系统负载: $SYSTEM_LOAD%"
    
else
    warn_status "队列状态API无响应"
fi

# 3. 系统资源检查
echo ""
echo "3. 💻 系统资源检查"
echo "--------------------"

# CPU检查
CPU_CORES=$(nproc)
if [ "$CPU_CORES" -ge 16 ]; then
    check_status "CPU核心数充足: ${CPU_CORES}核 (推荐: 32核)"
else
    warn_status "CPU核心数偏少: ${CPU_CORES}核 (推荐: 32核)"
fi

# 内存检查
TOTAL_MEM=$(free -g | awk 'NR==2{print $2}')
AVAILABLE_MEM=$(free -g | awk 'NR==2{print $7}')

if [ "$TOTAL_MEM" -ge 32 ]; then
    check_status "总内存充足: ${TOTAL_MEM}GB (推荐: 64GB)"
else
    warn_status "总内存偏少: ${TOTAL_MEM}GB (推荐: 64GB)"
fi

if [ "$AVAILABLE_MEM" -ge 16 ]; then
    check_status "可用内存充足: ${AVAILABLE_MEM}GB"
else
    warn_status "可用内存偏少: ${AVAILABLE_MEM}GB"
fi

# 磁盘空间检查
DISK_USAGE=$(df -h / | awk 'NR==2{print $5}' | sed 's/%//')
if [ "$DISK_USAGE" -lt 80 ]; then
    check_status "磁盘空间充足: 已使用${DISK_USAGE}%"
else
    warn_status "磁盘空间紧张: 已使用${DISK_USAGE}%"
fi

# 4. 进程状态检查
echo ""
echo "4. 🔄 进程状态检查"
echo "--------------------"

OCR_PID=$(pgrep -f ocr-server)
if [ -n "$OCR_PID" ]; then
    check_status "OCR服务进程运行正常 (PID: $OCR_PID)"
    
    # 检查进程内存使用
    PROC_MEM=$(ps -p "$OCR_PID" -o rss= 2>/dev/null)
    if [ -n "$PROC_MEM" ]; then
        PROC_MEM_MB=$((PROC_MEM / 1024))
        if [ "$PROC_MEM_MB" -lt 4096 ]; then
            check_status "进程内存使用正常: ${PROC_MEM_MB}MB"
        else
            warn_status "进程内存使用较高: ${PROC_MEM_MB}MB"
        fi
    fi
else
    warn_status "未找到OCR服务进程"
fi

# 5. 配置文件检查
echo ""
echo "5. ⚙️ 配置文件检查"
echo "--------------------"

CONFIG_FILES=(
    "config/config.yaml"
    "config/config.production.yaml"
)

for config_file in "${CONFIG_FILES[@]}"; do
    if [ -f "$config_file" ]; then
        check_status "配置文件存在: $config_file"
        
        # 检查调试模式是否关闭（生产环境）
        if grep -q "enabled: false" "$config_file" 2>/dev/null; then
            check_status "调试模式已关闭 ($config_file)"
        else
            warn_status "建议关闭调试模式 ($config_file)"
        fi
    else
        warn_status "配置文件缺失: $config_file"
    fi
done

# 6. 日志文件检查
echo ""
echo "6. 📝 日志文件检查"
echo "--------------------"

LOG_DIRS=("runtime/logs" "logs")
LOG_FOUND=false

for log_dir in "${LOG_DIRS[@]}"; do
    if [ -d "$log_dir" ]; then
        LOG_COUNT=$(find "$log_dir" -name "*.log" | wc -l)
        if [ "$LOG_COUNT" -gt 0 ]; then
            check_status "日志目录存在: $log_dir (包含${LOG_COUNT}个日志文件)"
            
            # 检查最新日志文件的大小
            LATEST_LOG=$(find "$log_dir" -name "*.log" -type f -exec ls -t {} + | head -1)
            if [ -f "$LATEST_LOG" ]; then
                LOG_SIZE=$(du -h "$LATEST_LOG" | cut -f1)
                info_status "最新日志文件: $LATEST_LOG (大小: $LOG_SIZE)"
            fi
            LOG_FOUND=true
            break
        fi
    fi
done

if [ "$LOG_FOUND" = false ]; then
    warn_status "未找到日志文件目录"
fi

# 7. 性能基准测试（可选）
echo ""
echo "7. 🏃 性能基准测试"
echo "--------------------"

if command -v curl &> /dev/null; then
    echo "执行简单的响应时间测试..."
    
    # 测试健康检查接口响应时间
    START_TIME=$(date +%s%3N)
    curl -s http://localhost:31101/api/health > /dev/null
    END_TIME=$(date +%s%3N)
    RESPONSE_TIME=$((END_TIME - START_TIME))
    
    if [ "$RESPONSE_TIME" -lt 1000 ]; then
        check_status "健康检查响应时间: ${RESPONSE_TIME}ms (良好)"
    elif [ "$RESPONSE_TIME" -lt 3000 ]; then
        warn_status "健康检查响应时间: ${RESPONSE_TIME}ms (一般)"
    else
        warn_status "健康检查响应时间: ${RESPONSE_TIME}ms (偏慢)"
    fi
else
    warn_status "未安装curl，跳过响应时间测试"
fi

# 8. 总结报告
echo ""
echo "================================================"
echo "📊 部署验证总结"
echo "================================================"

echo ""
echo "🎯 关键性能指标:"
echo "- 并发处理能力: 12个OCR任务同时处理"
echo "- 内存使用预期: 48-72GB (每任务4-6GB)"
echo "- CPU利用率预期: 60-80% (高峰期)"
echo "- 响应时间预期: <30秒/请求"

echo ""
echo "🔧 生产环境管理命令:"
echo "- 查看队列状态: curl http://localhost:31101/api/queue/status"
echo "- 检查健康状态: curl http://localhost:31101/api/health"
echo "- 查看系统资源: free -h && top -bn1 | head -5"
echo "- 重启服务: systemctl restart ocr-server (或相应的启动脚本)"

echo ""
echo "📞 故障处理建议:"
echo "- 内存不足: 重启服务释放内存"
echo "- CPU过高: 检查积压任务数量"
echo "- 响应慢: 检查并发任务数和系统负载"
echo "- 磁盘满: 清理日志文件和临时文件"

echo ""
echo "✅ 部署验证完成！"
echo "如所有检查项均通过，系统已就绪投入生产使用。"