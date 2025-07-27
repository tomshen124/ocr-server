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
