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
MUSL_TARGET="x86_64-unknown-linux-musl"
FEATURES=""
BUILD_MODE="debug"
ENABLE_MONITORING=false
ENABLE_STRIP=false
CREATE_PACKAGE=false
HOST_OS="$(uname -s)"

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

# 自动写入 VERSION（可通过 DISABLE_AUTO_VERSION=1 禁用）
if [[ "${DISABLE_AUTO_VERSION:-0}" != "1" ]]; then
    VERSION_STR="${MANUAL_VERSION:-$(date +%Y%m%d%H%M%S)}"
    echo "$VERSION_STR" > "$PROJECT_ROOT/VERSION"
    log_info "写入版本号: ${VERSION_STR}"
else
    log_info "DISABLE_AUTO_VERSION=1，跳过自动写入 VERSION"
fi

# 选择 Cargo 工具链（支持 CARGO_TOOLCHAIN=nightly）
if [[ -n "${CARGO_TOOLCHAIN:-}" ]]; then
    CARGO_CMD=(cargo "+${CARGO_TOOLCHAIN}")
    log_info "使用 Cargo 工具链: +${CARGO_TOOLCHAIN}"
else
    CARGO_CMD=(cargo)
fi

# 显示帮助信息
show_help() {
    cat << EOF
OCR服务器统一编译脚本 - v1.3.4 (支持智能故障转移)

用法: $0 [选项]

选项:
    -m, --mode <mode>        编译模式: dev|prod|release (默认: dev)
    -t, --target <target>    编译目标: native|musl (默认: native)
    -f, --features <feat>    启用特性，逗号分隔 (如: dm_go,monitoring,reqwest)
    -s, --strip              裁剪调试符号（减小体积）
    -p, --package            创建发布包
    -c, --clean              清理编译缓存
    -h, --help               显示帮助信息
    --prod                   一键生产打包（等效: -m release -t musl -p；自动启用 monitoring 与 reqwest，若检测到DM网关配置则启用 dm_go）
    --prod-native            一键生产打包（glibc版，等效: -m release -t native -p；自动启用 monitoring，若检测到DM网关配置则启用 dm_go）

编译模式说明:
    dev      开发模式，快速编译，包含调试信息
    prod     生产模式，优化编译，静态链接
    release  发布模式，最大优化，创建发布包

生产环境特性组合说明:
    dm_go,monitoring     - Go网关 + 监控
    monitoring           - 仅监控（默认 HTTP 下载在 MUSL 下自动开启）
    
⚠️  MUSL兼容性提醒:
    - MUSL + dm_direct 不兼容，会导致链接失败
    - MUSL环境请使用 HTTP 代理方案

示例:
    $0                           # 开发模式编译
    $0 -m prod                   # 生产模式编译
    $0 -m release -t musl -p     # 发布模式，musl静态编译，创建发布包
    $0 -f monitoring             # 启用监控特性
    $0 --prod                                        # 一键生产（MUSL静态）
    $0 --prod-native                                 # 一键生产（glibc原生）
    $0 -m release -t musl -p -f monitoring,dm_go     # 明确启用Go网关
    $0 -c                        # 清理编译缓存

注意事项:
    - MUSL静态链接与ODBC不兼容，建议使用native目标
    - 生产环境推荐: -t native -f dm_odbc,monitoring
    - 轻量部署推荐: -t musl -f monitoring

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

        if [[ "$HOST_OS" == "Darwin" ]]; then
            if ! command -v x86_64-unknown-linux-musl-gcc &> /dev/null && ! command -v musl-gcc &> /dev/null; then
                missing_deps+=("musl-cross 工具链 (运行: brew install filosottile/musl-cross/musl-cross)")
            fi
        else
            if ! command -v musl-gcc &> /dev/null; then
                missing_deps+=("musl-tools (运行: sudo apt install musl-tools)")
            fi
        fi
    fi
    
    # 检查可选工具
    if [ "$ENABLE_STRIP" == "true" ]; then
        if ! command -v strip &> /dev/null; then
            if command -v llvm-strip &> /dev/null; then
                log_info "使用 llvm-strip 进行符号裁剪"
            else
                log_warning "未检测到 strip/llvm-strip，跳过符号裁剪"
                ENABLE_STRIP=false
            fi
        fi
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
    
    # 🔧 智能特性处理：MUSL环境自动启用reqwest以确保HTTP下载功能
    if [ "$TARGET" == "musl" ] && [[ "$FEATURES" != *"reqwest"* ]]; then
        if [ -n "$FEATURES" ]; then
            FEATURES="$FEATURES,reqwest"
        else
            FEATURES="reqwest"
        fi
        log_info "🚀 MUSL环境: 自动启用reqwest特性以支持HTTP下载"
    fi
    
    # 检查MUSL+ODBC兼容性
    if [ "$TARGET" == "musl" ] && [[ "$FEATURES" == *"dm_odbc"* ]]; then
        log_warning "⚠️  检测到MUSL目标 + ODBC特性的不兼容组合"
        echo ""
        echo "🚨 MUSL静态链接与ODBC库存在兼容性问题："
        echo "  - ODBC库依赖glibc的安全函数（__sprintf_chk等）"
        echo "  - MUSL不提供这些函数，导致链接失败"
        echo ""
        echo "💡 建议的解决方案："
        echo "  1. 使用native目标: $0 -m $BUILD_MODE -t native -p -f $FEATURES"
        echo "  2. 移除ODBC特性: $0 -m $BUILD_MODE -t musl -p -f monitoring"
        echo ""
        read -p "是否继续尝试编译? (y/N): " -n 1 -r
        echo ""
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            log_info "编译已取消"
            exit 0
        fi
    fi
    
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
                if command -v x86_64-unknown-linux-musl-gcc &> /dev/null; then
                    export CC=x86_64-unknown-linux-musl-gcc
                else
                    export CC=musl-gcc
                fi
            fi
            ;;
    esac
    
    # 如果未显式指定特性，则使用默认特性集（生产化简）
    if [ -z "$FEATURES" ]; then
        FEATURES="monitoring"  # 默认启用监控
        log_info "未指定特性，使用默认特性: $FEATURES"
    fi

    # 根据环境自动启用 dm_go（检测到 DM 网关配置时）
    if [ -z "${DISABLE_DM_GO_AUTO:-}" ]; then
        if [ -n "$DM_GATEWAY_URL" ] || [ -n "$DM_GATEWAY_API_KEY" ]; then
            if [[ ",$FEATURES," != *",dm_go,"* ]]; then
                FEATURES="$FEATURES,dm_go"
                log_info "检测到DM网关环境变量，自动启用特性: dm_go"
            fi
        fi
    fi

    # 设置特性标志
    if [ -n "$FEATURES" ]; then
        export CARGO_BUILD_FLAGS="$CARGO_BUILD_FLAGS --features $FEATURES"
    fi
    
    # 设置目标平台
    if [ "$TARGET" == "musl" ]; then
        export CARGO_BUILD_FLAGS="$CARGO_BUILD_FLAGS --target $MUSL_TARGET"
        TARGET_TRIPLE=$MUSL_TARGET

        if [ -z "${CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER:-}" ]; then
            if command -v x86_64-unknown-linux-musl-gcc &> /dev/null; then
                export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=x86_64-unknown-linux-musl-gcc
                export CC_x86_64_unknown_linux_musl=x86_64-unknown-linux-musl-gcc
            elif command -v musl-gcc &> /dev/null; then
                export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc
                export CC_x86_64_unknown_linux_musl=musl-gcc
            fi
        fi

        if [ -z "${AR_x86_64_unknown_linux_musl:-}" ]; then
            if command -v llvm-ar &> /dev/null; then
                export AR_x86_64_unknown_linux_musl=llvm-ar
            elif command -v ar &> /dev/null; then
                export AR_x86_64_unknown_linux_musl=ar
            fi
        fi

        if [ -z "${RANLIB_x86_64_unknown_linux_musl:-}" ]; then
            if command -v llvm-ranlib &> /dev/null; then
                export RANLIB_x86_64_unknown_linux_musl=llvm-ranlib
            elif command -v ranlib &> /dev/null; then
                export RANLIB_x86_64_unknown_linux_musl=ranlib
            fi
        fi
    else
        TARGET_TRIPLE=$(rustc -vV 2>/dev/null | awk '/host:/ {print $2}')
        TARGET_TRIPLE=${TARGET_TRIPLE:-native}
    fi
    
    log_info "编译模式: $BUILD_MODE"
    log_info "目标平台: $TARGET_TRIPLE"
    if [ -n "$FEATURES" ]; then
        log_info "启用特性: $FEATURES"
        case "$FEATURES" in
            *"monitoring"*)
                log_info "📊 监控: 系统资源和性能指标监控"
                ;;
        esac
        case "$FEATURES" in
            *"reqwest"*)
                log_info "🌐 HTTP下载: 支持HTTP/HTTPS文件下载 (MUSL兼容)"
                ;;
        esac
        case "$FEATURES" in
            *"dm_go"*)
                log_info "🔗 数据库: 启用达梦Go网关集成"
                ;;
        esac
    fi
}

# 构建前端资源
build_frontend() {
    log_step "构建前端资源..."
    
    local build_tools_dir="$PROJECT_ROOT/build-tools"
    
    # 检查是否有Node.js环境
    if ! command -v node &> /dev/null; then
        log_warning "未找到Node.js，跳过前端构建，使用源码版本"
        ensure_static_fallback
        return
    fi
    
    # 检查构建工具目录
    if [ ! -d "$build_tools_dir" ]; then
        log_warning "未找到前端构建工具，跳过前端构建，使用源码版本"
        ensure_static_fallback
        return
    fi
    
    cd "$build_tools_dir"
    
    # 安装依赖（如果需要）
    if [ ! -d "node_modules" ]; then
        log_info "安装前端构建依赖..."
        if ! npm install; then
            log_warning "前端依赖安装失败，跳过前端构建"
            ensure_static_fallback
            return
        fi
    fi
    
    # 根据构建模式选择前端构建方式
    local frontend_build_cmd="npm run build"
    if [ "$BUILD_MODE" == "dev" ]; then
        frontend_build_cmd="npm run build:dev"
        log_info "前端开发模式构建（保留源码）"
    else
        frontend_build_cmd="npm run build:prod"
        log_info "前端生产模式构建（混淆压缩）"
    fi
    
    # 执行前端构建
    if $frontend_build_cmd; then
        log_success "前端构建成功"
        
        # 如果是生产模式，显示压缩信息
        if [ "$BUILD_MODE" != "dev" ]; then
            local static_src="$PROJECT_ROOT/static"
            local static_dist="$PROJECT_ROOT/static-dist"
            
            if [ -d "$static_src" ] && [ -d "$static_dist" ]; then
                local src_size=$(du -sh "$static_src" | cut -f1)
                local dist_size=$(du -sh "$static_dist" | cut -f1)
                log_info "前端资源大小: $src_size -> $dist_size"
            fi
        fi
    else
        log_warning "前端构建失败，使用源码版本"
        ensure_static_fallback
    fi
    
    cd "$PROJECT_ROOT"
}

# 确保有静态资源可用（降级处理）
ensure_static_fallback() {
    local static_src="$PROJECT_ROOT/static"
    local static_dist="$PROJECT_ROOT/static-dist"
    
    if [ ! -d "$static_dist" ] && [ -d "$static_src" ]; then
        log_info "复制源码版本静态资源..."
        cp -r "$static_src" "$static_dist"
    fi
}

# 编译Rust后端
build_rust_backend() {
    log_step "编译Rust后端..."
    
    # 显示编译命令
    local build_cmd=("${CARGO_CMD[@]}" build)
    if [[ -n "$CARGO_BUILD_FLAGS" ]]; then
        # shellcheck disable=SC2206
        local extra_args=( $CARGO_BUILD_FLAGS )
        build_cmd+=("${extra_args[@]}")
    fi
    log_info "执行命令: ${build_cmd[*]}"

    # 执行编译
    if "${build_cmd[@]}"; then
        log_success "后端编译成功"
    else
        log_error "后端编译失败"
        exit 1
    fi
    
    # 验证编译产物
    verify_build_output
}

# 验证编译产物
verify_build_output() {
    
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
    else
        log_warning "未检测到 ldd，跳过依赖检查 (建议在容器或Linux环境验证)"
    fi
    
    # 裁剪符号
    if [ "$ENABLE_STRIP" == "true" ] && [ "$BUILD_MODE" != "dev" ]; then
        local strip_bin="strip"
        local strip_args="-s"

        if ! command -v strip &> /dev/null; then
            if command -v llvm-strip &> /dev/null; then
                strip_bin="llvm-strip"
            else
                log_warning "未检测到 strip 工具，跳过裁剪"
                ENABLE_STRIP=false
            fi
        fi

        if [[ "$HOST_OS" == "Darwin" ]]; then
            if command -v llvm-strip &> /dev/null; then
                strip_bin="llvm-strip"
                strip_args="-s"
            else
                strip_bin="strip"
                strip_args="-x"
            fi
        fi

        if [ "$ENABLE_STRIP" == "true" ]; then
            log_step "裁剪调试符号..."
            local stripped_path="${binary_path}.stripped"
            cp "$binary_path" "$stripped_path"
            if $strip_bin $strip_args "$stripped_path"; then
                mv "$stripped_path" "$binary_path"
                log_info "裁剪后大小: $(du -h "$binary_path" | cut -f1)"
            else
                log_warning "裁剪失败，保留原始二进制"
                rm -f "$stripped_path"
            fi
        fi
    fi
    
    # 复制到build目录
    cp "$binary_path" "$BUILD_DIR/ocr-server"
    log_success "二进制文件已复制到: $BUILD_DIR/ocr-server"
}

# 执行编译主流程
do_build() {
    log_step "开始编译..."
    
    cd "$PROJECT_ROOT"
    
    # 1. 构建前端资源
    build_frontend
    
    # 2. 编译Rust后端
    build_rust_backend
}

# 创建发布包
create_package() {
    if [ "$CREATE_PACKAGE" != "true" ]; then
        return
    fi
    
    log_step "创建发布包..."
    log_info "优化发布包内容: 排除开发文档、测试文件和调试脚本"
    
    local pkg_name="ocr-server-$(date +%Y%m%d-%H%M%S)"
    local pkg_dir="$BUILD_DIR/$pkg_name"
    
    # 创建包目录结构
    mkdir -p "$pkg_dir"/{bin,config,scripts,static,data,runtime/{logs,preview,cache,temp,fallback/{db,storage}}}
    
    # 复制文件
    cp "$BUILD_DIR/ocr-server" "$pkg_dir/bin/"

    # 复制生产配置文件（优先使用config/config.yaml，其次使用config/config.template.yaml 并落地为config.yaml）
    if [ -f "$PROJECT_ROOT/config/config.yaml" ]; then
        cp "$PROJECT_ROOT/config/config.yaml" "$pkg_dir/" 2>/dev/null || true
        mkdir -p "$pkg_dir/config"
        cp "$PROJECT_ROOT/config/config.yaml" "$pkg_dir/config/config.yaml" 2>/dev/null || true
        log_info "已复制生产配置: config/config.yaml"
    elif [ -f "$PROJECT_ROOT/config/config.template.yaml" ]; then
        mkdir -p "$pkg_dir/config"
        cp "$PROJECT_ROOT/config/config.template.yaml" "$pkg_dir/config/config.template.yaml" 2>/dev/null || true
        cp "$PROJECT_ROOT/config/config.template.yaml" "$pkg_dir/config/config.yaml" 2>/dev/null || true
        cp "$PROJECT_ROOT/config/config.template.yaml" "$pkg_dir/config.yaml" 2>/dev/null || true
        log_warning "未找到config/config.yaml，已使用模板生成config.yaml"
    else
        log_warning "未找到config/config.yaml 或模板，发布包将缺少默认配置"
    fi

    if [ -f "$PROJECT_ROOT/VERSION" ]; then
        cp "$PROJECT_ROOT/VERSION" "$pkg_dir/VERSION"
        log_info "已写入版本信息: $(cat "$PROJECT_ROOT/VERSION")"
    fi

    # 规则配置已入库，发布包无需附带旧版主题或静态规则文件

    # 复制生产环境脚本（排除开发调试脚本）
    cp "$PROJECT_ROOT/scripts"/ocr-server.sh "$pkg_dir/scripts/" 2>/dev/null || true

    # 精简文档收集：仅包含 README + API + 部署说明（若存在）
    mkdir -p "$pkg_dir/docs"
    if [ -f "$PROJECT_ROOT/docs/API.md" ]; then
        cp "$PROJECT_ROOT/docs/API.md" "$pkg_dir/docs/" 2>/dev/null || true
        log_info "已包含接口文档: docs/API.md"
    fi
    if [ -f "$PROJECT_ROOT/docs/DEPLOYMENT.md" ]; then
        cp "$PROJECT_ROOT/docs/DEPLOYMENT.md" "$pkg_dir/docs/" 2>/dev/null || true
        log_info "已包含部署文档: docs/DEPLOYMENT.md"
    fi

    # 复制前端资源（优先使用混淆后的版本）
    local static_source_dir="$PROJECT_ROOT/static"
    local static_dist_dir="$PROJECT_ROOT/static-dist"
    
    # 智能选择前端资源源目录：优先使用构建后的static-dist
    if [ -d "$static_dist_dir" ]; then
        log_info "使用混淆后的前端资源: static-dist/"
        static_source_dir="$static_dist_dir"
    elif [ -d "$PROJECT_ROOT/static" ]; then
        log_warning "未找到混淆后的前端资源，使用源码版本: static/"
        static_source_dir="$PROJECT_ROOT/static"
    else
        log_error "未找到前端资源目录"
        return
    fi
    
    if [ -d "$static_source_dir" ]; then
        mkdir -p "$pkg_dir/static"
        
        # 复制所有前端资源（混淆版本或源码版本）
        cp -r "$static_source_dir"/* "$pkg_dir/static/" 2>/dev/null || true
        
        # 排除测试和调试文件
        find "$pkg_dir/static" -name "test-*" -delete 2>/dev/null || true
        find "$pkg_dir/static" -name "*debug*" -delete 2>/dev/null || true
        find "$pkg_dir/static" -name "*-backup.*" -delete 2>/dev/null || true
        
        # 显示前端资源状态
        local total_files=$(find "$pkg_dir/static" -type f | wc -l)
        local js_files=$(find "$pkg_dir/static" -name "*.js" | wc -l)
        local css_files=$(find "$pkg_dir/static" -name "*.css" | wc -l)
        local html_files=$(find "$pkg_dir/static" -name "*.html" | wc -l)
        
        log_success "前端资源已复制: $total_files 个文件 (JS:$js_files, CSS:$css_files, HTML:$html_files)"
        
        # 如果使用的是混淆版本，显示额外信息
        if [ "$static_source_dir" = "$static_dist_dir" ]; then
            log_success "生产环境使用混淆压缩后的前端代码"
        fi
    fi

    # 复制OCR引擎
    if [ -d "$PROJECT_ROOT/ocr" ]; then
        cp -r "$PROJECT_ROOT/ocr" "$pkg_dir/"
    fi

    # 注意：根目录的rules已移动到config/rules，不再需要复制

    # 验证关键文件是否复制成功
    log_step "验证关键配置文件..."
    local missing_files=()

    [ ! -f "$pkg_dir/config.yaml" ] && missing_files+=("config.yaml")
    [ ! -d "$pkg_dir/static" ] && missing_files+=("static/")

    if [ ${#missing_files[@]} -gt 0 ]; then
        log_warning "以下关键文件未找到："
        for file in "${missing_files[@]}"; do
            echo "  - $file"
        done
        log_info "这些文件在生产环境中可能是可选的"
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
echo "访问地址: http://localhost:8964"
echo "健康检查: http://localhost:8964/api/health"
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
   - 访问: http://localhost:8964/api/health
   - 查看日志: runtime/logs/

## 目录结构

- bin/         二进制文件
- config/      配置文件模板  
- scripts/     管理脚本
- static/      前端资源（生产环境）
- runtime/     运行时数据
- rules/       业务规则配置
- ocr/         OCR引擎

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
            --prod)
                BUILD_MODE="release"
                TARGET="musl"
                CREATE_PACKAGE=true
                # 默认特性由 setup_build_env 决定（monitoring + reqwest@musl + 可选dm_go）
                shift
                ;;
            --prod-native)
                BUILD_MODE="release"
                TARGET="native"
                CREATE_PACKAGE=true
                shift
                ;;
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

    if [[ "$HOST_OS" == "Darwin" ]]; then
        log_warning "检测到 macOS 环境，如需生成生产包请优先使用 ./scripts/build-with-docker.sh"
    fi
    
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
