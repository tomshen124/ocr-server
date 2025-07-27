#!/bin/bash

# OCR服务简化部署包构建脚本
# 跳过 wkhtmltopdf 下载，仅打包核心服务

set -e

VERSION="${VERSION:-$(date +%Y%m%d_%H%M%S)}"
RELEASE_NAME="ocr-server-debug-${VERSION}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RELEASE_DIR="$SCRIPT_DIR/releases"
PACKAGE_DIR="$RELEASE_DIR/$RELEASE_NAME"

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

echo "🚀 OCR服务调试版部署包构建"
echo "=========================="
echo "📝 说明: 调试版本，包含完整开发工具和调试功能"
echo ""

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
            echo "  CentOS/RHEL: sudo yum install musl-libc musl-libc-devel"
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
    
    # 创建生产环境的目录结构
    mkdir -p "$PACKAGE_DIR"/{bin,config,static,docs,scripts,runtime/{logs,preview,cache,temp}}
    
    # 复制应用程序到 bin/
    cp "target/x86_64-unknown-linux-musl/release/ocr-server" "$PACKAGE_DIR/bin/"
    chmod +x "$PACKAGE_DIR/bin/ocr-server"
    
    # 复制配置文件到 config/
    # 优先使用用户自定义的配置，如果没有则使用默认配置
    if [ -f "config.yaml.prod" ]; then
        cp config.yaml.prod "$PACKAGE_DIR/config/config.yaml"
        log_info "使用生产环境配置: config.yaml.prod"
    elif [ -f "config.yaml" ]; then
        cp config.yaml "$PACKAGE_DIR/config/"
        log_warning "使用开发环境配置，建议创建 config.yaml.prod 用于生产部署"
    fi
    [ -f config.example.yaml ] && cp config.example.yaml "$PACKAGE_DIR/config/"
    [ -f graph.json ] && cp graph.json "$PACKAGE_DIR/config/"
    
    # 复制配置相关的目录
    [ -d "config" ] && cp -r config/* "$PACKAGE_DIR/config/"
    [ -d "rules" ] && cp -r rules "$PACKAGE_DIR/config/"
    [ -f "matter-theme-mapping.json" ] && cp matter-theme-mapping.json "$PACKAGE_DIR/config/"
    [ -f "themes.json" ] && cp themes.json "$PACKAGE_DIR/config/"
    
    # 复制静态资源到 static/
    [ -d "static" ] && cp -r static/* "$PACKAGE_DIR/static/"
    [ -d "ocr" ] && cp -r ocr "$PACKAGE_DIR/"
    
    # 复制演示数据（如果启用了debug模式）
    if [ -d "test/demo_data" ]; then
        log_info "复制演示数据..."
        mkdir -p "$PACKAGE_DIR/runtime/preview_mappings"
        mkdir -p "$PACKAGE_DIR/runtime/preview"
        
        # 复制演示数据文件
        if [ -f "test/demo_data/preview_mappings/test-preview-id.json" ]; then
            cp test/demo_data/preview_mappings/test-preview-id.json "$PACKAGE_DIR/runtime/preview_mappings/"
        fi
        
        if [ -f "test/demo_data/preview/test-preview-id.html" ]; then
            cp test/demo_data/preview/test-preview-id.html "$PACKAGE_DIR/runtime/preview/"
        fi
        
        log_success "演示数据已复制"
    fi
    
    # 复制文档到 docs/
    [ -f "README.md" ] && cp README.md "$PACKAGE_DIR/"
    [ -f "API_REFERENCE.md" ] && cp API_REFERENCE.md "$PACKAGE_DIR/docs/"
    [ -f "TEST_GUIDE.md" ] && cp TEST_GUIDE.md "$PACKAGE_DIR/docs/"
    [ -f "DEPLOYMENT-SUMMARY.md" ] && cp DEPLOYMENT-SUMMARY.md "$PACKAGE_DIR/docs/"
    
    # 复制脚本到 scripts/
    cp ocr-server.sh "$PACKAGE_DIR/scripts/"
    [ -f log-manager.sh ] && cp log-manager.sh "$PACKAGE_DIR/scripts/"
    chmod +x "$PACKAGE_DIR/scripts"/*.sh
    
    # 创建根目录的快捷脚本
    cat > "$PACKAGE_DIR/ocr-server" << 'EOF'
#!/bin/bash
# OCR服务管理脚本（生产环境）
cd "$(dirname "$0")"
exec ./scripts/ocr-server.sh "$@"
EOF
    
    chmod +x "$PACKAGE_DIR/ocr-server"
    
    log_success "发布包创建完成"
}

# 生成安装脚本
generate_install_script() {
    log_info "生成安装脚本..."
    
    cat > "$PACKAGE_DIR/install.sh" << 'EOF'
#!/bin/bash

# OCR服务简化安装脚本
set -e

echo "🚀 OCR服务安装程序 (简化版)"
echo "=========================="

# 检查权限
if [ "$EUID" -eq 0 ]; then
    echo "⚠️  请不要使用 root 用户运行此脚本"
    exit 1
fi

# 检查配置文件
if [ -f "config/config.yaml" ]; then
    echo "✅ 配置文件检查通过"
else
    echo "❌ 配置文件缺失"
    exit 1
fi

# 检查二进制文件
if [ -f "bin/ocr-server" ]; then
    echo "✅ 应用程序检查通过"
    file bin/ocr-server
else
    echo "❌ 应用程序缺失"
    exit 1
fi

echo "✅ 安装完成！"
echo ""
echo "启动服务: ./ocr-server start"
echo "查看状态: ./ocr-server status"
echo "查看日志: ./ocr-server logs"
EOF
    
    chmod +x "$PACKAGE_DIR/install.sh"
    log_success "安装脚本已生成"
}

# 打包
create_tarball() {
    log_info "创建压缩包..."
    
    cd "$RELEASE_DIR"
    tar -czf "${RELEASE_NAME}.tar.gz" "$RELEASE_NAME"
    
    log_success "部署包已创建: releases/${RELEASE_NAME}.tar.gz"
    echo "📏 文件大小: $(du -h "${RELEASE_NAME}.tar.gz" | cut -f1)"
}

# 执行构建流程
main() {
    build_application
    create_package
    generate_install_script
    create_tarball
    
    echo ""
    log_success "简化部署包构建完成！"
    echo "📦 部署包: releases/${RELEASE_NAME}.tar.gz"
    echo "🚀 部署方法:"
    echo "   scp releases/${RELEASE_NAME}.tar.gz user@server:/tmp/"
    echo "   ssh user@server 'cd /opt && tar -xzf /tmp/${RELEASE_NAME}.tar.gz && cd ${RELEASE_NAME} && ./install.sh'"
}

main "$@" 