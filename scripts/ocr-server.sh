#!/bin/bash
#
# OCR服务管理脚本
# 用法: ./ocr-server.sh {start|stop|restart|status|log}
#

# 配置项 - 可根据实际部署情况修改
SERVER_NAME="ocr-server"

# 智能检测项目根目录
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ -f "$SCRIPT_DIR/bin/ocr-server" ]; then
    # 脚本在项目根目录
    SERVER_DIR="$SCRIPT_DIR"
elif [ -f "$SCRIPT_DIR/../bin/ocr-server" ]; then
    # 脚本在 scripts/ 子目录，需要回到上级目录
    SERVER_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
elif [ -f "$SCRIPT_DIR/ocr-server" ]; then
    # 生产环境旧目录结构（脚本在根目录）
    SERVER_DIR="$SCRIPT_DIR"
elif [ -f "$SCRIPT_DIR/target/release/ocr-server" ]; then
    # 开发环境（脚本在项目根目录）
    SERVER_DIR="$SCRIPT_DIR"
else
    # 默认使用脚本所在目录
    SERVER_DIR="$SCRIPT_DIR"
fi

SERVER_PORT=8964  # 默认端口，将从配置文件中读取实际端口

# 根据环境设置路径
if [ -f "$SERVER_DIR/bin/ocr-server" ]; then
    # 生产环境新目录结构
    SERVER_BIN="$SERVER_DIR/bin/ocr-server"
    CONFIG_FILE="$SERVER_DIR/config/config.yaml"
    LOG_DIR="$SERVER_DIR/runtime/logs"
elif [ -f "$SERVER_DIR/ocr-server" ]; then
    # 生产环境旧目录结构（向后兼容）
    SERVER_BIN="$SERVER_DIR/ocr-server"
    CONFIG_FILE="$SERVER_DIR/config.yaml"
    LOG_DIR="$SERVER_DIR/logs"
else
    # 开发环境
    SERVER_BIN="$SERVER_DIR/target/release/ocr-server"
    CONFIG_FILE="$SERVER_DIR/config/config.yaml"
    LOG_DIR="$SERVER_DIR/runtime/logs"
fi

PID_FILE="/tmp/ocr-server.pid"
LOG_FILE="$LOG_DIR/ocr-server.log"

# 确保日志目录存在
mkdir -p $LOG_DIR

# 从配置文件读取实际端口
get_port_from_config() {
    if [ -f "$CONFIG_FILE" ]; then
        # 尝试使用yq读取端口
        if command -v yq >/dev/null 2>&1; then
            local port=$(yq eval '.port' "$CONFIG_FILE" 2>/dev/null)
            if [ "$port" != "null" ] && [ -n "$port" ]; then
                echo "$port"
                return
            fi
        fi
        
        # 使用grep和awk作为备选方案
        local port=$(grep "^port:" "$CONFIG_FILE" 2>/dev/null | awk '{print $2}' | tr -d ' ')
        if [ -n "$port" ] && [ "$port" -eq "$port" ] 2>/dev/null; then
            echo "$port"
            return
        fi
    fi
    
    # 如果无法读取，返回默认端口
    echo "8964"
}

# 更新SERVER_PORT为配置文件中的实际端口
SERVER_PORT=$(get_port_from_config)

# 检查服务是否正在运行
check_status() {
    if [ -f $PID_FILE ]; then
        PID=$(cat $PID_FILE)
        if ps -p $PID > /dev/null; then
            echo "$SERVER_NAME 正在运行，PID: $PID"
            echo "服务地址: http://localhost:$SERVER_PORT"
            return 0
        else
            echo "$SERVER_NAME 未运行 (PID文件存在但进程不存在)"
            rm -f $PID_FILE
            return 1
        fi
    else
        echo "$SERVER_NAME 未运行"
        return 1
    fi
}

# 启动服务
start_server() {
    echo "启动 $SERVER_NAME..."

    # 检查可执行文件
    if [ ! -f $SERVER_BIN ]; then
        echo "错误: 服务程序不存在 - $SERVER_BIN"
        echo "请确保程序已编译或部署到正确位置"
        return 1
    fi

    # 检查是否已在运行
    if check_status >/dev/null 2>&1; then
        echo "$SERVER_NAME 已在运行"
        return 0
    fi

    # 检查端口占用
    if lsof -Pi :$SERVER_PORT -sTCP:LISTEN -t >/dev/null 2>&1; then
        echo "端口 $SERVER_PORT 已被占用"
        echo "请检查是否有其他服务占用该端口，或修改配置文件中的端口设置"
        return 1
    fi

    # 确保日志目录存在
    mkdir -p $LOG_DIR

    # 启动服务
    cd "$SERVER_DIR"
    nohup $SERVER_BIN >>$LOG_FILE 2>&1 &
    echo $! > $PID_FILE

    # 等待启动
    sleep 3

    if ps -p $(cat $PID_FILE) > /dev/null; then
        echo "$SERVER_NAME 启动成功 (PID: $(cat $PID_FILE))"
        echo "服务地址: http://localhost:$SERVER_PORT"
        echo "健康检查: http://localhost:$SERVER_PORT/api/health"
        echo ""
        echo "使用以下命令管理服务:"
        echo "  $0 status  # 查看服务状态"
        echo "  $0 log     # 查看服务日志"
        echo "  $0 stop    # 停止服务"
    else
        echo "$SERVER_NAME 启动失败，请检查日志"
        echo "日志位置: $LOG_FILE"
        rm -f $PID_FILE
        return 1
    fi
}

# 停止服务
stop_server() {
    echo "正在停止 $SERVER_NAME..."

    if [ -f $PID_FILE ]; then
        PID=$(cat $PID_FILE)
        if ps -p $PID > /dev/null; then
            echo "发送终止信号到进程 $PID..."
            kill $PID

            # 等待进程终止
            TIMEOUT=30
            while ps -p $PID > /dev/null && [ $TIMEOUT -gt 0 ]; do
                sleep 1
                TIMEOUT=$((TIMEOUT-1))
            done

            if ps -p $PID > /dev/null; then
                echo "进程未在30秒内终止，强制终止..."
                kill -9 $PID
                sleep 1
            fi

            if ps -p $PID > /dev/null; then
                echo "无法终止进程 $PID"
                return 1
            else
                echo "$SERVER_NAME 已停止"
                rm -f $PID_FILE
            fi
        else
            echo "$SERVER_NAME 未运行 (PID文件存在但进程不存在)"
            rm -f $PID_FILE
        fi
    else
        echo "$SERVER_NAME 未运行"
    fi
}

# 重启服务
restart_server() {
    echo "正在重启 $SERVER_NAME..."
    stop_server
    sleep 2
    start_server
}

# 查看日志
view_log() {
    local program_log="$LOG_DIR/ocr.$(date +%Y-%m-%d)"
    local shell_log="$LOG_FILE"
    
    echo "=== OCR服务日志查看 ==="
    
    # 优先查看程序内部日志
    if [ -f "$program_log" ]; then
        echo "查看程序日志: $program_log"
        echo "=== 最近100行日志 ==="
        tail -n 100 "$program_log"
        echo ""
        echo "=== 实时日志 (Ctrl+C 退出) ==="
        tail -f "$program_log"
    elif [ -f "$shell_log" ]; then
        echo "查看启动日志: $shell_log"
        echo "=== 最近100行日志 ==="
        tail -n 100 "$shell_log"
        echo ""
        echo "=== 实时日志 (Ctrl+C 退出) ==="
        tail -f "$shell_log"
    else
        echo "日志文件不存在"
        echo "可能的日志文件位置："
        echo "  - $program_log (程序内部日志)"
        echo "  - $shell_log (启动脚本日志)"
        echo ""
        echo "请检查服务是否已启动: $0 status"
    fi
}

# 检查故障转移状态
check_failover_status() {
    echo "=== OCR系统故障转移状态检查 ==="
    echo ""
    
    # 检查服务是否运行
    if ! check_status >/dev/null 2>&1; then
        echo "❌ 服务未运行，无法检查故障转移状态"
        echo "请先启动服务：$0 start"
        return 1
    fi
    
    # 检查是否安装了jq工具
    if ! command -v jq >/dev/null 2>&1; then
        echo "⚠️ 未安装jq工具，显示原始JSON数据"
        echo ""
        echo "🔄 故障转移状态:"
        curl -s "http://localhost:$SERVER_PORT/api/failover/status" 2>/dev/null || {
            echo "❌ 无法获取故障转移状态，请检查服务是否正常运行"
            return 1
        }
        return 0
    fi
    
    echo "🔄 故障转移总体状态:"
    local failover_status=$(curl -s "http://localhost:$SERVER_PORT/api/failover/status" 2>/dev/null)
    if [ -z "$failover_status" ]; then
        echo "❌ 无法获取故障转移状态，请检查服务是否正常运行"
        return 1
    fi
    
    # 解析并显示关键状态信息
    echo "$failover_status" | jq -r '.data | 
        "  整体健康状态: " + (.overall_health // "unknown") +
        "\n  数据库状态: " + (.database.current_state // "unknown") + " (" + (if .database.is_using_primary then "主数据库" else "备用SQLite" end) + ")" +
        "\n  存储状态: " + (.storage.current_state // "unknown") + " (" + (if .storage.is_using_primary then "OSS存储" else "本地存储" end) + ")"
    ' 2>/dev/null || {
        echo "⚠️ JSON解析失败，显示原始数据:"
        echo "$failover_status"
    }
    
    echo ""
    echo "📊 详细状态查询:"
    echo "  数据库详情: curl http://localhost:$SERVER_PORT/api/failover/database"
    echo "  存储详情: curl http://localhost:$SERVER_PORT/api/failover/storage"
    echo "  系统监控: $0 monitor"
}

# 显示系统监控状态
show_monitoring_status() {
    echo "=== OCR系统监控状态 ==="
    echo ""
    
    # 检查服务是否运行
    if ! check_status >/dev/null 2>&1; then
        echo "❌ 服务未运行，无法获取监控状态"
        echo "请先启动服务：$0 start"
        return 1
    fi
    
    # 基础健康检查
    echo "🏥 健康检查:"
    local health_status=$(curl -s "http://localhost:$SERVER_PORT/api/health" 2>/dev/null)
    if [ -n "$health_status" ]; then
        if command -v jq >/dev/null 2>&1; then
            echo "$health_status" | jq -r '"  服务状态: " + (.status // "unknown")' 2>/dev/null || echo "  $health_status"
        else
            echo "  $health_status"
        fi
    else
        echo "  ❌ 无法获取健康状态"
        return 1
    fi
    
    echo ""
    echo "⚡ OCR处理队列:"
    local queue_status=$(curl -s "http://localhost:$SERVER_PORT/api/queue/status" 2>/dev/null)
    if [ -n "$queue_status" ] && command -v jq >/dev/null 2>&1; then
        echo "$queue_status" | jq -r '.data | 
            "  可用槽位: " + (.available_slots | tostring) + "/" + (.max_concurrent | tostring) +
            "\n  处理中: " + (.processing_count | tostring) +
            "\n  等待队列: " + (.queue_length | tostring)
        ' 2>/dev/null || echo "  原始数据: $queue_status"
    else
        echo "  ⚠️ 无法获取队列状态或未安装jq工具"
    fi
    
    echo ""
    echo "📊 系统资源监控:"
    local monitor_status=$(curl -s "http://localhost:$SERVER_PORT/api/monitoring/status" 2>/dev/null)
    if [ -n "$monitor_status" ] && command -v jq >/dev/null 2>&1; then
        echo "$monitor_status" | jq -r '.data | 
            "  CPU使用率: " + (.cpu_usage | tostring) + "%" +
            "\n  内存使用率: " + (.memory_usage | tostring) + "%" +
            "\n  磁盘使用率: " + (.disk_usage | tostring) + "%"
        ' 2>/dev/null || echo "  原始数据: $monitor_status"
    else
        echo "  ⚠️ 无法获取资源监控数据（可能未启用monitoring特性）"
    fi
    
    echo ""
    echo "🔍 更多监控选项:"
    echo "  故障转移状态: $0 failover"
    echo "  实时日志: $0 log"
    echo "  详细健康检查: curl http://localhost:$SERVER_PORT/api/health/details"
}
# 显示帮助信息
show_help() {
    echo "OCR服务管理脚本 - v1.3.4 (支持故障转移监控)"
    echo ""
    echo "用法: $0 {start|stop|restart|status|log|failover|monitor|help}"
    echo ""
    echo "命令说明:"
    echo "  start    - 启动服务"
    echo "  stop     - 停止服务"
    echo "  restart  - 重启服务"
    echo "  status   - 查看服务状态"
    echo "  log      - 查看服务日志"
    echo "  failover - 查看故障转移状态 (新增)"
    echo "  monitor  - 系统监控和性能状态 (新增)"
    echo "  help     - 显示此帮助信息"
    echo ""
    echo "新特性 (v1.3.4):"
    echo "  🔄 智能故障转移: 数据库和OSS故障时自动切换至备用方案"
    echo "  📊 系统监控: 实时CPU/内存/磁盘使用状态监控"
    echo "  ⚡ OCR队列状态: 实时并发处理状态监控"
    echo "  🏥 健康检查: 增强型服务健康状态检查"
    echo ""
    echo "文件路径:"
    echo "  程序文件: $SERVER_BIN"
    echo "  配置文件: $CONFIG_FILE"
    echo "  日志目录: $LOG_DIR"
    echo "  PID文件:  $PID_FILE"
    echo ""
    echo "示例:"
    echo "  $0 start     # 启动服务"
    echo "  $0 status    # 查看状态"
    echo "  $0 failover  # 故障转移状态"
    echo "  $0 monitor   # 系统监控状态"
    echo "  $0 log       # 查看日志"
    echo "  $0 restart   # 重启服务"
}

# 根据命令行参数执行相应操作
case "$1" in
    start)
        start_server
        ;;
    stop)
        stop_server
        ;;
    restart)
        restart_server
        ;;
    status)
        check_status
        ;;
    log)
        view_log
        ;;
    "failover")
        check_failover_status
        ;;
    "monitor")
        show_monitoring_status
        ;;
    "help"|-help|-h)
        show_help
        ;;
    *)
        echo "用法: $0 {start|stop|restart|status|log|failover|monitor|help}"
        echo "使用 '$0 help' 查看详细帮助"
        exit 1
        ;;
esac

exit 0