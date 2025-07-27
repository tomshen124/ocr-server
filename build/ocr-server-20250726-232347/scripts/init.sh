#!/bin/bash

# OCR智能预审系统初始化脚本
# 用于新环境部署时的系统初始化

set -e

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 配置变量
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
RUNTIME_DIR="$PROJECT_DIR/runtime"
CONFIG_DIR="$PROJECT_DIR/config"

# 日志函数
log() {
    echo -e "${GREEN}[$(date +'%Y-%m-%d %H:%M:%S')] $1${NC}"
}

warn() {
    echo -e "${YELLOW}[$(date +'%Y-%m-%d %H:%M:%S')] WARNING: $1${NC}"
}

error() {
    echo -e "${RED}[$(date +'%Y-%m-%d %H:%M:%S')] ERROR: $1${NC}"
    exit 1
}

# 检查root权限
check_root() {
    if [[ $EUID -eq 0 ]]; then
        warn "不建议使用root用户运行此服务"
    fi
}

# 检查系统依赖
check_dependencies() {
    log "检查系统依赖..."
    
    local missing_deps=()
    
    # 检查常用工具
    command -v curl >/dev/null 2>&1 || missing_deps+=("curl")
    command -v wget >/dev/null 2>&1 || missing_deps+=("wget")
    command -v tar >/dev/null 2>&1 || missing_deps+=("tar")
    command -v sqlite3 >/dev/null 2>&1 || missing_deps+=("sqlite3")
    
    # 检查PDF生成工具
    if ! command -v wkhtmltopdf >/dev/null 2>&1; then
        warn "wkhtmltopdf未安装，PDF报告功能将不可用"
        missing_deps+=("wkhtmltopdf")
    fi
    
    if [ ${#missing_deps[@]} -gt 0 ]; then
        error "缺少依赖: ${missing_deps[*]}\n请使用以下命令安装:\nUbuntu/Debian: sudo apt install ${missing_deps[*]}\nCentOS/RHEL: sudo yum install ${missing_deps[*]}"
    fi
    
    log "系统依赖检查完成"
}

# 创建必要目录
create_directories() {
    log "创建运行时目录..."
    
    local dirs=(
        "$RUNTIME_DIR/logs"
        "$RUNTIME_DIR/data"
        "$RUNTIME_DIR/cache"
        "$RUNTIME_DIR/temp"
        "$RUNTIME_DIR/preview"
        "$RUNTIME_DIR/preview_mappings"
        "$RUNTIME_DIR/storage"
    )
    
    for dir in "${dirs[@]}"; do
        if [[ ! -d "$dir" ]]; then
            mkdir -p "$dir"
            log "创建目录: $dir"
        fi
    done
    
    log "目录创建完成"
}

# 初始化数据库
init_database() {
    log "初始化数据库..."
    
    local db_file="$RUNTIME_DIR/data/ocr.db"
    local init_sql="$CONFIG_DIR/schema.sql"
    
    if [[ ! -f "$init_sql" ]]; then
        cat > "$init_sql" << 'EOF'
-- OCR系统数据库初始化脚本

-- 用户会话表
CREATE TABLE IF NOT EXISTS user_sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    username TEXT NOT NULL,
    email TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    expires_at DATETIME NOT NULL,
    last_activity DATETIME DEFAULT CURRENT_TIMESTAMP,
    ip_address TEXT,
    user_agent TEXT
);

-- 预览任务表
CREATE TABLE IF NOT EXISTS preview_tasks (
    id TEXT PRIMARY KEY,
    filename TEXT NOT NULL,
    file_size INTEGER,
    file_hash TEXT,
    theme_id TEXT DEFAULT 'theme_001',
    matter_type TEXT,
    status TEXT DEFAULT 'pending',
    progress INTEGER DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    completed_at DATETIME,
    user_id TEXT,
    ocr_result TEXT,
    evaluation_result TEXT,
    error_message TEXT,
    processing_time_ms INTEGER
);

-- 第三方API调用记录
CREATE TABLE IF NOT EXISTS api_calls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    client_id TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    method TEXT NOT NULL,
    status_code INTEGER,
    response_time_ms INTEGER,
    request_size INTEGER,
    response_size INTEGER,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    ip_address TEXT,
    user_agent TEXT
);

-- 系统配置表
CREATE TABLE IF NOT EXISTS system_config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    description TEXT,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- 初始化系统配置
INSERT OR IGNORE INTO system_config (key, value, description) VALUES
('version', '1.3.0', '系统版本'),
('max_concurrent_ocr', '4', '最大并发OCR处理数'),
('rate_limit_per_minute', '100', '每分钟API调用限制'),
('session_timeout_minutes', '60', '会话超时时间');

-- 创建索引
CREATE INDEX IF NOT EXISTS idx_preview_tasks_status ON preview_tasks(status);
CREATE INDEX IF NOT EXISTS idx_preview_tasks_user_id ON preview_tasks(user_id);
CREATE INDEX IF NOT EXISTS idx_preview_tasks_created_at ON preview_tasks(created_at);
CREATE INDEX IF NOT EXISTS idx_user_sessions_expires_at ON user_sessions(expires_at);
CREATE INDEX IF NOT EXISTS idx_api_calls_client_id ON api_calls(client_id);
CREATE INDEX IF NOT EXISTS idx_api_calls_created_at ON api_calls(created_at);
EOF
    fi
    
    if [[ ! -f "$db_file" ]]; then
        sqlite3 "$db_file" < "$init_sql"
        log "数据库初始化完成: $db_file"
    else
        log "数据库已存在: $db_file"
    fi
}

# 检查OCR模型完整性
check_ocr_models() {
    log "检查OCR模型..."
    
    local models_dir="$PROJECT_DIR/ocr/models"
    local required_models=(
        "ch_PP-OCRv3_det_infer"
        "ch_PP-OCRv3_rec_infer"
        "ch_ppocr_mobile_v2.0_cls_infer"
        "en_PP-OCRv3_rec_infer"
        "japan_PP-OCRv3_rec_infer"
        "korean_PP-OCRv3_rec_infer"
        "cyrillic_PP-OCRv3_rec_infer"
    )
    
    local missing_models=()
    
    for model in "${required_models[@]}"; do
        local model_path="$models_dir/$model"
        if [[ ! -d "$model_path" ]] || [[ ! -f "$model_path/inference.pdmodel" ]]; then
            missing_models+=("$model")
        fi
    done
    
    if [ ${#missing_models[@]} -gt 0 ]; then
        warn "缺少以下OCR模型: ${missing_models[*]}"
        log "请运行: ./scripts/download-models.sh 下载缺失模型"
    else
        log "所有OCR模型检查通过"
    fi
}

# 检查OCR引擎可执行性
check_ocr_engine() {
    log "检查OCR引擎..."
    
    local engine_path="$PROJECT_DIR/ocr/PaddleOCR-json"
    
    if [[ ! -f "$engine_path" ]]; then
        error "OCR引擎未找到: $engine_path"
    fi
    
    if [[ ! -x "$engine_path" ]]; then
        chmod +x "$engine_path"
        log "设置OCR引擎执行权限"
    fi
    
    # 测试OCR引擎
    if ! "$engine_path" --help >/dev/null 2>&1; then
        error "OCR引擎无法执行，请检查依赖库"
    fi
    
    log "OCR引擎检查通过"
}

# 检查PDF生成工具
check_pdf_tools() {
    log "检查PDF生成工具..."
    
    if command -v wkhtmltopdf >/dev/null 2>&1; then
        local version=$(wkhtmltopdf --version 2>/dev/null | head -1)
        log "PDF工具已安装: $version"
    else
        warn "wkhtmltopdf未安装，PDF报告功能将不可用"
        warn "安装命令:"
        warn "  Ubuntu/Debian: sudo apt install wkhtmltopdf"
        warn "  CentOS/RHEL: sudo yum install wkhtmltopdf"
    fi
}

# 设置文件权限
set_permissions() {
    log "设置文件权限..."
    
    # 设置运行时目录权限
    chmod 755 "$RUNTIME_DIR"
    chmod -R 755 "$RUNTIME_DIR/logs"
    chmod -R 755 "$RUNTIME_DIR/data"
    chmod -R 755 "$RUNTIME_DIR/cache"
    chmod -R 755 "$RUNTIME_DIR/temp"
    chmod -R 755 "$RUNTIME_DIR/preview"
    chmod -R 755 "$RUNTIME_DIR/storage"
    
    # 设置OCR引擎权限
    chmod +x "$PROJECT_DIR/ocr/PaddleOCR-json"
    
    log "文件权限设置完成"
}

# 验证配置文件
validate_config() {
    log "验证配置文件..."
    
    local config_files=(
        "$CONFIG_DIR/config.yaml"
        "$CONFIG_DIR/config.yaml.prod"
        "$CONFIG_DIR/rules"
        "$CONFIG_DIR/mappings"
    )
    
    for config_file in "${config_files[@]}"; do
        if [[ ! -e "$config_file" ]]; then
            warn "配置文件不存在: $config_file"
        fi
    done
    
    log "配置文件验证完成"
}

# 创建服务启动脚本
create_service_script() {
    log "创建服务启动脚本..."
    
    cat > "$PROJECT_DIR/start.sh" << 'EOF'
#!/bin/bash
# OCR服务启动脚本

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# 检查环境
if [[ ! -f "./ocr-server" ]]; then
    echo "错误: OCR服务可执行文件未找到，请先运行构建脚本"
    exit 1
fi

# 启动服务
./ocr-server
EOF
    
    chmod +x "$PROJECT_DIR/start.sh"
    log "服务启动脚本创建完成"
}

# 生成环境检查报告
generate_report() {
    local report_file="$RUNTIME_DIR/init-report.txt"
    
    cat > "$report_file" << EOF
OCR智能预审系统初始化报告
================================
生成时间: $(date)
系统信息: $(uname -a)

目录结构:
- 项目目录: $PROJECT_DIR
- 运行时目录: $RUNTIME_DIR
- 配置目录: $CONFIG_DIR

检查项状态:
$(command -v sqlite3 >/dev/null 2>&1 && echo "✓ SQLite3已安装" || echo "✗ SQLite3未安装")
$(command -v wkhtmltopdf >/dev/null 2>&1 && echo "✓ wkhtmltopdf已安装" || echo "✗ wkhtmltopdf未安装")
$(test -x "$PROJECT_DIR/ocr/PaddleOCR-json" && echo "✓ OCR引擎可执行" || echo "✗ OCR引擎不可执行")
$(test -f "$RUNTIME_DIR/data/ocr.db" && echo "✓ 数据库已初始化" || echo "✗ 数据库未初始化")

下一步操作:
1. 检查配置文件: $CONFIG_DIR/config.yaml
2. 启动服务: ./scripts/ocr-server.sh start
3. 查看日志: tail -f $RUNTIME_DIR/logs/ocr-server.log

服务状态检查:
- 健康检查: curl http://localhost:31101/api/health
- 版本信息: curl http://localhost:31101/api/version

初始化完成！
EOF
    
    log "初始化报告已生成: $report_file"
}

# 主函数
main() {
    log "开始OCR智能预审系统初始化..."
    
    check_root
    check_dependencies
    create_directories
    init_database
    check_ocr_models
    check_ocr_engine
    check_pdf_tools
    set_permissions
    validate_config
    create_service_script
    generate_report
    
    log "系统初始化完成！"
    log "请查看初始化报告: $RUNTIME_DIR/init-report.txt"
    log "下一步: 编辑配置文件并启动服务"
}

# 处理命令行参数
case "${1:-}" in
    --help|-h)
        echo "OCR智能预审系统初始化脚本"
        echo "用法: $0 [选项]"
        echo ""
        echo "选项:"
        echo "  --help, -h     显示帮助信息"
        echo "  --check        仅检查环境，不执行初始化"
        echo ""
        echo "此脚本用于初始化OCR智能预审系统的运行环境，"
        echo "包括目录创建、数据库初始化、依赖检查等。"
        exit 0
        ;;
    --check)
        log "执行环境检查..."
        check_dependencies
        check_ocr_models
        check_ocr_engine
        check_pdf_tools
        log "环境检查完成"
        exit 0
        ;;
    *)
        main
        ;;
esac

}

log_warning() {
    echo -e "${YELLOW}[!]${NC} $1" | tee -a "$INIT_LOG"
}

log_error() {
    echo -e "${RED}[✗]${NC} $1" | tee -a "$INIT_LOG"
    HAS_ERROR=true
}

log_step() {
    echo -e "\n${PURPLE}=== $1 ===${NC}" | tee -a "$INIT_LOG"
}

# 显示欢迎信息
show_welcome() {
    clear
    cat << 'EOF'
    ____  ____________     ____        __       _____ _____             __         
   / __ \/ ____/ ____/    /  _/____   / /____  / / (_) ___  ___  ____/ /_        
  / / / / /   / /         / // __ \ / __/ _ \/ / / / / _ \/ _ \/ __  / __\      
 / /_/ / /___/ /___     _/ // / / // /_/  __/ / / / /  __/  __/ /_/ / /_         
 \____/\____/\____/    /___/_/ /_/ \__/\___/_/_/_/_/\___/\___/\__,_/\__/         
                                                                                  
                        OCR智能预审系统 - 项目初始化向导
                                  版本 2.0
EOF
    echo ""
    echo "本脚本将帮助您完成项目的初始化设置"
    echo "================================================================"
    echo ""
    
    # 初始化日志文件
    echo "初始化开始时间: $(date)" > "$INIT_LOG"
}

# 确认继续
confirm_continue() {
    echo -n "是否继续? (y/N): "
    read -r response
    if [[ ! "$response" =~ ^[Yy]$ ]]; then
        echo "初始化已取消"
        exit 0
    fi
    echo ""
}

# 检查操作系统
check_os() {
    log_step "检查操作系统"
    
    if [[ "$OSTYPE" == "linux-gnu"* ]]; then
        if [ -f /etc/os-release ]; then
            . /etc/os-release
            log_info "操作系统: $NAME $VERSION"
            OS_TYPE="linux"
            
            # 检测包管理器
            if command -v apt-get &> /dev/null; then
                PKG_MANAGER="apt"
            elif command -v yum &> /dev/null; then
                PKG_MANAGER="yum"
            elif command -v dnf &> /dev/null; then
                PKG_MANAGER="dnf"
            else
                log_warning "未检测到支持的包管理器"
                PKG_MANAGER="unknown"
            fi
        else
            log_warning "无法确定Linux发行版"
            OS_TYPE="linux"
            PKG_MANAGER="unknown"
        fi
    elif [[ "$OSTYPE" == "darwin"* ]]; then
        log_info "操作系统: macOS $(sw_vers -productVersion)"
        OS_TYPE="macos"
        PKG_MANAGER="brew"
    else
        log_error "不支持的操作系统: $OSTYPE"
        return 1
    fi
    
    log_success "操作系统检查完成"
}

# 检查并安装依赖
check_dependencies() {
    log_step "检查系统依赖"
    
    local missing_deps=()
    
    # 检查基础工具
    local base_tools=("git" "curl" "wget" "tar" "gzip")
    for tool in "${base_tools[@]}"; do
        if ! command -v "$tool" &> /dev/null; then
            missing_deps+=("$tool")
            log_warning "缺少工具: $tool"
        else
            log_success "已安装: $tool"
        fi
    done
    
    # 检查Rust
    if ! command -v rustc &> /dev/null; then
        log_warning "未安装Rust工具链"
        echo -n "是否自动安装Rust? (y/N): "
        read -r response
        if [[ "$response" =~ ^[Yy]$ ]]; then
            install_rust
        else
            missing_deps+=("rust")
        fi
    else
        local rust_version=$(rustc --version | cut -d' ' -f2)
        log_success "Rust已安装: $rust_version"
        
        # 检查cargo
        if command -v cargo &> /dev/null; then
            log_success "Cargo已安装: $(cargo --version | cut -d' ' -f2)"
        else
            missing_deps+=("cargo")
        fi
    fi
    
    # 检查musl工具链（可选）
    if ! command -v musl-gcc &> /dev/null; then
        log_warning "未安装musl工具链（用于静态编译）"
        echo -n "是否安装musl工具链? (y/N): "
        read -r response
        if [[ "$response" =~ ^[Yy]$ ]]; then
            install_musl
        fi
    else
        log_success "musl工具链已安装"
    fi
    
    # 检查其他可选依赖
    local optional_tools=("sqlite3" "yq" "jq")
    for tool in "${optional_tools[@]}"; do
        if ! command -v "$tool" &> /dev/null; then
            log_warning "未安装可选工具: $tool"
        else
            log_success "已安装: $tool"
        fi
    done
    
    if [ ${#missing_deps[@]} -gt 0 ]; then
        log_error "缺少必要的依赖: ${missing_deps[*]}"
        echo "请安装缺少的依赖后重新运行此脚本"
        return 1
    fi
}

# 安装Rust
install_rust() {
    log_info "开始安装Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    log_success "Rust安装完成"
}

# 安装musl工具链
install_musl() {
    log_info "安装musl工具链..."
    
    case "$PKG_MANAGER" in
        apt)
            sudo apt-get update
            sudo apt-get install -y musl-tools
            ;;
        yum|dnf)
            sudo $PKG_MANAGER install -y musl-libc musl-libc-devel
            ;;
        brew)
            brew install filosottile/musl-cross/musl-cross
            ;;
        *)
            log_warning "请手动安装musl工具链"
            return
            ;;
    esac
    
    # 添加musl target
    if command -v rustup &> /dev/null; then
        rustup target add x86_64-unknown-linux-musl
        log_success "musl target已添加"
    fi
}

# 创建目录结构
create_directories() {
    log_step "创建项目目录结构"
    
    local dirs=(
        "data"
        "data/storage"
        "runtime/logs"
        "runtime/preview"
        "runtime/cache"
        "runtime/temp"
        "runtime/fallback/db"
        "runtime/fallback/storage"
        "config"
        "config/rules"
        "config/backups"
        "build"
        "static/css"
        "static/js"
        "static/images"
        "static/debug/tools"
    )
    
    for dir in "${dirs[@]}"; do
        if [ ! -d "$PROJECT_ROOT/$dir" ]; then
            mkdir -p "$PROJECT_ROOT/$dir"
            log_success "创建目录: $dir"
        else
            log_info "目录已存在: $dir"
        fi
    done
    
    # 设置权限
    chmod -R 755 "$PROJECT_ROOT/runtime"
    chmod -R 755 "$PROJECT_ROOT/data"
}

# 初始化配置文件
init_config() {
    log_step "初始化配置文件"
    
    # 检查是否已有配置
    if [ -f "$PROJECT_ROOT/config.yaml" ]; then
        log_warning "配置文件已存在"
        echo -n "是否覆盖现有配置? (y/N): "
        read -r response
        if [[ ! "$response" =~ ^[Yy]$ ]]; then
            log_info "保留现有配置"
            return
        fi
    fi
    
    # 选择环境
    echo ""
    echo "请选择初始化环境:"
    echo "1) 开发环境 (development)"
    echo "2) 生产环境 (production)"
    echo -n "请输入选择 (1-2): "
    read -r env_choice
    
    case "$env_choice" in
        1)
            ENV_TYPE="development"
            ;;
        2)
            ENV_TYPE="production"
            ;;
        *)
            log_warning "无效选择，使用开发环境"
            ENV_TYPE="development"
            ;;
    esac
    
    # 使用配置管理器初始化
    if [ -x "$SCRIPT_DIR/config-manager.sh" ]; then
        "$SCRIPT_DIR/config-manager.sh" switch "$ENV_TYPE"
        log_success "配置文件已初始化为 $ENV_TYPE 环境"
    else
        # 手动复制配置
        if [ -f "$PROJECT_ROOT/config/config.$ENV_TYPE.yaml" ]; then
            cp "$PROJECT_ROOT/config/config.$ENV_TYPE.yaml" "$PROJECT_ROOT/config.yaml"
            log_success "配置文件已创建"
        else
            log_error "找不到配置模板"
        fi
    fi
    
    # 生成环境变量文件
    if [ ! -f "$PROJECT_ROOT/.env" ]; then
        log_info "生成环境变量文件..."
        "$SCRIPT_DIR/config-manager.sh" generate-env > "$PROJECT_ROOT/.env"
        log_success ".env文件已创建"
    fi
}

# 检查OCR引擎
check_ocr_engine() {
    log_step "检查OCR引擎"
    
    local ocr_dir="$PROJECT_ROOT/ocr"
    
    if [ ! -d "$ocr_dir" ]; then
        log_warning "OCR引擎目录不存在"
        echo "请手动下载并解压OCR引擎到 $ocr_dir 目录"
        echo "OCR引擎下载地址请参考项目文档"
        return
    fi
    
    # 检查关键文件
    local required_files=("paddle_ocr")
    local missing_files=0
    
    for file in "${required_files[@]}"; do
        if [ ! -f "$ocr_dir/$file" ]; then
            log_warning "缺少OCR引擎文件: $file"
            missing_files=$((missing_files + 1))
        fi
    done
    
    if [ $missing_files -eq 0 ]; then
        log_success "OCR引擎检查通过"
    else
        log_warning "OCR引擎不完整，请检查安装"
    fi
}

# 初始化数据库
init_database() {
    log_step "初始化数据库"
    
    # 这里可以添加数据库初始化逻辑
    # 目前系统会在首次运行时自动初始化
    log_info "数据库将在首次运行时自动初始化"
}

# 首次编译
first_build() {
    log_step "执行首次编译"
    
    echo -n "是否立即编译项目? (y/N): "
    read -r response
    if [[ ! "$response" =~ ^[Yy]$ ]]; then
        log_info "跳过编译步骤"
        return
    fi
    
    # 使用编译脚本
    if [ -x "$SCRIPT_DIR/build.sh" ]; then
        log_info "开始编译（开发模式）..."
        "$SCRIPT_DIR/build.sh" -m dev
    else
        log_info "使用cargo编译..."
        cd "$PROJECT_ROOT"
        cargo build
    fi
    
    if [ $? -eq 0 ]; then
        log_success "编译成功"
    else
        log_error "编译失败，请检查错误信息"
    fi
}

# 生成快速启动脚本
create_start_script() {
    log_step "创建快速启动脚本"
    
    cat > "$PROJECT_ROOT/start-dev.sh" << 'EOF'
#!/bin/bash
# OCR服务器开发环境启动脚本

cd "$(dirname "$0")"

# 设置环境变量
export RUST_LOG=${RUST_LOG:-debug}
export RUST_BACKTRACE=${RUST_BACKTRACE:-1}

# 检查配置文件
if [ ! -f "config.yaml" ]; then
    echo "错误: 配置文件不存在"
    echo "请运行 ./scripts/init.sh 初始化项目"
    exit 1
fi

# 启动服务
echo "启动OCR服务器..."
./target/debug/ocr-server

EOF
    
    chmod +x "$PROJECT_ROOT/start-dev.sh"
    log_success "快速启动脚本已创建: start-dev.sh"
}

# 显示完成信息
show_completion() {
    echo ""
    echo "================================================================"
    
    if [ "$HAS_ERROR" = true ]; then
        echo -e "${RED}初始化过程中遇到一些错误，请查看日志文件: $INIT_LOG${NC}"
        echo ""
        echo "解决错误后，您可以重新运行此脚本"
    else
        echo -e "${GREEN}🎉 项目初始化完成！${NC}"
        echo ""
        echo "下一步操作："
        echo ""
        echo "1. 编辑配置文件:"
        echo "   vim config.yaml"
        echo ""
        echo "2. 编译项目:"
        echo "   ./scripts/build.sh"
        echo ""
        echo "3. 启动服务:"
        echo "   ./start-dev.sh"
        echo ""
        echo "4. 访问服务:"
        echo "   http://localhost:31101"
        echo ""
        echo "更多信息请查看项目文档"
    fi
    
    echo "================================================================"
    echo ""
}

# 主函数
main() {
    show_welcome
    confirm_continue
    
    # 执行初始化步骤
    check_os
    check_dependencies
    create_directories
    init_config
    check_ocr_engine
    init_database
    first_build
    create_start_script
    
    # 显示完成信息
    show_completion
    
    # 保存初始化完成时间
    echo "初始化完成时间: $(date)" >> "$INIT_LOG"
}

# 运行主函数
main "$@"