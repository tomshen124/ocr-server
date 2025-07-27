#!/bin/bash

# OCR服务器全流程测试脚本
# 使用真实数据进行端到端测试

set -e

HOST="${1:-localhost}"
PORT="${2:-31101}"
BASE_URL="http://${HOST}:${PORT}"

echo "🚀 开始OCR服务器全流程测试"
echo "📍 测试地址: $BASE_URL"
echo ""

# 1. 检查服务状态
echo "1️⃣ 检查服务状态..."
if curl -s -f "${BASE_URL}/api/health" > /dev/null; then
    echo "✅ 服务运行正常"
else
    echo "❌ 服务未启动，请先运行: cargo run --release"
    exit 1
fi

# 2. 检查qingqiu.json文件
echo ""
echo "2️⃣ 检查测试数据..."
if [ -f "qingqiu.json" ]; then
    echo "✅ qingqiu.json 文件存在"
    echo "📊 文件大小: $(wc -c < qingqiu.json) 字节"
else
    echo "❌ qingqiu.json 文件不存在"
    exit 1
fi

# 3. 检查前端测试工具
echo ""
echo "3️⃣ 检查前端测试工具..."
if curl -s -f "${BASE_URL}/static/test-tools.html" > /dev/null; then
    echo "✅ 测试工具页面可访问"
else
    echo "❌ 测试工具页面无法访问"
    exit 1
fi

# 4. 测试配置接口
echo ""
echo "4️⃣ 测试配置接口..."
config_response=$(curl -s "${BASE_URL}/api/config/frontend")
if echo "$config_response" | grep -q "success"; then
    echo "✅ 前端配置接口正常"
else
    echo "⚠️  前端配置接口可能有问题"
fi

# 5. 测试qingqiu.json访问
echo ""
echo "5️⃣ 测试qingqiu.json访问..."
if curl -s -f "${BASE_URL}/qingqiu.json" > /dev/null; then
    echo "✅ qingqiu.json 可通过HTTP访问"
else
    echo "⚠️  qingqiu.json 无法通过HTTP访问，请检查静态文件配置"
fi

echo ""
echo "🎯 自动化检查完成！"
echo ""
echo "📋 接下来请手动测试："
echo "1. 访问测试工具: ${BASE_URL}/static/test-tools.html"
echo "2. 点击'生产环境模拟'区域"
echo "3. 选择任意场景，点击'模拟第三方请求'"
echo "4. 观察是否使用了qingqiu.json的真实数据"
echo "5. 验证整个预审流程是否正常"
echo ""
echo "🔍 关键测试点："
echo "• 模拟登录功能"
echo "• 真实数据加载"
echo "• OCR处理"
echo "• 预审结果生成"
echo "• 报告下载"
echo ""
echo "✅ 测试完成后，确认无问题即可准备生产部署！"
