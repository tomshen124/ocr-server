#!/bin/bash

# OCR服务离线部署包构建脚本
# 整合了构建、打包、依赖处理的完整流程

set -e

VERSION="${VERSION:-$(date +%Y%m%d_%H%M%S)}"
RELEASE_NAME="ocr-server-offline-${VERSION}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RELEASE_DIR="$SCRIPT_DIR/releases"
PACKAGE_DIR="$RELEASE_DIR/$RELEASE_NAME"
WKHTMLTOPDF_DIR="$PACKAGE_DIR/wkhtmltopdf"

# 颜色定义
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

log_info() { echo -e "${BLUE}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[SUCCESS]${NC} $1"; }
log_warning() { echo -e "${YELLOW}[WARNING]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

echo "🚀 OCR服务离线部署包构建"
echo "========================="

# 创建wkhtmltopdf下载目录
mkdir -p wkhtmltopdf-downloads

# wkhtmltopdf下载配置
declare -A WKHTMLTOPDF_PACKAGES=(
    ["wkhtmltox-centos7.x86_64.rpm"]="https://github.com/wkhtmltopdf/packaging/releases/download/0.12.6.1-2/wkhtmltox-0.12.6.1-2.centos7.x86_64.rpm"
    ["wkhtmltox-centos8.x86_64.rpm"]="https://github.com/wkhtmltopdf/packaging/releases/download/0.12.6.1-2/wkhtmltox-0.12.6.1-2.centos8.x86_64.rpm"
    ["wkhtmltox-ubuntu20.amd64.deb"]="https://github.com/wkhtmltopdf/packaging/releases/download/0.12.6.1-2/wkhtmltox_0.12.6.1-2.focal_amd64.deb"
    ["wkhtmltox-ubuntu22.amd64.deb"]="https://github.com/wkhtmltopdf/packaging/releases/download/0.12.6.1-2/wkhtmltox_0.12.6.1-2.jammy_amd64.deb"
)

# 检查/下载 wkhtmltopdf 包
check_or_download_wkhtmltopdf() {
    log_info "检查 wkhtmltopdf 安装包..."
    
    local missing_packages=()
    local all_exist=true
    
    for package in "${!WKHTMLTOPDF_PACKAGES[@]}"; do
        local filepath="wkhtmltopdf-downloads/$package"
        if [ -f "$filepath" ] && [ -s "$filepath" ]; then
            log_success "已存在: $package ($(du -h "$filepath" | cut -f1))"
        else
            missing_packages+=("$package")
            all_exist=false
        fi
    done
    
    if [ "$all_exist" = true ]; then
        log_success "所有 wkhtmltopdf 包已准备就绪"
        return 0
    fi
    
    log_warning "缺失 ${#missing_packages[@]} 个安装包"
    
    for package in "${missing_packages[@]}"; do
        local url="${WKHTMLTOPDF_PACKAGES[$package]}"
        local filepath="wkhtmltopdf-downloads/$package"
        
        log_info "下载: $package"
        if command -v wget >/dev/null 2>&1; then
            wget -q --show-progress "$url" -O "$filepath" || {
                log_error "下载失败: $package"
                return 1
            }
        elif command -v curl >/dev/null 2>&1; then
            curl -L "$url" -o "$filepath" || {
                log_error "下载失败: $package"
                return 1
            }
        else
            log_error "未找到 wget 或 curl"
            return 1
        fi
    done
    
    log_success "所有安装包下载完成！"
}

# 构建应用程序
build_application() {
    log_info "构建应用程序..."
    
    if [ ! -f "target/x86_64-unknown-linux-musl/release/ocr-server" ]; then
        log_info "执行 musl 构建..."
        
        # 检查 musl 目标
        if ! rustup target list --installed | grep -q x86_64-unknown-linux-musl; then
            log_info "安装 musl 编译目标..."
            rustup target add x86_64-unknown-linux-musl
        fi
        
        # 检查 musl-gcc
        if ! command -v musl-gcc &> /dev/null; then
            log_error "未找到 musl-gcc，请先安装："
            echo "  Ubuntu/Debian: sudo apt install musl-tools"
            return 1
        fi
        
        cargo build --release --target x86_64-unknown-linux-musl
    fi
    
    if [ ! -f "target/x86_64-unknown-linux-musl/release/ocr-server" ]; then
        log_error "应用程序构建失败"
        return 1
    fi
    
    log_success "应用程序构建完成"
}

# 创建发布包
create_package() {
    log_info "创建发布包..."
    
    # 清理并创建目录
    rm -rf "$PACKAGE_DIR"
    mkdir -p "$PACKAGE_DIR"
    mkdir -p "$WKHTMLTOPDF_DIR"
    
    # 创建生产环境的目录结构
    mkdir -p "$PACKAGE_DIR"/{bin,config,static,docs,scripts,runtime/{logs,preview,cache,temp}}
    
    # 复制应用程序到 bin/
    cp "target/x86_64-unknown-linux-musl/release/ocr-server" "$PACKAGE_DIR/bin/"
    chmod +x "$PACKAGE_DIR/bin/ocr-server"
    
    # 复制配置文件
    if [ -f "config.yaml.prod" ]; then
        cp config.yaml.prod "$PACKAGE_DIR/config/config.yaml"
    elif [ -f "config.yaml" ]; then
        cp config.yaml "$PACKAGE_DIR/config/"
    fi
    [ -f config.example.yaml ] && cp config.example.yaml "$PACKAGE_DIR/config/"
    [ -f graph.json ] && cp graph.json "$PACKAGE_DIR/config/"
    
    # 复制配置目录
    [ -d "config" ] && cp -r config/* "$PACKAGE_DIR/config/"
    [ -d "rules" ] && cp -r rules "$PACKAGE_DIR/config/"
    [ -f "matter-theme-mapping.json" ] && cp matter-theme-mapping.json "$PACKAGE_DIR/config/"
    [ -f "themes.json" ] && cp themes.json "$PACKAGE_DIR/config/"
    
    # 复制静态资源
    [ -d "static" ] && cp -r static/* "$PACKAGE_DIR/static/"
    [ -d "ocr" ] && cp -r ocr "$PACKAGE_DIR/"
    
    # 复制wkhtmltopdf安装包
    cp wkhtmltopdf-downloads/* "$WKHTMLTOPDF_DIR/"
    
    # 复制脚本
    cp ocr-server.sh "$PACKAGE_DIR/scripts/"
    [ -f log-manager.sh ] && cp log-manager.sh "$PACKAGE_DIR/scripts/"
    chmod +x "$PACKAGE_DIR/scripts"/*.sh
    
    # 创建根目录脚本
    cat > "$PACKAGE_DIR/ocr-server" << 'EOF'
#!/bin/bash
cd "$(dirname "$0")"
exec ./scripts/ocr-server.sh "$@"
EOF
    chmod +x "$PACKAGE_DIR/ocr-server"
    
    log_success "发布包创建完成"
}

# 执行构建流程
main() {
    check_or_download_wkhtmltopdf
    build_application
    create_package
    
    cd "$RELEASE_DIR"
    tar -czf "${RELEASE_NAME}.tar.gz" "$RELEASE_NAME"
    
    echo ""
    log_success "离线部署包构建完成！"
    echo "📦 部署包: releases/${RELEASE_NAME}.tar.gz"
    echo "📏 文件大小: $(du -h "${RELEASE_NAME}.tar.gz" | cut -f1)"
}

main "$@" 