#!/bin/bash
#
# OCR服务管理脚本
# 用法: ./ocr-server.sh {start|stop|restart|status|log|build}
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

BUILD_TYPE="${BUILD_TYPE:-release}"  # 构建类型：debug 或 release
ENABLE_MONITORING="${ENABLE_MONITORING:-auto}"  # 监控模式：auto|true|false
SERVER_PORT=31101  # 默认端口，将从配置文件中读取实际端口

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
    SERVER_BIN="$SERVER_DIR/target/$BUILD_TYPE/ocr-server"
    CONFIG_FILE="$SERVER_DIR/config.yaml"
    LOG_DIR="$SERVER_DIR/logs"
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
    echo "31101"
}

# 更新SERVER_PORT为配置文件中的实际端口
SERVER_PORT=$(get_port_from_config)

# 检查监控配置
check_monitoring_config() {
    if [ -f "$CONFIG_FILE" ]; then
        # 检查配置文件中是否启用了监控
        if command -v yq >/dev/null 2>&1; then
            local monitoring_enabled=$(yq eval '.monitoring.enabled' "$CONFIG_FILE" 2>/dev/null)
            if [ "$monitoring_enabled" = "true" ]; then
                return 0  # 监控已启用
            fi
        else
            # 如果没有yq，使用grep简单检查
            if grep -A 2 "monitoring:" "$CONFIG_FILE" | grep -q "enabled: true"; then
                return 0  # 监控已启用
            fi
        fi
    fi
    return 1  # 监控未启用
}

# 确定是否使用监控功能
should_enable_monitoring() {
    case "$ENABLE_MONITORING" in
        "true")
            echo "true"
            ;;
        "false")
            echo "false"
            ;;
        "auto"|*)
            if check_monitoring_config; then
                echo "true"
            else
                echo "false"
            fi
            ;;
    esac
}

# 检查服务是否正在运行
check_status() {
    if [ -f $PID_FILE ]; then
        PID=$(cat $PID_FILE)
        if ps -p $PID > /dev/null; then
            echo "$SERVER_NAME 正在运行，PID: $PID"
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

# 构建项目
build_project() {
    echo "正在构建 $SERVER_NAME..."
    
    # 检查是否在开发环境
    if [ ! -f "$SERVER_DIR/Cargo.toml" ]; then
        echo "错误: 当前环境不支持构建功能"
        echo "构建功能仅在开发环境中可用（需要 Cargo.toml 文件）"
        echo "生产环境请使用预编译的二进制文件"
        return 1
    fi
    
    cd $SERVER_DIR

    # 检查是否需要启用监控功能
    local enable_monitoring=$(should_enable_monitoring)
    local build_features=""

    if [ "$enable_monitoring" = "true" ]; then
        echo "检测到监控功能已启用，将包含监控特性..."
        build_features="--features monitoring"
    fi

    if [ "$BUILD_TYPE" = "release" ]; then
        echo "使用 release 模式构建..."
        if [ -n "$build_features" ]; then
            echo "构建命令: cargo build --release $build_features"
            cargo build --release $build_features
        else
            cargo build --release
        fi
    else
        echo "使用 debug 模式构建..."
        if [ -n "$build_features" ]; then
            echo "构建命令: cargo build $build_features"
            cargo build $build_features
        else
            cargo build
        fi
    fi

    if [ $? -eq 0 ]; then
        echo "构建成功: $SERVER_BIN"

        # 显示构建信息
        echo ""
        echo "=== 构建信息 ==="
        if [ "$enable_monitoring" = "true" ]; then
            echo "✓ 已包含集成监控功能"
            echo "✓ 监控页面: http://localhost:31101/static/monitor.html"
        else
            echo "ℹ 未包含集成监控功能"
            echo "如需启用监控: $0 enable-monitoring"
        fi

        # 显示构建产物信息
        if [ -f "$SERVER_BIN" ]; then
            local file_size=$(du -h "$SERVER_BIN" | cut -f1)
            echo "✓ 可执行文件大小: $file_size"
        fi

        # 检查静态文件
        local static_dir="$SERVER_DIR/target/$BUILD_TYPE/static"
        if [ -d "$static_dir" ]; then
            echo "✓ 静态文件已复制到构建目录"
            if [ -f "$static_dir/monitor.html" ]; then
                echo "✓ 监控页面文件已包含"
            fi
        fi

        # 检查配置文件
        local config_target="$SERVER_DIR/target/$BUILD_TYPE/config.yaml"
        if [ -f "$config_target" ]; then
            echo "✓ 配置文件已复制到构建目录"
        fi

        echo ""
        echo "构建完成！可以使用以下命令启动服务:"
        echo "  $0 start"

    else
        echo "构建失败"
        echo "请检查错误信息并修复后重试"
        return 1
    fi
}

# 启动服务
start_server() {
    echo "启动 $SERVER_NAME..."

    # 检查可执行文件
    if [ ! -f $SERVER_BIN ]; then
        echo "错误: 服务程序不存在 - $SERVER_BIN"
        echo "请先运行构建: $0 build"
        return 1
    fi

    # 检查端口占用
    if lsof -Pi :$SERVER_PORT -sTCP:LISTEN -t >/dev/null 2>&1; then
        echo "端口 $SERVER_PORT 已被占用"
        return 1
    fi

    # 确保日志目录存在
    mkdir -p $LOG_DIR

    # 启动服务（保留程序内部日志，仅将stdout/stderr重定向到shell日志）
    cd "$SERVER_DIR"
    nohup $SERVER_BIN >>$LOG_FILE 2>&1 &
    echo $! > $PID_FILE

    # 等待启动
    sleep 3

    if ps -p $(cat $PID_FILE) > /dev/null; then
        echo "$SERVER_NAME 启动成功 (PID: $(cat $PID_FILE))"
        echo "服务地址: http://localhost:$SERVER_PORT"
        echo "监控页面: http://localhost:$SERVER_PORT/static/monitor.html"
        echo "程序日志: $LOG_DIR/ocr.$(date +%Y-%m-%d)"
        echo ""
        echo "使用以下命令查看日志:"
        echo "  $0 log     # 查看程序内部日志"
        echo "  $0 status  # 查看服务状态"
    else
        echo "$SERVER_NAME 启动失败，请检查配置和端口占用"
        echo "程序日志: $LOG_DIR/ocr.$(date +%Y-%m-%d)"
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
    local old_shell_log="$LOG_DIR/ocr-server.log"
    
    echo "=== OCR服务日志查看 ==="
    
    # 优先查看程序内部日志
    if [ -f "$program_log" ]; then
        echo "查看程序日志: $program_log"
        echo "=== 最近100行日志 ==="
        tail -n 100 "$program_log"
        echo ""
        echo "=== 实时日志 (Ctrl+C 退出) ==="
        tail -f "$program_log"
    elif [ -f "$old_shell_log" ]; then
        echo "查看shell重定向日志: $old_shell_log"
        echo "=== 最近100行日志 ==="
        tail -n 100 "$old_shell_log"
        echo ""
        echo "=== 实时日志 (Ctrl+C 退出) ==="
        tail -f "$old_shell_log"
    else
        echo "日志文件不存在"
        echo "可能的日志文件位置："
        echo "  - $program_log (程序内部日志)"
        echo "  - $old_shell_log (shell重定向日志)"
        echo ""
        echo "请检查服务是否已启动: $0 status"
    fi
}

# 检查监控服务状态
check_monitor_service() {
    echo "=== 检查监控服务状态 ==="

    # 检查集成监控
    local enable_monitoring=$(should_enable_monitoring)
    if [ "$enable_monitoring" = "true" ]; then
        echo "✓ 集成监控功能已配置"
        if check_status >/dev/null 2>&1; then
            echo "✓ 主服务运行中，集成监控应该可用"
            echo "监控页面: http://localhost:31101/static/monitor.html"
        else
            echo "⚠ 主服务未运行，集成监控不可用"
        fi
    else
        echo "ℹ 集成监控功能未启用"
    fi

    echo ""

    # 检查独立监控服务
    if lsof -Pi :8964 -sTCP:LISTEN -t >/dev/null 2>&1; then
        echo "✓ 独立监控服务正在运行 (端口: 8964)"
        echo "监控页面: http://localhost:8964"
    else
        echo "ℹ 独立监控服务未运行"
        echo "如需启动独立监控: ./ocr-monitor.sh start"
    fi
}

# 启用监控功能
enable_monitoring() {
    echo "正在启用集成监控功能..."

    if [ ! -f "$CONFIG_FILE" ]; then
        echo "错误: 配置文件不存在: $CONFIG_FILE"
        return 1
    fi

    # 备份配置文件
    cp "$CONFIG_FILE" "$CONFIG_FILE.bak.$(date +%Y%m%d%H%M%S)"
    echo "已备份配置文件: $CONFIG_FILE.bak.$(date +%Y%m%d%H%M%S)"

    # 修改配置文件
    if command -v yq >/dev/null 2>&1; then
        yq eval '.monitoring.enabled = true' -i "$CONFIG_FILE"
        echo "✓ 已启用监控功能 (使用yq修改)"
    else
        # 如果没有yq，使用sed简单替换
        if grep -q "enabled: false" "$CONFIG_FILE"; then
            sed -i 's/enabled: false/enabled: true/g' "$CONFIG_FILE"
            echo "✓ 已启用监控功能 (使用sed修改)"
        else
            echo "⚠ 无法自动修改配置文件，请手动编辑 $CONFIG_FILE"
            echo "将 monitoring.enabled 设置为 true"
            return 1
        fi
    fi

    echo "请重新构建并启动服务以应用监控功能:"
    echo "  $0 build"
    echo "  $0 restart"
}

# 显示帮助信息
show_help() {
    echo "OCR服务管理脚本"
    echo ""
    echo "用法: $0 {start|stop|restart|status|log|build|monitor|enable-monitoring|help}"
    echo ""
    echo "命令说明:"
    echo "  start            - 启动服务"
    echo "  stop             - 停止服务"
    echo "  restart          - 重启服务"
    echo "  status           - 查看服务状态"
    echo "  log              - 查看服务日志"
    echo "  build            - 构建项目"
    echo "  monitor          - 检查监控服务状态"
    echo "  enable-monitoring - 启用集成监控功能"
    echo "  help             - 显示此帮助信息"
    echo ""
    echo "环境变量:"
    echo "  BUILD_TYPE       - 构建类型 (debug|release)，默认: release"
    echo "  ENABLE_MONITORING - 监控模式 (auto|true|false)，默认: auto"
    echo ""
    echo "监控功能:"
    echo "  集成监控 - 在主服务中集成监控功能 (推荐)"
    echo "    配置: 编辑 $CONFIG_FILE 设置 monitoring.enabled: true"
    echo "    访问: http://localhost:31101/static/monitor.html"
    echo ""
    echo "  独立监控 - 运行独立的监控服务"
    echo "    启动: ./ocr-monitor.sh start"
    echo "    访问: http://localhost:8964"
    echo ""
    echo "示例:"
    echo "  $0 build                         # 构建项目"
    echo "  $0 start                         # 启动服务"
    echo "  $0 enable-monitoring             # 启用集成监控"
    echo "  $0 monitor                       # 检查监控状态"
    echo "  BUILD_TYPE=debug $0 start        # 使用debug版本启动"
    echo "  ENABLE_MONITORING=true $0 build  # 强制启用监控功能构建"
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
    build)
        build_project
        ;;
    monitor)
        check_monitor_service
        ;;
    enable-monitoring)
        enable_monitoring
        ;;
    help|--help|-h)
        show_help
        ;;
    *)
        echo "用法: $0 {start|stop|restart|status|log|build|monitor|enable-monitoring|help}"
        echo "使用 '$0 help' 查看详细帮助"
        exit 1
        ;;
esac

exit 0
