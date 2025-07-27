#!/bin/bash

# OCR服务生产环境部署包构建脚本
# 专用于生产环境，不包含调试工具

set -e

VERSION="${VERSION:-$(date +%Y%m%d_%H%M%S)}"
RELEASE_NAME="ocr-server-release-${VERSION}"
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

echo "🚀 OCR服务生产环境部署包构建"
echo "============================"
echo "📝 说明: 生产版本，已优化并移除调试工具"
echo ""

# 构建应用程序
build_application() {
    log_info "构建生产环境应用程序..."
    
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
    
    # 执行release构建（带优化）
    log_info "执行 musl release 构建（带监控功能）..."
    cargo build --release --target x86_64-unknown-linux-musl --features monitoring
    
    if [ ! -f "target/x86_64-unknown-linux-musl/release/ocr-server" ]; then
        log_error "应用程序构建失败"
        return 1
    fi
    
    log_success "生产环境应用程序构建完成"
}

# 创建发布包
create_package() {
    log_info "创建生产环境发布包..."
    
    # 清理并创建目录
    rm -rf "$PACKAGE_DIR"
    mkdir -p "$PACKAGE_DIR"
    
    # 创建生产环境的目录结构
    mkdir -p "$PACKAGE_DIR"/{bin,config,static,docs,scripts,runtime/{logs,preview,cache,temp}}
    
    # 复制应用程序到 bin/
    cp "target/x86_64-unknown-linux-musl/release/ocr-server" "$PACKAGE_DIR/bin/"
    chmod +x "$PACKAGE_DIR/bin/ocr-server"
    
    # 复制生产环境配置文件
    if [ -f "config.yaml.prod" ]; then
        cp config.yaml.prod "$PACKAGE_DIR/config/config.yaml"
        log_info "✅ 使用生产环境配置: config.yaml.prod"
    else
        log_error "❌ 缺少生产环境配置文件: config.yaml.prod"
        log_error "请先创建 config.yaml.prod 文件"
        return 1
    fi
    
    # 复制配置相关文件
    [ -f config.example.yaml ] && cp config.example.yaml "$PACKAGE_DIR/config/"
    [ -f graph.json ] && cp graph.json "$PACKAGE_DIR/config/"
    
    # 复制配置相关的目录
    [ -d "config" ] && cp -r config/* "$PACKAGE_DIR/config/"
    [ -d "rules" ] && cp -r rules "$PACKAGE_DIR/config/"
    [ -f "matter-theme-mapping.json" ] && cp matter-theme-mapping.json "$PACKAGE_DIR/config/"
    [ -f "themes.json" ] && cp themes.json "$PACKAGE_DIR/config/"
    
    # 复制静态资源（只复制生产版本，排除debug工具）
    log_info "复制生产环境静态资源..."
    mkdir -p "$PACKAGE_DIR/static"
    
    # 复制必要的静态资源
    [ -d "static/css" ] && cp -r static/css "$PACKAGE_DIR/static/"
    [ -d "static/js" ] && cp -r static/js "$PACKAGE_DIR/static/"
    [ -d "static/prod" ] && cp -r static/prod "$PACKAGE_DIR/static/"
    [ -f "static/index.html" ] && cp static/index.html "$PACKAGE_DIR/static/"
    [ -f "static/login.html" ] && cp static/login.html "$PACKAGE_DIR/static/"
    [ -f "static/monitor.html" ] && cp static/monitor.html "$PACKAGE_DIR/static/"
    
    log_warning "⚠️ 已排除 static/debug/ 目录（生产环境不需要）"
    
    # 复制OCR引擎
    [ -d "ocr" ] && cp -r ocr "$PACKAGE_DIR/"
    
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
    
    log_success "生产环境发布包创建完成"
}

# 生成安装脚本
generate_install_script() {
    log_info "生成生产环境安装脚本..."
    
    cat > "$PACKAGE_DIR/install.sh" << 'EOF'
#!/bin/bash

# OCR服务生产环境安装脚本
set -e

echo "🚀 OCR服务安装程序 (生产环境)"
echo "============================"

# 检查权限
if [ "$EUID" -eq 0 ]; then
    echo "⚠️  请不要使用 root 用户运行此脚本"
    exit 1
fi

# 检查配置文件
if [ -f "config/config.yaml" ]; then
    echo "✅ 配置文件检查通过"
    
    # 检查生产环境配置
    if grep -q "enable_mock_login: false" config/config.yaml && grep -q "debug:" config/config.yaml; then
        echo "✅ 生产环境配置检查通过"
    else
        echo "⚠️  警告: 配置文件可能不是生产环境版本"
        echo "请确认以下设置："
        echo "  - debug.enabled: false"
        echo "  - debug.enable_mock_login: false"
        echo "  - third_party_access.enabled: true"
    fi
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

echo ""
echo "✅ 生产环境安装完成！"
echo ""
echo "🔧 配置提醒："
echo "  1. 修改 config/config.yaml 中的域名配置"
echo "  2. 修改数据库连接信息"
echo "  3. 修改OSS配置信息"
echo "  4. 修改第三方客户端密钥"
echo ""
echo "🚀 服务管理："
echo "  启动服务: ./ocr-server start"
echo "  查看状态: ./ocr-server status"
echo "  查看日志: ./ocr-server logs"
echo "  监控界面: http://your-domain:31101/static/monitor.html"
EOF
    
    chmod +x "$PACKAGE_DIR/install.sh"
    log_success "生产环境安装脚本已生成"
}

# 版本信息
update_version() {
    log_info "更新版本信息..."
    
    # 创建版本信息文件
    cat > "$PACKAGE_DIR/VERSION" << EOF
智能预审系统 v1.3.0
构建时间: $(date '+%Y-%m-%d %H:%M:%S')
构建版本: $VERSION
构建类型: RELEASE (生产环境)
编译器: $(rustc --version)
特性: 静态链接(musl) + 监控功能
EOF
    
    log_success "版本信息已更新"
}

# 打包
create_tarball() {
    log_info "创建压缩包..."
    
    cd "$RELEASE_DIR"
    tar -czf "${RELEASE_NAME}.tar.gz" "$RELEASE_NAME"
    
    log_success "生产环境部署包已创建: releases/${RELEASE_NAME}.tar.gz"
    echo "📏 文件大小: $(du -h "${RELEASE_NAME}.tar.gz" | cut -f1)"
}

# 执行构建流程
main() {
    build_application
    create_package
    generate_install_script
    update_version
    create_tarball
    
    echo ""
    log_success "生产环境部署包构建完成！"
    echo "📦 部署包: releases/${RELEASE_NAME}.tar.gz"
    echo "🎯 特点: 生产优化、无调试工具、监控功能"
    echo "🚀 部署方法:"
    echo "   scp releases/${RELEASE_NAME}.tar.gz user@server:/tmp/"
    echo "   ssh user@server 'cd /opt && tar -xzf /tmp/${RELEASE_NAME}.tar.gz && cd ${RELEASE_NAME} && ./install.sh'"
}

main "$@" 