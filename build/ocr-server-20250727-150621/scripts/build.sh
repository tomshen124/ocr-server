#!/bin/bash

#================================================================
# OCR服务器统一编译脚本
# 
# 功能：
# 1. 多种编译模式（开发、生产、发布）
# 2. 自动依赖检查
# 3. 交叉编译支持
# 4. 特性开关管理
# 5. 编译产物打包
#================================================================

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
NC='\033[0m'

# 全局变量
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
TARGET_DIR="$PROJECT_ROOT/target"
BUILD_DIR="$PROJECT_ROOT/build"
CARGO_TOML="$PROJECT_ROOT/Cargo.toml"

# 编译配置
DEFAULT_TARGET="x86_64-unknown-linux-gnu"
MUSL_TARGET="x86_64-unknown-linux-musl"
FEATURES=""
BUILD_MODE="debug"
ENABLE_MONITORING=false
ENABLE_STRIP=false
CREATE_PACKAGE=false

# 日志函数
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[✓]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[!]${NC} $1"
}

log_error() {
    echo -e "${RED}[✗]${NC} $1"
}

log_step() {
    echo -e "${PURPLE}[>>]${NC} $1"
}

# 显示帮助信息
show_help() {
    cat << EOF
OCR服务器统一编译脚本

用法: $0 [选项]

选项:
    -m, --mode <mode>        编译模式: dev|prod|release (默认: dev)
    -t, --target <target>    编译目标: native|musl (默认: native)
    -f, --features <feat>    启用特性，逗号分隔 (如: monitoring,debug-tools)
    -s, --strip              裁剪调试符号（减小体积）
    -p, --package            创建发布包
    -c, --clean              清理编译缓存
    -h, --help               显示帮助信息

编译模式说明:
    dev      开发模式，快速编译，包含调试信息
    prod     生产模式，优化编译，静态链接
    release  发布模式，最大优化，创建发布包

示例:
    $0                           # 开发模式编译
    $0 -m prod                   # 生产模式编译
    $0 -m release -t musl -p     # 发布模式，musl静态编译，创建发布包
    $0 -f monitoring             # 启用监控特性
    $0 -c                        # 清理编译缓存

EOF
}

# 检查依赖
check_dependencies() {
    log_step "检查编译依赖..."
    
    local missing_deps=()
    
    # 检查必需的工具
    if ! command -v cargo &> /dev/null; then
        missing_deps+=("cargo (Rust工具链)")
    fi
    
    if ! command -v rustc &> /dev/null; then
        missing_deps+=("rustc (Rust编译器)")
    fi
    
    # 检查musl工具链（如果需要）
    if [ "$1" == "musl" ]; then
        if ! rustup target list --installed | grep -q "$MUSL_TARGET"; then
            missing_deps+=("musl target (运行: rustup target add $MUSL_TARGET)")
        fi
        
        if ! command -v musl-gcc &> /dev/null; then
            missing_deps+=("musl-tools (运行: sudo apt install musl-tools)")
        fi
    fi
    
    # 检查可选工具
    if ! command -v strip &> /dev/null && [ "$ENABLE_STRIP" == "true" ]; then
        log_warning "未安装strip工具，跳过符号裁剪"
        ENABLE_STRIP=false
    fi
    
    if [ ${#missing_deps[@]} -gt 0 ]; then
        log_error "缺少以下依赖："
        for dep in "${missing_deps[@]}"; do
            echo "  - $dep"
        done
        exit 1
    fi
    
    log_success "依赖检查通过"
}

# 设置编译环境
setup_build_env() {
    log_step "设置编译环境..."
    
    # 创建必要的目录
    mkdir -p "$BUILD_DIR"
    mkdir -p "$PROJECT_ROOT/runtime/logs"
    
    # 设置Rust编译优化
    case "$BUILD_MODE" in
        "dev")
            export CARGO_BUILD_FLAGS=""
            export RUSTFLAGS="-C debuginfo=2"
            ;;
        "prod"|"release")
            export CARGO_BUILD_FLAGS="--release"
            # 使用更兼容的优化选项
            export RUSTFLAGS="-C opt-level=3 -C codegen-units=1"
            
            # musl静态链接额外设置
            if [ "$TARGET" == "musl" ]; then
                export RUSTFLAGS="$RUSTFLAGS -C target-feature=+crt-static -C link-arg=-s"
                export CC=musl-gcc
            fi
            ;;
    esac
    
    # 设置特性标志
    if [ -n "$FEATURES" ]; then
        export CARGO_BUILD_FLAGS="$CARGO_BUILD_FLAGS --features $FEATURES"
    fi
    
    # 设置目标平台
    if [ "$TARGET" == "musl" ]; then
        export CARGO_BUILD_FLAGS="$CARGO_BUILD_FLAGS --target $MUSL_TARGET"
        TARGET_TRIPLE=$MUSL_TARGET
    else
        TARGET_TRIPLE=$DEFAULT_TARGET
    fi
    
    log_info "编译模式: $BUILD_MODE"
    log_info "目标平台: $TARGET_TRIPLE"
    if [ -n "$FEATURES" ]; then
        log_info "启用特性: $FEATURES"
    fi
}

# 执行编译
do_build() {
    log_step "开始编译..."
    
    cd "$PROJECT_ROOT"
    
    # 显示编译命令
    local build_cmd="cargo build $CARGO_BUILD_FLAGS"
    log_info "执行命令: $build_cmd"
    
    # 执行编译
    if $build_cmd; then
        log_success "编译成功"
    else
        log_error "编译失败"
        exit 1
    fi
    
    # 获取输出路径
    local output_dir="$TARGET_DIR"
    if [ "$TARGET" == "musl" ]; then
        output_dir="$output_dir/$MUSL_TARGET"
    fi
    
    if [ "$BUILD_MODE" == "dev" ]; then
        output_dir="$output_dir/debug"
    else
        output_dir="$output_dir/release"
    fi
    
    local binary_path="$output_dir/ocr-server"
    
    if [ ! -f "$binary_path" ]; then
        log_error "未找到编译产物: $binary_path"
        exit 1
    fi
    
    # 显示二进制信息
    log_info "二进制文件: $binary_path"
    log_info "文件大小: $(du -h "$binary_path" | cut -f1)"
    
    # 检查动态链接
    if command -v ldd &> /dev/null; then
        if ldd "$binary_path" 2>&1 | grep -q "not a dynamic executable"; then
            log_success "静态链接二进制"
        else
            log_info "动态链接库："
            ldd "$binary_path" | grep -v "linux-vdso" | head -5
        fi
    fi
    
    # 裁剪符号
    if [ "$ENABLE_STRIP" == "true" ] && [ "$BUILD_MODE" != "dev" ]; then
        log_step "裁剪调试符号..."
        local stripped_path="${binary_path}.stripped"
        strip -s "$binary_path" -o "$stripped_path"
        mv "$stripped_path" "$binary_path"
        log_info "裁剪后大小: $(du -h "$binary_path" | cut -f1)"
    fi
    
    # 复制到build目录
    cp "$binary_path" "$BUILD_DIR/ocr-server"
    log_success "二进制文件已复制到: $BUILD_DIR/ocr-server"
}

# 创建发布包
create_package() {
    if [ "$CREATE_PACKAGE" != "true" ]; then
        return
    fi
    
    log_step "创建发布包..."
    
    local pkg_name="ocr-server-$(date +%Y%m%d-%H%M%S)"
    local pkg_dir="$BUILD_DIR/$pkg_name"
    
    # 创建包目录结构
    mkdir -p "$pkg_dir"/{bin,config,scripts,docs,static,data,runtime/{logs,preview,cache,temp,fallback/{db,storage}}}
    
    # 复制文件
    cp "$BUILD_DIR/ocr-server" "$pkg_dir/bin/"

    # 复制配置文件
    cp -r "$PROJECT_ROOT/config"/*.yaml "$pkg_dir/config/" 2>/dev/null || true
    cp "$PROJECT_ROOT/config.yaml" "$pkg_dir/" 2>/dev/null || true

    # 复制主题和规则配置文件
    cp "$PROJECT_ROOT/themes.json" "$pkg_dir/" 2>/dev/null || true
    cp "$PROJECT_ROOT/matter-theme-mapping.json" "$pkg_dir/" 2>/dev/null || true

    # 复制脚本和文档
    cp -r "$PROJECT_ROOT/scripts"/*.sh "$pkg_dir/scripts/" 2>/dev/null || true
    cp -r "$PROJECT_ROOT/docs"/*.md "$pkg_dir/docs/" 2>/dev/null || true

    # 复制前端资源
    cp -r "$PROJECT_ROOT/static" "$pkg_dir/" 2>/dev/null || true

    # 复制OCR引擎和规则
    if [ -d "$PROJECT_ROOT/ocr" ]; then
        cp -r "$PROJECT_ROOT/ocr" "$pkg_dir/"
    fi

    if [ -d "$PROJECT_ROOT/rules" ]; then
        cp -r "$PROJECT_ROOT/rules" "$pkg_dir/"
    fi

    # 验证关键文件是否复制成功
    log_step "验证关键配置文件..."
    local missing_files=()

    [ ! -f "$pkg_dir/config.yaml" ] && missing_files+=("config.yaml")
    [ ! -f "$pkg_dir/themes.json" ] && missing_files+=("themes.json")
    [ ! -f "$pkg_dir/matter-theme-mapping.json" ] && missing_files+=("matter-theme-mapping.json")
    [ ! -d "$pkg_dir/rules" ] && missing_files+=("rules/")
    [ ! -d "$pkg_dir/static" ] && missing_files+=("static/")

    if [ ${#missing_files[@]} -gt 0 ]; then
        log_warning "以下关键文件未找到："
        for file in "${missing_files[@]}"; do
            echo "  - $file"
        done
    else
        log_success "所有关键配置文件已复制"
    fi
    
    # 创建启动脚本
    cat > "$pkg_dir/start.sh" << 'EOF'
#!/bin/bash
cd "$(dirname "$0")"

echo "=== OCR智能预审系统启动 ==="

# 检查关键配置文件
echo "正在检查配置文件..."
missing_configs=()

[ ! -f "config.yaml" ] && missing_configs+=("config.yaml")
[ ! -f "themes.json" ] && missing_configs+=("themes.json")
[ ! -f "matter-theme-mapping.json" ] && missing_configs+=("matter-theme-mapping.json")
[ ! -d "rules" ] && missing_configs+=("rules/")
[ ! -d "static" ] && missing_configs+=("static/")

if [ ${#missing_configs[@]} -gt 0 ]; then
    echo "❌ 缺少以下关键配置文件："
    for config in "${missing_configs[@]}"; do
        echo "  - $config"
    done
    echo "请确保发布包完整，或从源码目录复制缺失文件"
    exit 1
fi

echo "✅ 配置文件检查通过"

# 确保数据库文件存在
echo "正在检查数据库文件..."
if [ ! -f "data/ocr.db" ]; then
    echo "创建主数据库文件: data/ocr.db"
    mkdir -p data
    touch data/ocr.db
fi

if [ ! -f "runtime/fallback/db/fallback.db" ]; then
    echo "创建故障转移数据库文件: runtime/fallback/db/fallback.db"
    mkdir -p runtime/fallback/db
    touch runtime/fallback/db/fallback.db
fi

echo "✅ 数据库文件检查完成"

# 设置环境变量
export RUST_LOG=${RUST_LOG:-info}

echo "🚀 启动OCR服务..."
echo "访问地址: http://localhost:31101"
echo "健康检查: http://localhost:31101/api/health"
echo ""

./bin/ocr-server
EOF
    chmod +x "$pkg_dir/start.sh"
    
    # 创建README
    cat > "$pkg_dir/README.md" << EOF
# OCR智能预审系统

版本: $(date +%Y%m%d)
编译模式: $BUILD_MODE
目标平台: $TARGET_TRIPLE

## 快速开始

1. 配置系统
   - 编辑 config/config.yaml
   - 或使用环境变量覆盖配置

2. 启动服务
   \`\`\`bash
   ./start.sh
   \`\`\`

3. 检查服务
   - 访问: http://localhost:31101/api/health
   - 查看日志: runtime/logs/

## 目录结构

- bin/         二进制文件
- config/      配置文件
- scripts/     管理脚本
- static/      前端资源
- runtime/     运行时数据
- docs/        文档

EOF
    
    # 打包
    cd "$BUILD_DIR"
    tar -czf "$pkg_name.tar.gz" "$pkg_name"
    
    log_success "发布包已创建: $BUILD_DIR/$pkg_name.tar.gz"
    log_info "包大小: $(du -h "$BUILD_DIR/$pkg_name.tar.gz" | cut -f1)"
}

# 清理编译缓存
clean_build() {
    log_step "清理编译缓存..."
    
    cd "$PROJECT_ROOT"
    cargo clean
    rm -rf "$BUILD_DIR"
    
    log_success "清理完成"
}

# 解析命令行参数
parse_args() {
    while [[ $# -gt 0 ]]; do
        case $1 in
            -m|--mode)
                case "$2" in
                    dev|development)
                        BUILD_MODE="dev"
                        ;;
                    prod|production)
                        BUILD_MODE="prod"
                        ;;
                    release)
                        BUILD_MODE="release"
                        CREATE_PACKAGE=true
                        ;;
                    *)
                        log_error "无效的编译模式: $2"
                        exit 1
                        ;;
                esac
                shift 2
                ;;
            -t|--target)
                case "$2" in
                    native)
                        TARGET="native"
                        ;;
                    musl)
                        TARGET="musl"
                        ;;
                    *)
                        log_error "无效的目标平台: $2"
                        exit 1
                        ;;
                esac
                shift 2
                ;;
            -f|--features)
                FEATURES="$2"
                shift 2
                ;;
            -s|--strip)
                ENABLE_STRIP=true
                shift
                ;;
            -p|--package)
                CREATE_PACKAGE=true
                shift
                ;;
            -c|--clean)
                clean_build
                exit 0
                ;;
            -h|--help)
                show_help
                exit 0
                ;;
            *)
                log_error "未知参数: $1"
                show_help
                exit 1
                ;;
        esac
    done
}

# 主函数
main() {
    echo "================================"
    echo "OCR服务器统一编译脚本"
    echo "================================"
    echo ""
    
    # 解析参数
    parse_args "$@"
    
    # 默认值
    TARGET=${TARGET:-native}
    
    # 根据模式设置默认值
    if [ "$BUILD_MODE" == "prod" ] || [ "$BUILD_MODE" == "release" ]; then
        TARGET=${TARGET:-musl}
    fi
    
    # 执行编译流程
    check_dependencies "$TARGET"
    setup_build_env
    do_build
    create_package
    
    echo ""
    log_success "编译完成！"
    
    # 显示下一步提示
    echo ""
    echo "下一步："
    if [ "$BUILD_MODE" == "dev" ]; then
        echo "  运行: $BUILD_DIR/ocr-server"
    else
        echo "  部署: 将 $BUILD_DIR/ocr-server 复制到目标服务器"
    fi
    
    if [ "$CREATE_PACKAGE" == "true" ]; then
        echo "  发布包: $BUILD_DIR/ocr-server-*.tar.gz"
    fi
}

# 运行主函数
main "$@"